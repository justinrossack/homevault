//! Teilen-Links: ein Album oder ein einzelnes Foto/Video für Leute ohne
//! Konto freigeben, optional mit Passwort, Ablaufdatum und Download.
//!
//! Öffentliche Aufrufe gehen über `/api/s/<token>/…`. Jedes Bild wird über
//! seine Index-ID angefragt und nur ausgeliefert, wenn es wirklich zum
//! geteilten Album bzw. Foto gehört. Dateipfade bleiben verborgen.

use std::path::{Path, PathBuf};

use argon2::{
    Argon2, PasswordHash, PasswordHasher, PasswordVerifier,
    password_hash::{SaltString, rand_core::OsRng},
};
use axum::{
    Extension, Json,
    extract::{Path as UrlPath, Query, Request, State},
    http::{HeaderMap, HeaderValue, header},
    response::{IntoResponse, Response},
};
use rand::RngCore;
use rusqlite::{Connection, OptionalExtension, params};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{
    AppState,
    auth::CurrentUser,
    db,
    error::AppError,
    fs_api::needs_preview,
    index, library, media,
    video,
};

struct Share {
    id: i64,
    token: String,
    album_id: Option<i64>,
    media_id: Option<i64>,
    password_hash: Option<String>,
    allow_download: bool,
    expires_at: Option<i64>,
}

fn load_share(conn: &Connection, token: &str) -> rusqlite::Result<Option<Share>> {
    conn.query_row(
        "SELECT id, token, album_id, media_id, password_hash, allow_download, expires_at FROM shares WHERE token = ?",
        [token],
        |r| {
            Ok(Share {
                id: r.get(0)?,
                token: r.get(1)?,
                album_id: r.get(2)?,
                media_id: r.get(3)?,
                password_hash: r.get(4)?,
                allow_download: r.get(5)?,
                expires_at: r.get(6)?,
            })
        },
    )
    .optional()
}

/// Geheimer Schlüssel für Passwort-Cookies, wird beim ersten Bedarf erzeugt.
fn secret(conn: &Connection) -> rusqlite::Result<[u8; 32]> {
    let hex = match db::get_setting(conn, "share_secret")? {
        Some(h) => h,
        None => {
            let mut b = [0u8; 32];
            rand::thread_rng().fill_bytes(&mut b);
            let h: String = b.iter().map(|x| format!("{x:02x}")).collect();
            db::set_setting(conn, "share_secret", &h)?;
            h
        }
    };
    let mut key = [0u8; 32];
    for (i, k) in key.iter_mut().enumerate() {
        *k = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).unwrap_or(0);
    }
    Ok(key)
}

/// Cookie-Wert für eine geöffnete, passwortgeschützte Freigabe. Ändert sich
/// das Passwort, werden alte Cookies automatisch ungültig.
fn unlock_value(key: &[u8; 32], share: &Share) -> String {
    let data = format!("{}|{}", share.token, share.password_hash.as_deref().unwrap_or(""));
    blake3::keyed_hash(key, data.as_bytes()).to_hex().to_string()
}

fn cookie_name(share: &Share) -> String {
    format!("hvs_{}", share.id)
}

fn cookie_value<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .find_map(|c| c.trim().strip_prefix(name)?.strip_prefix('='))
}

enum Access {
    Open(Share),
    NeedsPassword(Share),
}

async fn open(state: &AppState, token: String, headers: &HeaderMap) -> Result<Access, AppError> {
    let (share, key) = state
        .db
        .call(move |c| Ok((load_share(c, &token)?, secret(c)?)))
        .await?;
    let share = share.ok_or(AppError::NotFound)?;
    if share.expires_at.is_some_and(|e| e < db::now()) {
        return Err(AppError::Gone("Dieser Link ist abgelaufen".into()));
    }
    if share.password_hash.is_some() && cookie_value(headers, &cookie_name(&share)) != Some(unlock_value(&key, &share).as_str()) {
        return Ok(Access::NeedsPassword(share));
    }
    Ok(Access::Open(share))
}

async fn open_unlocked(state: &AppState, token: String, headers: &HeaderMap) -> Result<Share, AppError> {
    match open(state, token, headers).await? {
        Access::Open(s) => Ok(s),
        Access::NeedsPassword(_) => Err(AppError::Forbidden),
    }
}

const ITEM_COLS: &str = "m.id, m.path, m.kind, m.size, m.mtime, m.taken_at, m.live_path IS NOT NULL";

fn public_entry(r: &rusqlite::Row) -> rusqlite::Result<Value> {
    let path: String = r.get(1)?;
    Ok(json!({
        "id": r.get::<_, i64>(0)?,
        "name": path.rsplit('/').next().unwrap_or(""),
        "kind": r.get::<_, String>(2)?,
        "size": r.get::<_, i64>(3)?,
        "modified": r.get::<_, i64>(4)?,
        "taken": r.get::<_, i64>(5)?,
        "preview": needs_preview(Path::new(&path)),
        "live": r.get::<_, bool>(6)?,
    }))
}

/// SQL-Bedingung: Datei liegt (noch) dort, wo der Besitzer des Links sie
/// sehen darf – in Gemeinsam oder seinem eigenen Privat-Ordner, nicht im
/// Papierkorb. `sid` ist der SQL-Parameter mit der ID der Freigabe.
fn owner_visible(sid: &str) -> String {
    format!(
        "(substr(m.path, 1, 10) = 'Gemeinsam/' OR EXISTS (SELECT 1 FROM shares s JOIN users o ON o.id = s.owner_id
          WHERE s.id = {sid} AND substr(m.path, 1, length(o.name) + 8) = 'Privat/' || o.name || '/'))"
    )
}

/// Pfad eines Bildes, falls es zu dieser Freigabe gehört.
async fn item_path(state: &AppState, share: &Share, item: i64) -> Result<PathBuf, AppError> {
    let (sid, album, media) = (share.id, share.album_id, share.media_id);
    let path: Option<String> = state
        .db
        .call(move |c| {
            Ok(c.query_row(
                &format!(
                    "SELECT m.path FROM media m WHERE m.id = ?1 AND {}
                     AND (m.id = ?2 OR EXISTS (SELECT 1 FROM album_items i WHERE i.album_id = ?3 AND i.media_id = m.id))",
                    owner_visible("?4")
                ),
                params![item, media, album, sid],
                |r| r.get(0),
            )
            .optional()?)
        })
        .await?;
    Ok(state.data_dir.join(path.ok_or(AppError::NotFound)?))
}

// ---------- Öffentlich ----------

pub async fn info(State(state): State<AppState>, UrlPath(token): UrlPath<String>, headers: HeaderMap) -> Result<Json<Value>, AppError> {
    let server_name = state
        .db
        .call(|c| Ok(db::get_setting(c, "server_name")?.unwrap_or_else(|| crate::users::DEFAULT_SERVER_NAME.into())))
        .await?;
    let share = match open(&state, token, &headers).await? {
        Access::NeedsPassword(_) => return Ok(Json(json!({ "needs_password": true, "server_name": server_name }))),
        Access::Open(s) => s,
    };
    let (sid, album, media) = (share.id, share.album_id, share.media_id);
    let (title, owner, entries) = state
        .db
        .call(move |c| {
            c.execute("UPDATE shares SET views = views + 1 WHERE id = ?", [sid])?;
            let owner: String =
                c.query_row("SELECT u.name FROM shares s JOIN users u ON u.id = s.owner_id WHERE s.id = ?", [sid], |r| r.get(0))?;
            let visible = owner_visible("?2");
            if let Some(a) = album {
                let title: String = c.query_row("SELECT name FROM albums WHERE id = ?", [a], |r| r.get(0))?;
                let entries = c
                    .prepare(&format!(
                        "SELECT {ITEM_COLS} FROM media m JOIN album_items i ON i.media_id = m.id
                         WHERE i.album_id = ?1 AND {visible} ORDER BY m.taken_at DESC, m.id DESC"
                    ))?
                    .query_map([a, sid], public_entry)?
                    .collect::<Result<Vec<_>, _>>()?;
                Ok((title, owner, entries))
            } else {
                let entries = c
                    .prepare(&format!("SELECT {ITEM_COLS} FROM media m WHERE m.id = ?1 AND {visible}"))?
                    .query_map(params![media, sid], public_entry)?
                    .collect::<Result<Vec<_>, _>>()?;
                let title = entries.first().and_then(|e| e["name"].as_str()).unwrap_or("Geteilte Datei").to_owned();
                Ok((title, owner, entries))
            }
        })
        .await?;
    Ok(Json(json!({
        "needs_password": false,
        "server_name": server_name,
        "title": title,
        "owner": owner,
        "kind": if share.album_id.is_some() { "album" } else { "file" },
        "allow_download": share.allow_download,
        "expires_at": share.expires_at,
        "entries": entries,
    })))
}

#[derive(Deserialize)]
pub struct UnlockBody {
    password: String,
}

pub async fn unlock(
    State(state): State<AppState>,
    UrlPath(token): UrlPath<String>,
    headers: HeaderMap,
    Json(b): Json<UnlockBody>,
) -> Result<Response, AppError> {
    let share = match open(&state, token, &headers).await? {
        Access::Open(s) | Access::NeedsPassword(s) => s,
    };
    let hash = share.password_hash.clone().unwrap_or_default();
    let ok = tokio::task::spawn_blocking(move || {
        PasswordHash::new(&hash).is_ok_and(|p| Argon2::default().verify_password(b.password.as_bytes(), &p).is_ok())
    })
    .await
    .unwrap_or(false);
    if !ok {
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        return Err(AppError::BadRequest("Falsches Passwort".into()));
    }
    let key = state.db.call(|c| Ok(secret(c)?)).await?;
    let cookie = format!(
        "{}={}; Path=/; HttpOnly; SameSite=Lax; Max-Age={}",
        cookie_name(&share),
        unlock_value(&key, &share),
        60 * 60 * 24 * 7
    );
    let mut res = Json(json!({ "ok": true })).into_response();
    res.headers_mut().insert(header::SET_COOKIE, HeaderValue::from_str(&cookie).unwrap());
    Ok(res)
}

#[derive(Deserialize)]
pub struct ItemQuery {
    #[serde(default)]
    download: bool,
}

pub async fn item(
    State(state): State<AppState>,
    UrlPath((token, item, what)): UrlPath<(String, i64, String)>,
    Query(q): Query<ItemQuery>,
    headers: HeaderMap,
    req: Request,
) -> Result<Response, AppError> {
    let share = open_unlocked(&state, token, &headers).await?;
    let src = item_path(&state, &share, item).await?;
    match what.as_str() {
        "thumb" => media::thumb_of(&state, src).await,
        "view" if needs_preview(&src) => media::preview_of(&state, &src, req).await,
        "view" => media::send_file(&src, req, false).await,
        "file" => {
            if q.download && !share.allow_download {
                return Err(AppError::Forbidden);
            }
            media::send_file(&src, req, q.download).await
        }
        "live" => media::live_of(&state, &src, req).await,
        "video" => Ok(Json(video::status_of(&state, src).await?).into_response()),
        "web" => video::web_of(&state, &src, req).await,
        _ => Err(AppError::NotFound),
    }
}

// ---------- Für angemeldete Benutzer ----------

#[derive(Deserialize)]
pub struct CreateBody {
    album_id: Option<i64>,
    path: Option<String>,
    password: Option<String>,
    /// 0 oder fehlend = läuft nie ab
    expires_days: Option<i64>,
    #[serde(default = "yes")]
    allow_download: bool,
}

fn yes() -> bool {
    true
}

fn new_token() -> String {
    let mut b = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut b);
    b.iter().map(|x| format!("{x:02x}")).collect()
}

pub async fn create(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Json(b): Json<CreateBody>,
) -> Result<Json<Value>, AppError> {
    let db_path = match (&b.album_id, &b.path) {
        (Some(_), None) => None,
        (None, Some(p)) => Some(library::to_db(&state, &user.name, p)?),
        _ => return Err(AppError::BadRequest("Album oder Datei angeben".into())),
    };
    let password_hash = match b.password.filter(|p| !p.is_empty()) {
        Some(pw) => Some(
            tokio::task::spawn_blocking(move || {
                Argon2::default()
                    .hash_password(pw.as_bytes(), &SaltString::generate(&mut OsRng))
                    .map(|h| h.to_string())
                    .map_err(|e| AppError::Internal(e.to_string()))
            })
            .await
            .map_err(|e| AppError::Internal(e.to_string()))??,
        ),
        None => None,
    };
    let expires_at = b.expires_days.filter(|d| *d > 0).map(|d| db::now() + d.min(3650) * 86400);
    let data_dir = state.data_dir.clone();
    let token = new_token();
    let t = token.clone();
    state
        .db
        .call(move |c| {
            let media_id = match &db_path {
                Some(p) => Some(index::ensure_media(c, &data_dir, p)?),
                None => None,
            };
            if let Some(a) = b.album_id {
                // nur eigene Alben
                c.query_row("SELECT 1 FROM albums WHERE id = ? AND owner_id = ?", params![a, user.id], |_| Ok(()))
                    .optional()?
                    .ok_or(AppError::NotFound)?;
            }
            c.execute(
                "INSERT INTO shares (token, owner_id, album_id, media_id, password_hash, allow_download, expires_at, created_at)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
                params![t, user.id, b.album_id, media_id, password_hash, b.allow_download, expires_at, db::now()],
            )?;
            Ok(())
        })
        .await?;
    Ok(Json(json!({ "token": token, "url": format!("/s/{token}") })))
}

#[derive(Deserialize)]
pub struct ListQuery {
    album_id: Option<i64>,
    path: Option<String>,
}

/// Eigene Links, optional gefiltert auf ein Album oder eine Datei.
pub async fn list(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Query(q): Query<ListQuery>,
) -> Result<Json<Value>, AppError> {
    let media_path = q.path.as_deref().map(|p| library::to_db(&state, &user.name, p)).transpose()?;
    let shares = state
        .db
        .call(move |c| {
            let mut stmt = c.prepare(
                "SELECT s.id, s.token, s.album_id, a.name, m.path, s.password_hash IS NOT NULL, s.allow_download,
                        s.expires_at, s.views, s.created_at
                 FROM shares s LEFT JOIN albums a ON a.id = s.album_id LEFT JOIN media m ON m.id = s.media_id
                 WHERE s.owner_id = ?1 AND (?2 IS NULL OR s.album_id = ?2) AND (?3 IS NULL OR m.path = ?3)
                 ORDER BY s.created_at DESC",
            )?;
            let rows = stmt
                .query_map(params![user.id, q.album_id, media_path], |r| {
                    let token: String = r.get(1)?;
                    let album: Option<String> = r.get(3)?;
                    let path: Option<String> = r.get(4)?;
                    let expires: Option<i64> = r.get(7)?;
                    Ok(json!({
                        "id": r.get::<_, i64>(0)?,
                        "url": format!("/s/{token}"),
                        "kind": if r.get::<_, Option<i64>>(2)?.is_some() { "album" } else { "file" },
                        "title": album.or_else(|| path.as_deref().and_then(|p| p.rsplit('/').next()).map(str::to_owned)),
                        "path": path.and_then(|p| library::to_virtual(&user.name, &p)),
                        "has_password": r.get::<_, bool>(5)?,
                        "allow_download": r.get::<_, bool>(6)?,
                        "expires_at": expires,
                        "expired": expires.is_some_and(|e| e < db::now()),
                        "views": r.get::<_, i64>(8)?,
                        "created_at": r.get::<_, i64>(9)?,
                    }))
                })?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
        .await?;
    Ok(Json(json!({ "shares": shares })))
}

pub async fn delete(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    UrlPath(id): UrlPath<i64>,
) -> Result<Json<Value>, AppError> {
    state
        .db
        .call(move |c| {
            if c.execute("DELETE FROM shares WHERE id = ? AND owner_id = ?", params![id, user.id])? == 0 {
                return Err(AppError::NotFound);
            }
            Ok(())
        })
        .await?;
    Ok(Json(json!({ "ok": true })))
}
