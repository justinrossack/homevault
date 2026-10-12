//! Benutzerverwaltung und Ersteinrichtung.
//!
//! Benutzer liegen in SQLite. Nach jeder Änderung wird zusätzlich
//! `<config>/smbpasswd` geschrieben, aus der der Samba-Container die Logins
//! für das Netzlaufwerk liest (gleiche Zugangsdaten wie im Web).

use std::{
    io::Write,
    path::{Path, PathBuf},
};

use argon2::{
    Argon2, PasswordHash, PasswordHasher, PasswordVerifier,
    password_hash::{SaltString, rand_core::OsRng},
};
use axum::{
    Extension, Json,
    extract::{Path as UrlPath, State},
    http::header,
    response::{IntoResponse, Response},
};
use md4::{Digest, Md4};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{
    AppState,
    auth::{self, CurrentUser},
    db::{self, Db},
    error::AppError,
    paths,
};

/// Erste UID für Samba-Benutzer im Samba-Container.
const FIRST_UID: i64 = 20000;
const MIN_PASSWORD: usize = 4;
pub const DEFAULT_SERVER_NAME: &str = "homevault";

#[derive(Serialize, Clone)]
pub struct UserInfo {
    #[serde(skip)]
    pub id: i64,
    pub name: String,
    pub admin: bool,
}

pub fn valid_username(name: &str) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(|c| c.is_ascii_lowercase())
        && name.len() <= 32
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
        && name != "root"
}

fn check_username(name: &str) -> Result<(), AppError> {
    if valid_username(name) {
        Ok(())
    } else {
        Err(AppError::BadRequest(
            "Benutzername: Kleinbuchstaben, Ziffern, _ und -, mit Buchstabe beginnend".into(),
        ))
    }
}

fn check_password_rules(pw: &str) -> Result<(), AppError> {
    if pw.chars().count() < MIN_PASSWORD {
        return Err(AppError::BadRequest(format!(
            "Passwort muss mindestens {MIN_PASSWORD} Zeichen haben"
        )));
    }
    Ok(())
}

/// Argon2-Hash fürs Web und NT-Hash (MD4 über UTF-16LE) für Samba.
fn hash_password(pw: &str) -> Result<(String, String), AppError> {
    let salt = SaltString::generate(&mut OsRng);
    let phc = Argon2::default()
        .hash_password(pw.as_bytes(), &salt)
        .map_err(|e| AppError::Internal(e.to_string()))?
        .to_string();
    let utf16: Vec<u8> = pw.encode_utf16().flat_map(|c| c.to_le_bytes()).collect();
    let nt: String = Md4::digest(&utf16).iter().map(|b| format!("{b:02X}")).collect();
    Ok((phc, nt))
}

/// Argon2 ist absichtlich langsam, daher nicht auf dem Async-Thread.
async fn hash_blocking(pw: String) -> Result<(String, String), AppError> {
    tokio::task::spawn_blocking(move || hash_password(&pw))
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?
}

// ---------- Datenbank ----------

pub fn user_count(conn: &Connection) -> rusqlite::Result<i64> {
    conn.query_row("SELECT count(*) FROM users", [], |r| r.get(0))
}

pub fn find(conn: &Connection, name: &str) -> rusqlite::Result<Option<UserInfo>> {
    conn.query_row(
        "SELECT id, name, admin FROM users WHERE name = ?",
        [name],
        |r| Ok(UserInfo { id: r.get(0)?, name: r.get(1)?, admin: r.get(2)? }),
    )
    .optional()
}

fn insert_user(conn: &Connection, name: &str, admin: bool, phc: &str, nt: &str) -> Result<i64, AppError> {
    let uid: i64 = conn.query_row(
        "SELECT coalesce(max(uid) + 1, ?) FROM users",
        [FIRST_UID],
        |r| r.get(0),
    )?;
    conn.execute(
        "INSERT INTO users (name, admin, uid, password_hash, nt_hash, created_at)
         VALUES (?, ?, ?, ?, ?, ?)",
        params![name, admin, uid, phc, nt, db::now()],
    )
    .map_err(|e| match AppError::from(e) {
        AppError::Conflict(_) => AppError::Conflict("Benutzer existiert bereits".into()),
        other => other,
    })?;
    Ok(conn.last_insert_rowid())
}

/// Schreibt die Samba-Passwortdatei aus der Datenbank neu.
/// Format pro Zeile: name:uid:LM-Hash:NT-Hash:[Flags]:LCT-Zeit:
pub fn write_smbpasswd(conn: &Connection, config_dir: &Path) -> Result<(), AppError> {
    let mut stmt = conn.prepare("SELECT name, uid, nt_hash FROM users ORDER BY uid")?;
    let rows = stmt.query_map([], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?, r.get::<_, String>(2)?))
    })?;
    let mut out = String::new();
    for row in rows {
        let (name, uid, nt) = row?;
        out.push_str(&format!(
            "{name}:{uid}:XXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX:{nt}:[U          ]:LCT-{:08X}:\n",
            db::now()
        ));
    }
    write_private(&config_dir.join("smbpasswd"), out.as_bytes())
}

/// Schreibt atomar mit Rechten 600 (enthält Passwort-Hashes).
fn write_private(path: &Path, data: &[u8]) -> Result<(), AppError> {
    let tmp = path.with_extension("tmp");
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut opts, 0o600);
    let mut f = opts.open(&tmp)?;
    f.write_all(data)?;
    f.sync_all()?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

fn ensure_home(data_dir: &Path, name: &str) -> Result<(), AppError> {
    std::fs::create_dir_all(data_dir.join(paths::PRIVATE).join(name))?;
    Ok(())
}

/// Programmstart: alte users.json übernehmen, optional Admin aus der
/// Umgebung anlegen, Ordner und Samba-Datei aktuell halten.
pub fn startup(
    db: &Db,
    data_dir: &Path,
    config_dir: &Path,
    bootstrap: Option<(String, String)>,
) -> Result<(), String> {
    db.blocking(|conn| -> Result<(), AppError> {
        let legacy = config_dir.join("users.json");
        if user_count(conn)? == 0 && legacy.exists() {
            import_legacy_json(conn, &legacy)?;
        }
        if user_count(conn)? == 0 {
            if let Some((name, pw)) = bootstrap {
                check_username(&name)?;
                check_password_rules(&pw)?;
                let (phc, nt) = hash_password(&pw)?;
                insert_user(conn, &name, true, &phc, &nt)?;
                tracing::info!("Erster Admin '{name}' aus ADMIN_USER/ADMIN_PASSWORD angelegt");
            } else {
                tracing::info!("Noch keine Benutzer: Einrichtung im Browser öffnen");
            }
        }
        let names: Vec<String> = conn
            .prepare("SELECT name FROM users")?
            .query_map([], |r| r.get(0))?
            .collect::<Result<_, _>>()?;
        for n in names {
            ensure_home(data_dir, &n)?;
        }
        write_smbpasswd(conn, config_dir)
    })
    .map_err(|e| e.to_string())
}

/// Übernahme aus der früheren users.json (vor SQLite).
fn import_legacy_json(conn: &mut Connection, file: &Path) -> Result<(), AppError> {
    #[derive(Deserialize)]
    struct Legacy {
        users: Vec<LegacyUser>,
    }
    #[derive(Deserialize)]
    struct LegacyUser {
        name: String,
        admin: bool,
        uid: i64,
        password_hash: String,
        nt_hash: String,
    }
    let data: Legacy = serde_json::from_slice(&std::fs::read(file)?)
        .map_err(|e| AppError::Internal(format!("users.json: {e}")))?;
    let tx = conn.transaction()?;
    for u in &data.users {
        tx.execute(
            "INSERT INTO users (name, admin, uid, password_hash, nt_hash, created_at) VALUES (?, ?, ?, ?, ?, ?)",
            params![u.name, u.admin, u.uid, u.password_hash, u.nt_hash, db::now()],
        )?;
    }
    tx.commit()?;
    std::fs::rename(file, file.with_extension("json.migriert"))?;
    tracing::info!("{} Benutzer aus users.json in die Datenbank übernommen", data.users.len());
    Ok(())
}

/// Prüft die Zugangsdaten. Rechnet auch bei unbekanntem Namen einen Hash,
/// damit die Antwortzeit nicht verrät, ob es den Benutzer gibt.
pub async fn verify(db: &std::sync::Arc<Db>, name: &str, pw: &str) -> Result<Option<UserInfo>, AppError> {
    let name = name.trim().to_lowercase();
    let found = db
        .call(move |c| {
            Ok(c.query_row(
                "SELECT id, name, admin, password_hash FROM users WHERE name = ?",
                [&name],
                |r| Ok((UserInfo { id: r.get(0)?, name: r.get(1)?, admin: r.get(2)? }, r.get::<_, String>(3)?)),
            )
            .optional()?)
        })
        .await?;
    let pw = pw.to_owned();
    tokio::task::spawn_blocking(move || {
        static DUMMY: std::sync::OnceLock<String> = std::sync::OnceLock::new();
        let dummy = DUMMY.get_or_init(|| hash_password("dummy").map(|h| h.0).unwrap_or_default());
        let hash = found.as_ref().map_or(dummy.as_str(), |(_, h)| h.as_str());
        let ok = PasswordHash::new(hash)
            .is_ok_and(|parsed| Argon2::default().verify_password(pw.as_bytes(), &parsed).is_ok());
        Ok(found.filter(|_| ok).map(|(u, _)| u))
    })
    .await
    .map_err(|e| AppError::Internal(e.to_string()))?
}

// ---------- HTTP: Ersteinrichtung ----------

#[derive(Deserialize)]
pub struct SetupBody {
    server_name: Option<String>,
    name: String,
    password: String,
}

/// Legt beim allerersten Start den Admin an und meldet ihn direkt an.
/// Funktioniert nur, solange es noch keinen Benutzer gibt.
pub async fn setup(State(state): State<AppState>, Json(b): Json<SetupBody>) -> Result<Response, AppError> {
    let name = b.name.trim().to_lowercase();
    check_username(&name)?;
    check_password_rules(&b.password)?;
    let (phc, nt) = hash_blocking(b.password).await?;
    let server_name = b
        .server_name
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| DEFAULT_SERVER_NAME.into());
    let (data_dir, config_dir) = (state.data_dir.clone(), state.config_dir.clone());

    let token = state
        .db
        .call(move |conn| {
            let tx = conn.transaction()?;
            // innerhalb der Transaktion prüfen, damit zwei gleichzeitige
            // Einrichtungen nicht beide durchkommen
            if user_count(&tx)? > 0 {
                return Err(AppError::Forbidden);
            }
            let id = insert_user(&tx, &name, true, &phc, &nt)?;
            db::set_setting(&tx, "server_name", &server_name)?;
            let token = auth::create_session(&tx, id)?;
            tx.commit()?;
            ensure_home(&data_dir, &name)?;
            write_smbpasswd(conn, &config_dir)?;
            tracing::info!("Einrichtung abgeschlossen, Admin '{name}' angelegt");
            Ok(token)
        })
        .await?;

    let mut res = Json(json!({ "ok": true })).into_response();
    res.headers_mut().insert(header::SET_COOKIE, auth::session_cookie(&token));
    Ok(res)
}

// ---------- HTTP: Einstellungen ----------

#[derive(Deserialize)]
pub struct SettingsBody {
    server_name: Option<String>,
    trash_days: Option<i64>,
    auto_transcode: Option<bool>,
    /// Eingangs-Ordner automatisch einsortieren
    inbox_enabled: Option<bool>,
}

/// Einstellungen für die Einstellungsseite (Admin).
pub async fn get_settings(
    State(state): State<AppState>,
    Extension(me): Extension<CurrentUser>,
) -> Result<Json<Value>, AppError> {
    require_admin(&me)?;
    let (name, days, auto, inbox) = state
        .db
        .call(|c| {
            Ok((
                db::get_setting(c, "server_name")?.unwrap_or_else(|| DEFAULT_SERVER_NAME.into()),
                crate::trash::trash_days(c),
                db::get_setting(c, "auto_transcode")?.as_deref() != Some("0"),
                crate::inbox::enabled(c),
            ))
        })
        .await?;
    Ok(Json(json!({ "server_name": name, "trash_days": days, "auto_transcode": auto, "inbox_enabled": inbox })))
}

pub async fn update_settings(
    State(state): State<AppState>,
    Extension(me): Extension<CurrentUser>,
    Json(b): Json<SettingsBody>,
) -> Result<Json<Value>, AppError> {
    require_admin(&me)?;
    state
        .db
        .call(move |conn| {
            if let Some(n) = b.server_name.map(|s| s.trim().to_owned()) {
                if n.is_empty() || n.chars().count() > 40 {
                    return Err(AppError::BadRequest("Servername: 1 bis 40 Zeichen".into()));
                }
                db::set_setting(conn, "server_name", &n)?;
            }
            if let Some(days) = b.trash_days {
                if !(1..=365).contains(&days) {
                    return Err(AppError::BadRequest("Papierkorb: 1 bis 365 Tage".into()));
                }
                db::set_setting(conn, "trash_days", &days.to_string())?;
            }
            if let Some(auto) = b.auto_transcode {
                db::set_setting(conn, "auto_transcode", if auto { "1" } else { "0" })?;
            }
            if let Some(on) = b.inbox_enabled {
                db::set_setting(conn, "inbox_enabled", if on { "1" } else { "0" })?;
            }
            Ok(())
        })
        .await?;
    state.inbox.wake();
    Ok(Json(json!({ "ok": true })))
}

// ---------- HTTP: Benutzer ----------

fn require_admin(user: &CurrentUser) -> Result<(), AppError> {
    if user.admin { Ok(()) } else { Err(AppError::Forbidden) }
}

pub async fn list(
    State(state): State<AppState>,
    Extension(me): Extension<CurrentUser>,
) -> Result<Json<Value>, AppError> {
    require_admin(&me)?;
    let users = state
        .db
        .call(|c| {
            let mut stmt = c.prepare(
                "SELECT name, admin, created_at, quota_bytes FROM users ORDER BY admin DESC, name",
            )?;
            let rows = stmt
                .query_map([], |r| {
                    Ok(json!({
                        "name": r.get::<_, String>(0)?,
                        "admin": r.get::<_, bool>(1)?,
                        "created_at": r.get::<_, i64>(2)?,
                        "quota_bytes": r.get::<_, Option<i64>>(3)?,
                    }))
                })?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
        .await?;
    Ok(Json(json!({ "users": users })))
}

#[derive(Deserialize)]
pub struct CreateBody {
    name: String,
    password: String,
    #[serde(default)]
    admin: bool,
}

pub async fn create(
    State(state): State<AppState>,
    Extension(me): Extension<CurrentUser>,
    Json(b): Json<CreateBody>,
) -> Result<Json<Value>, AppError> {
    require_admin(&me)?;
    let name = b.name.trim().to_lowercase();
    check_username(&name)?;
    check_password_rules(&b.password)?;
    let (phc, nt) = hash_blocking(b.password).await?;
    let (data_dir, config_dir) = (state.data_dir.clone(), state.config_dir.clone());
    state
        .db
        .call(move |c| {
            insert_user(c, &name, b.admin, &phc, &nt)?;
            ensure_home(&data_dir, &name)?;
            write_smbpasswd(c, &config_dir)?;
            tracing::info!("Benutzer '{name}' angelegt von '{}'", me.name);
            Ok(())
        })
        .await?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
pub struct UpdateBody {
    password: Option<String>,
    admin: Option<bool>,
    /// Speicherlimit für „Privat“ in Bytes, 0 = unbegrenzt
    quota_bytes: Option<i64>,
}

pub async fn update(
    State(state): State<AppState>,
    Extension(me): Extension<CurrentUser>,
    UrlPath(name): UrlPath<String>,
    Json(b): Json<UpdateBody>,
) -> Result<Json<Value>, AppError> {
    require_admin(&me)?;
    if b.admin == Some(false) && name == me.name {
        return Err(AppError::BadRequest("Du kannst dir die Admin-Rechte nicht selbst entziehen".into()));
    }
    let hashes = match b.password {
        Some(pw) => {
            check_password_rules(&pw)?;
            Some(hash_blocking(pw).await?)
        }
        None => None,
    };
    let config_dir = state.config_dir.clone();
    state
        .db
        .call(move |c| {
            let user = find(c, &name)?.ok_or(AppError::NotFound)?;
            if let Some(admin) = b.admin {
                c.execute("UPDATE users SET admin = ? WHERE id = ?", params![admin, user.id])?;
            }
            if let Some(q) = b.quota_bytes {
                let q = (q > 0).then_some(q);
                c.execute("UPDATE users SET quota_bytes = ? WHERE id = ?", params![q, user.id])?;
            }
            if let Some((phc, nt)) = hashes {
                c.execute(
                    "UPDATE users SET password_hash = ?, nt_hash = ? WHERE id = ?",
                    params![phc, nt, user.id],
                )?;
                // Benutzer auf allen Geräten abmelden (außer man ändert sich selbst)
                let keep = if user.id == me.id { Some(me.token.as_str()) } else { None };
                auth::delete_user_sessions(c, user.id, keep)?;
                write_smbpasswd(c, &config_dir)?;
            }
            Ok(())
        })
        .await?;
    Ok(Json(json!({ "ok": true })))
}

pub async fn delete(
    State(state): State<AppState>,
    Extension(me): Extension<CurrentUser>,
    UrlPath(name): UrlPath<String>,
) -> Result<Json<Value>, AppError> {
    require_admin(&me)?;
    if name == me.name {
        return Err(AppError::BadRequest("Du kannst dich nicht selbst löschen".into()));
    }
    // Seinen privaten Papierkorb endgültig leeren, sonst bleiben die
    // Dateien unsichtbar liegen
    let n = name.clone();
    let trash_ids: Vec<i64> = state
        .db
        .call(move |c| {
            Ok(c.prepare("SELECT t.id FROM trash t JOIN users u ON u.id = t.owner_id WHERE u.name = ?")?
                .query_map([&n], |r| r.get(0))?
                .collect::<Result<_, _>>()?)
        })
        .await?;
    for id in trash_ids {
        crate::trash::purge_id(&state, id).await?;
    }
    let config_dir = state.config_dir.clone();
    state
        .db
        .call(move |c| {
            // Sessions, Favoriten und Alben werden per ON DELETE CASCADE entfernt.
            // Private Dateien bleiben absichtlich erhalten (Privat/<name>).
            if c.execute("DELETE FROM users WHERE name = ?", [&name])? == 0 {
                return Err(AppError::NotFound);
            }
            write_smbpasswd(c, &config_dir)?;
            tracing::info!("Benutzer '{name}' gelöscht von '{}'", me.name);
            Ok(())
        })
        .await?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
pub struct PasswordBody {
    current: String,
    new: String,
}

/// Eigenes Passwort ändern (für alle Benutzer).
pub async fn change_own_password(
    State(state): State<AppState>,
    Extension(me): Extension<CurrentUser>,
    Json(b): Json<PasswordBody>,
) -> Result<Json<Value>, AppError> {
    if verify(&state.db, &me.name, &b.current).await?.is_none() {
        return Err(AppError::BadRequest("Aktuelles Passwort ist falsch".into()));
    }
    check_password_rules(&b.new)?;
    let (phc, nt) = hash_blocking(b.new).await?;
    let config_dir: PathBuf = state.config_dir.to_path_buf();
    state
        .db
        .call(move |c| {
            c.execute(
                "UPDATE users SET password_hash = ?, nt_hash = ? WHERE id = ?",
                params![phc, nt, me.id],
            )?;
            // andere Geräte abmelden, aktuelle Sitzung behalten
            auth::delete_user_sessions(c, me.id, Some(&me.token))?;
            write_smbpasswd(c, &config_dir)
        })
        .await?;
    Ok(Json(json!({ "ok": true })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nt_hash_wie_samba() {
        // bekannter NT-Hash von "password"
        let (_, nt) = hash_password("password").unwrap();
        assert_eq!(nt, "8846F7EAEE8FB117AD06BDD830B7586C");
    }

    #[test]
    fn benutzernamen() {
        assert!(valid_username("anna"));
        assert!(valid_username("oma-2"));
        assert!(!valid_username("Anna"));
        assert!(!valid_username("2abc"));
        assert!(!valid_username("a/b"));
        assert!(!valid_username(".."));
        assert!(!valid_username("root"));
        assert!(!valid_username(""));
    }

    #[test]
    fn start_ohne_und_mit_admin() {
        let d = tempfile::tempdir().unwrap();
        let (cfg, data) = (d.path().join("cfg"), d.path().join("data"));
        std::fs::create_dir_all(&cfg).unwrap();
        let db = Db::open(&cfg.join("t.db")).unwrap();
        // ohne Bootstrap: wartet auf Einrichtung
        startup(&db, &data, &cfg, None).unwrap();
        assert_eq!(db.blocking(|c| user_count(c)).unwrap(), 0);
        // mit Bootstrap
        startup(&db, &data, &cfg, Some(("admin".into(), "geheim".into()))).unwrap();
        assert!(db.blocking(|c| find(c, "admin")).unwrap().unwrap().admin);
        assert!(data.join("Privat/admin").is_dir());
        let smb = std::fs::read_to_string(cfg.join("smbpasswd")).unwrap();
        assert!(smb.starts_with("admin:20000:"));
    }

    #[test]
    fn uebernahme_aus_users_json() {
        let d = tempfile::tempdir().unwrap();
        let (cfg, data) = (d.path().join("cfg"), d.path().join("data"));
        std::fs::create_dir_all(&cfg).unwrap();
        let (phc, nt) = hash_password("alt").unwrap();
        std::fs::write(
            cfg.join("users.json"),
            json!({ "users": [{ "name": "anna", "admin": true, "uid": 20000, "password_hash": phc, "nt_hash": nt }] })
                .to_string(),
        )
        .unwrap();
        let db = Db::open(&cfg.join("t.db")).unwrap();
        startup(&db, &data, &cfg, None).unwrap();
        assert!(db.blocking(|c| find(c, "anna")).unwrap().is_some());
        assert!(!cfg.join("users.json").exists());
        assert!(cfg.join("users.json.migriert").exists());
    }
}
