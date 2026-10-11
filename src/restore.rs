//! Wiederherstellen aus einem Backup auf einem anderen NAS.
//!
//! - **Einzelne Dateien/Ordner:** im Backup blättern (aktueller Stand oder
//!   ältere Versionen unter `.versionen/`) und zurückholen, wahlweise an den
//!   Originalort (Vorhandenes wird nicht überschrieben) oder in einen neuen
//!   Ordner „Wiederhergestellt …“.
//! - **Komplett (neuer Server):** Im Einrichtungsassistenten werden Datenbank
//!   (Benutzer, Alben, Favoriten, Einstellungen) und alle Dateien geladen.
//!   Danach startet homevault neu und übernimmt die Datenbank.

use std::path::Path;

use axum::{
    Extension, Json,
    extract::{Path as UrlPath, Query, State},
};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{
    AppState,
    auth::CurrentUser,
    backup::{self, DB_DIR, Progress, RunError, Target, TargetBody, VERSIONS_DIR},
    db,
    error::AppError,
    paths, users,
};

/// Name der übernommenen Datenbank; wird beim nächsten Start eingesetzt.
pub const PENDING_DB: &str = "homevault.db.restore";

fn require_admin(u: &CurrentUser) -> Result<(), AppError> {
    if u.admin { Ok(()) } else { Err(AppError::Forbidden) }
}

/// Pfad im Backup prüfen: nur Gemeinsam/… oder Privat/<name>/…, kein "..".
fn clean_path(p: &str) -> Result<String, AppError> {
    let p = p.trim().trim_matches('/');
    let bad = p.split('/').any(|s| s.is_empty() || s == "." || s == ".." || s.contains('\\'));
    if !p.is_empty() && bad {
        return Err(AppError::BadRequest("Ungültiger Pfad".into()));
    }
    Ok(p.to_owned())
}

/// Versionsordner heißen nach dem Zeitpunkt, z. B. 2026-10-11_030000
fn clean_version(v: &str) -> Result<String, AppError> {
    if v.chars().all(|c| c.is_ascii_digit() || c == '-' || c == '_') {
        Ok(v.to_owned())
    } else {
        Err(AppError::BadRequest("Ungültige Version".into()))
    }
}

fn remote_for(t: &Target, version: &str, path: &str) -> String {
    let sub = match (version.is_empty(), path.is_empty()) {
        (true, _) => path.to_owned(),
        (false, true) => format!("{VERSIONS_DIR}/{version}"),
        (false, false) => format!("{VERSIONS_DIR}/{version}/{path}"),
    };
    t.remote(&sub)
}

async fn target(state: &AppState, id: i64) -> Result<Target, AppError> {
    state.db.call(move |c| backup::load_target(c, id)).await
}

/// Aktueller Stand und alle älteren Versionen, die auf dem Ziel liegen.
pub async fn versions(
    State(state): State<AppState>,
    Extension(me): Extension<CurrentUser>,
    UrlPath(id): UrlPath<i64>,
) -> Result<Json<Value>, AppError> {
    require_admin(&me)?;
    let t = target(&state, id).await?;
    let env = backup::rclone_env(&t).await.map_err(AppError::Internal)?;
    let listing = backup::rclone_output(&["lsf", "--dirs-only", &t.remote(VERSIONS_DIR)], &env).await.unwrap_or_default();
    let mut stamps: Vec<String> = listing.lines().map(|l| l.trim_end_matches('/').to_owned()).filter(|l| clean_version(l).is_ok()).collect();
    stamps.sort_unstable_by(|a, b| b.cmp(a));
    let mut out = vec![json!({ "id": "", "label": "Aktueller Stand" })];
    for s in stamps {
        // "2026-10-11_030000" -> "11.10.2026 03:00"
        let label = (s.len() == 17).then(|| format!("{}.{}.{} {}:{}", &s[8..10], &s[5..7], &s[0..4], &s[11..13], &s[13..15]));
        out.push(json!({ "id": s, "label": format!("Gelöscht/geändert am {}", label.unwrap_or_else(|| s.clone())) }));
    }
    Ok(Json(json!({ "versions": out })))
}

#[derive(Deserialize)]
pub struct BrowseQuery {
    #[serde(default)]
    version: String,
    #[serde(default)]
    path: String,
}

/// Inhalt eines Ordners im Backup.
pub async fn browse(
    State(state): State<AppState>,
    Extension(me): Extension<CurrentUser>,
    UrlPath(id): UrlPath<i64>,
    Query(q): Query<BrowseQuery>,
) -> Result<Json<Value>, AppError> {
    require_admin(&me)?;
    let (version, path) = (clean_version(&q.version)?, clean_path(&q.path)?);
    let t = target(&state, id).await?;
    let env = backup::rclone_env(&t).await.map_err(AppError::Internal)?;
    let raw = backup::rclone_output(&["lsjson", "--max-depth", "1", &remote_for(&t, &version, &path)], &env)
        .await
        .map_err(AppError::BadRequest)?;
    let items: Vec<Value> = serde_json::from_str(&raw).unwrap_or_default();
    let mut entries: Vec<Value> = items
        .into_iter()
        .filter(|i| {
            let name = i["Name"].as_str().unwrap_or_default();
            // oberste Ebene: nur die Datenbereiche zeigen
            !name.starts_with('.') && (!path.is_empty() || name == paths::SHARED || name == paths::PRIVATE)
        })
        .map(|i| {
            let name = i["Name"].as_str().unwrap_or_default().to_owned();
            json!({
                "name": name,
                "path": if path.is_empty() { name.clone() } else { format!("{path}/{name}") },
                "dir": i["IsDir"],
                "size": i["Size"].as_i64().filter(|s| *s >= 0),
                "modified": i["ModTime"],
            })
        })
        .collect();
    entries.sort_by(|a, b| {
        (b["dir"] == true)
            .cmp(&(a["dir"] == true))
            .then_with(|| a["name"].as_str().unwrap_or_default().to_lowercase().cmp(&b["name"].as_str().unwrap_or_default().to_lowercase()))
    });
    Ok(Json(json!({ "entries": entries })))
}

#[derive(Deserialize)]
pub struct RestoreBody {
    #[serde(default)]
    version: String,
    paths: Vec<String>,
    /// "original" = an den Originalort (nichts überschreiben), "folder" = in neuen Ordner
    #[serde(default)]
    mode: String,
}

/// Wohin wird ein Pfad aus dem Backup zurückgelegt?
fn destination(data_dir: &Path, path: &str, into_folder: Option<&str>) -> Result<String, AppError> {
    let parts: Vec<&str> = path.split('/').collect();
    // Bereich: "Gemeinsam" bzw. "Privat/<name>"
    let space_len = match parts.first() {
        Some(&s) if s == paths::SHARED => 1,
        Some(&s) if s == paths::PRIVATE && parts.len() >= 2 => 2,
        _ => return Err(AppError::BadRequest("Nur Dateien aus „Gemeinsam“ oder „Privat“ lassen sich wiederherstellen".into())),
    };
    if parts.len() <= space_len {
        // ganzer Bereich
        return Ok(match into_folder {
            Some(f) => data_dir.join(parts.join("/")).join(f),
            None => data_dir.join(parts.join("/")),
        }
        .to_string_lossy()
        .into_owned());
    }
    let space = parts[..space_len].join("/");
    let rest = parts[space_len..].join("/");
    Ok(match into_folder {
        Some(f) => data_dir.join(space).join(f).join(rest),
        None => data_dir.join(space).join(rest),
    }
    .to_string_lossy()
    .into_owned())
}

pub async fn restore(
    State(state): State<AppState>,
    Extension(me): Extension<CurrentUser>,
    UrlPath(id): UrlPath<i64>,
    Json(b): Json<RestoreBody>,
) -> Result<Json<Value>, AppError> {
    require_admin(&me)?;
    let version = clean_version(&b.version)?;
    let paths: Vec<String> = b.paths.iter().map(|p| clean_path(p)).collect::<Result<_, _>>()?;
    if paths.is_empty() || paths.iter().any(String::is_empty) {
        return Err(AppError::BadRequest("Bitte Dateien oder Ordner auswählen".into()));
    }
    let folder = (b.mode == "folder").then(|| format!("Wiederhergestellt {}", chrono::Local::now().format("%Y-%m-%d %H-%M")));
    // Ziele vorab prüfen, damit Fehler sofort sichtbar sind
    let jobs: Vec<(String, String)> = paths
        .iter()
        .map(|p| Ok((p.clone(), destination(&state.data_dir, p, folder.as_deref())?)))
        .collect::<Result<_, AppError>>()?;
    let t = target(&state, id).await?;
    let backups = state.backups.clone();
    let guard = backups.busy.clone().try_lock_owned().map_err(|_| AppError::Conflict("Es läuft bereits ein Backup oder eine Wiederherstellung".into()))?;
    let index = state.index.clone();
    let keep_existing = folder.is_none();
    tokio::spawn(async move {
        let _guard = guard;
        let result = run_restore(&backups, &t, id, &version, &jobs, keep_existing).await;
        index.request_scan();
        finish(&backups, result, folder);
    });
    tracing::info!("Wiederherstellung aus Backup {id} gestartet von '{}'", me.name);
    Ok(Json(json!({ "ok": true })))
}

fn finish(backups: &backup::Backups, result: Result<(i64, i64), String>, folder: Option<String>) {
    *backups.progress.lock().unwrap() = None;
    let value = match result {
        Ok((files, bytes)) => json!({ "ok": true, "files": files, "bytes": bytes, "folder": folder, "finished_at": db::now() }),
        Err(e) => json!({ "ok": false, "message": e, "finished_at": db::now() }),
    };
    *backups.restore_result.lock().unwrap() = Some(value);
}

async fn run_restore(
    backups: &backup::Backups,
    t: &Target,
    target_id: i64,
    version: &str,
    jobs: &[(String, String)],
    keep_existing: bool,
) -> Result<(i64, i64), String> {
    *backups.restore_result.lock().unwrap() = None;
    *backups.progress.lock().unwrap() = Some(Progress::start(target_id));
    let env = backup::rclone_env(t).await?;
    let (mut files, mut bytes) = (0, 0);
    for (path, dest) in jobs {
        backups.set_part(&format!("Wiederherstellen: {path}"));
        let src = remote_for(t, version, path);
        // Datei oder Ordner? Bei einer Datei wird in den Elternordner kopiert.
        let stat = backup::rclone_output(&["lsjson", "--stat", &src], &env).await?;
        let is_dir = serde_json::from_str::<Value>(&stat).map(|v| v["IsDir"] == true).unwrap_or(false);
        let dest = if is_dir { dest.clone() } else { Path::new(dest).parent().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default() };
        let mut args: Vec<String> = vec!["copy".into(), src, dest];
        if keep_existing {
            args.push("--ignore-existing".into());
        }
        args.extend(["--use-json-log", "--stats", "2s", "--stats-log-level", "NOTICE"].map(String::from));
        let (f, b) = backups.rclone_tracked(&args, &env).await.map_err(|e| match e {
            RunError::Cancelled => "Abgebrochen".to_owned(),
            RunError::Failed(m) => m,
        })?;
        files += f;
        bytes += b;
    }
    Ok((files, bytes))
}

// ---------- Komplett-Wiederherstellung im Einrichtungsassistenten ----------

/// Startet die Wiederherstellung auf einem neuen, leeren Server.
/// Nur möglich, solange noch kein Benutzer existiert (wie die Einrichtung).
pub async fn setup_restore(State(state): State<AppState>, Json(mut b): Json<TargetBody>) -> Result<Json<Value>, AppError> {
    if state.db.call(|c| Ok(users::user_count(c)?)).await? > 0 {
        return Err(AppError::Forbidden);
    }
    b.validate()?;
    let password = b.password.clone().unwrap_or_default();
    let t = b.into_target(password);
    let env = backup::rclone_env(&t).await.map_err(AppError::Internal)?;
    let once = ["--retries", "1", "--low-level-retries", "1"];
    let db_remote = t.remote(&format!("{DB_DIR}/homevault.db"));
    backup::rclone_output(&[&["lsjson", "--stat", db_remote.as_str()][..], &once].concat(), &env)
        .await
        .map_err(|e| AppError::BadRequest(if e.contains("not found") || e.contains("directory not found") {
            format!("Unter {} wurde kein homevault-Backup gefunden", t.display())
        } else {
            e
        }))?;

    let backups = state.backups.clone();
    let guard = backups.busy.clone().try_lock_owned().map_err(|_| AppError::Conflict("Wiederherstellung läuft bereits".into()))?;
    let (data_dir, config_dir) = (state.data_dir.clone(), state.config_dir.clone());
    tokio::spawn(async move {
        let _guard = guard;
        *backups.restore_result.lock().unwrap() = None;
        *backups.progress.lock().unwrap() = Some(Progress::start(0));
        let result = async {
            let tmp = config_dir.join("restore-db");
            let _ = tokio::fs::remove_dir_all(&tmp).await;
            backups.set_part("Datenbank (Benutzer, Alben, Einstellungen)");
            let args = |src: String, dest: String| -> Vec<String> {
                vec![
                    "copy".into(), src, dest, "--use-json-log".into(), "--stats".into(), "2s".into(),
                    "--stats-log-level".into(), "NOTICE".into(),
                ]
            };
            let run = |a: Vec<String>| {
                let b = backups.clone();
                let e = env.clone();
                async move {
                    b.rclone_tracked(&a, &e).await.map_err(|e| match e {
                        RunError::Cancelled => "Abgebrochen".to_owned(),
                        RunError::Failed(m) => m,
                    })
                }
            };
            run(args(t.remote(DB_DIR), tmp.to_string_lossy().into_owned())).await?;
            let (mut files, mut bytes) = (0, 0);
            for space in [paths::SHARED, paths::PRIVATE] {
                // Bereich fehlt im Backup (z. B. war er leer): überspringen
                if backup::rclone_output(&["lsjson", "--stat", &t.remote(space)], &env).await.is_err() {
                    continue;
                }
                backups.set_part(space);
                let (f, b) = run(args(t.remote(space), data_dir.join(space).to_string_lossy().into_owned())).await?;
                files += f;
                bytes += b;
            }
            // Datenbank beim Neustart übernehmen (sie ist jetzt gerade geöffnet)
            tokio::fs::rename(tmp.join("homevault.db"), config_dir.join(PENDING_DB)).await.map_err(|e| e.to_string())?;
            let _ = tokio::fs::remove_dir_all(&tmp).await;
            Ok::<_, String>((files, bytes))
        }
        .await;
        let ok = result.is_ok();
        finish(&backups, result, None);
        if ok {
            tracing::info!("Wiederherstellung abgeschlossen, starte neu …");
            tokio::time::sleep(std::time::Duration::from_secs(3)).await;
            std::process::exit(0);
        }
    });
    Ok(Json(json!({ "ok": true })))
}

/// Fortschritt der Komplett-Wiederherstellung (ohne Anmeldung, nur während der Einrichtung).
pub async fn setup_restore_status(State(state): State<AppState>) -> Result<Json<Value>, AppError> {
    let progress = state.backups.progress.lock().unwrap().clone();
    let result = state.backups.restore_result.lock().unwrap().clone();
    Ok(Json(json!({ "running": progress, "result": result })))
}

/// Beim Start: eine wiederhergestellte Datenbank übernehmen.
pub fn apply_pending_db(config_dir: &Path) {
    let pending = config_dir.join(PENDING_DB);
    if !pending.exists() {
        return;
    }
    for f in ["homevault.db", "homevault.db-wal", "homevault.db-shm"] {
        let _ = std::fs::remove_file(config_dir.join(f));
    }
    match std::fs::rename(&pending, config_dir.join("homevault.db")) {
        Ok(()) => tracing::info!("Datenbank aus dem Backup übernommen"),
        Err(e) => tracing::error!("Datenbank aus dem Backup konnte nicht übernommen werden: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ziele() {
        let d = Path::new("/data");
        assert_eq!(destination(d, "Gemeinsam/Urlaub/a.jpg", None).unwrap(), "/data/Gemeinsam/Urlaub/a.jpg");
        assert_eq!(destination(d, "Privat/anna/x/a.jpg", Some("W")).unwrap(), "/data/Privat/anna/W/x/a.jpg");
        assert_eq!(destination(d, "Gemeinsam", Some("W")).unwrap(), "/data/Gemeinsam/W");
        assert!(destination(d, "Datenbank/homevault.db", None).is_err());
        assert!(clean_path("Gemeinsam/../../etc").is_err());
        assert!(clean_version("2026-10-11_030000").is_ok());
        assert!(clean_version("../x").is_err());
    }
}
