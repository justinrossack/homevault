//! Zeitleiste, Favoriten und Alben auf Basis des Medien-Index.
//!
//! In der Datenbank stehen Pfade relativ zu DATA_DIR
//! ("Privat/anna/x.jpg"), der Benutzer sieht virtuelle Pfade ("Privat/x.jpg").
//! Jede Abfrage filtert auf das, was der Benutzer sehen darf.

use std::path::Path;

use axum::{
    Extension, Json,
    extract::{Path as UrlPath, Query, State},
};
use rusqlite::{Connection, OptionalExtension, Row, params};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{AppState, auth::CurrentUser, db, error::AppError, fs_api::needs_preview, index, paths};

const TIMELINE_PAGE: i64 = 300;
const MAX_LIST: i64 = 10_000;

/// Sichtbar: alles in Gemeinsam und der eigene private Ordner.
/// ?1 = "Privat/<name>/", ?2 = "Privat/<name>0" (Bereichsabfrage nutzt den Index)
const VISIBLE: &str = "((m.path >= 'Gemeinsam/' AND m.path < 'Gemeinsam0') OR (m.path >= ?1 AND m.path < ?2))";

const MEDIA_COLS: &str = "m.id, m.path, m.kind, m.size, m.mtime, m.taken_at, m.live_path, \
                          EXISTS (SELECT 1 FROM favorites f WHERE f.media_id = m.id AND f.user_id = ?3)";

fn private_range(user: &str) -> (String, String) {
    (format!("{}/{user}/", paths::PRIVATE), format!("{}/{user}0", paths::PRIVATE))
}

/// DB-Pfad -> Pfad aus Sicht des Benutzers
pub fn to_virtual(user: &str, db_path: &str) -> Option<String> {
    if db_path.starts_with(&format!("{}/", paths::SHARED)) {
        return Some(db_path.to_owned());
    }
    let rest = db_path.strip_prefix(&format!("{}/{user}/", paths::PRIVATE))?;
    Some(format!("{}/{rest}", paths::PRIVATE))
}

/// Pfad aus Sicht des Benutzers -> DB-Pfad (mit Rechteprüfung)
pub fn to_db(state: &AppState, user: &str, virtual_path: &str) -> Result<String, AppError> {
    let r = paths::resolve_in_space(&state.data_dir, user, virtual_path)?;
    if r.is_root() {
        return Err(AppError::BadRequest("Ungültiger Pfad".into()));
    }
    Ok(paths::relative(&state.data_dir, &r.full))
}

/// Eine Zeile (MEDIA_COLS) als Eintrag wie in der Ordneransicht.
fn entry(user: &str, r: &Row) -> rusqlite::Result<Option<Value>> {
    let path: String = r.get(1)?;
    let Some(vpath) = to_virtual(user, &path) else { return Ok(None) };
    let live: Option<String> = r.get(6)?;
    let mut e = json!({
        "id": r.get::<_, i64>(0)?,
        "name": path.rsplit('/').next().unwrap_or(&path),
        "path": vpath,
        "kind": r.get::<_, String>(2)?,
        "size": r.get::<_, i64>(3)?,
        "modified": r.get::<_, i64>(4)?,
        "taken": r.get::<_, i64>(5)?,
        "favorite": r.get::<_, bool>(7)?,
    });
    if needs_preview(Path::new(&path)) {
        e["preview"] = json!(true);
    }
    if let Some(l) = live.and_then(|l| to_virtual(user, &l)) {
        e["live"] = json!(l);
    }
    Ok(Some(e))
}

fn query_entries(conn: &Connection, user: &CurrentUser, sql: &str, extra: &[&dyn rusqlite::ToSql]) -> Result<Vec<Value>, AppError> {
    let (lo, hi) = private_range(&user.name);
    let mut args: Vec<&dyn rusqlite::ToSql> = vec![&lo, &hi, &user.id];
    args.extend_from_slice(extra);
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map(args.as_slice(), |r| entry(&user.name, r))?;
    let mut out = Vec::new();
    for r in rows {
        if let Some(e) = r? {
            out.push(e);
        }
    }
    Ok(out)
}

// ---------- Zeitleiste ----------

#[derive(Deserialize)]
pub struct TimelineQuery {
    /// "<taken>_<id>" des letzten Eintrags der vorigen Seite
    cursor: Option<String>,
}

pub async fn timeline(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Query(q): Query<TimelineQuery>,
) -> Result<Json<Value>, AppError> {
    let (before_taken, before_id) = q
        .cursor
        .as_deref()
        .and_then(|c| c.split_once('_'))
        .and_then(|(t, i)| Some((t.parse::<i64>().ok()?, i.parse::<i64>().ok()?)))
        .unwrap_or((i64::MAX, i64::MAX));
    let indexing = state.index.status().scanning;
    let entries = state
        .db
        .call(move |c| {
            let sql = format!(
                "SELECT {MEDIA_COLS} FROM media m
                 WHERE {VISIBLE} AND (m.taken_at < ?4 OR (m.taken_at = ?4 AND m.id < ?5))
                 ORDER BY m.taken_at DESC, m.id DESC LIMIT ?6"
            );
            query_entries(c, &user, &sql, &[&before_taken, &before_id, &TIMELINE_PAGE])
        })
        .await?;
    let next = (entries.len() as i64 == TIMELINE_PAGE)
        .then(|| entries.last())
        .flatten()
        .map(|e| format!("{}_{}", e["taken"], e["id"]));
    Ok(Json(json!({ "entries": entries, "next": next, "indexing": indexing })))
}

// ---------- Favoriten ----------

pub async fn favorites(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
) -> Result<Json<Value>, AppError> {
    let entries = state
        .db
        .call(move |c| {
            let sql = format!(
                "SELECT {MEDIA_COLS} FROM media m JOIN favorites fav ON fav.media_id = m.id AND fav.user_id = ?3
                 WHERE {VISIBLE} ORDER BY m.taken_at DESC, m.id DESC LIMIT ?4"
            );
            query_entries(c, &user, &sql, &[&MAX_LIST])
        })
        .await?;
    Ok(Json(json!({ "entries": entries })))
}

#[derive(Deserialize)]
pub struct FavoriteBody {
    paths: Vec<String>,
    favorite: bool,
}

pub async fn set_favorites(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Json(b): Json<FavoriteBody>,
) -> Result<Json<Value>, AppError> {
    let db_paths = b
        .paths
        .iter()
        .map(|p| to_db(&state, &user.name, p))
        .collect::<Result<Vec<_>, _>>()?;
    let data_dir = state.data_dir.clone();
    state
        .db
        .call(move |c| {
            let tx = c.transaction()?;
            for p in &db_paths {
                let id = index::ensure_media(&tx, &data_dir, p)?;
                if b.favorite {
                    tx.execute(
                        "INSERT OR IGNORE INTO favorites (user_id, media_id, created_at) VALUES (?, ?, ?)",
                        params![user.id, id, db::now()],
                    )?;
                } else {
                    tx.execute("DELETE FROM favorites WHERE user_id = ? AND media_id = ?", params![user.id, id])?;
                }
            }
            tx.commit()?;
            Ok(())
        })
        .await?;
    Ok(Json(json!({ "ok": true })))
}

/// Favoriten-Markierungen für einen Ordner (für die Dateiansicht).
pub fn favorites_in_dir(conn: &Connection, user_id: i64, dir_db_path: &str) -> rusqlite::Result<Vec<String>> {
    let prefix = format!("{dir_db_path}/");
    let mut stmt = conn.prepare(
        "SELECT m.path FROM favorites f JOIN media m ON m.id = f.media_id
         WHERE f.user_id = ?1 AND m.path >= ?2 AND m.path < ?3",
    )?;
    let rows = stmt.query_map(params![user_id, prefix, format!("{dir_db_path}0")], |r| r.get(0))?;
    rows.collect()
}

// ---------- Alben ----------

fn own_album(conn: &Connection, user: &CurrentUser, id: i64) -> Result<String, AppError> {
    conn.query_row(
        "SELECT name FROM albums WHERE id = ? AND owner_id = ?",
        params![id, user.id],
        |r| r.get(0),
    )
    .optional()?
    .ok_or(AppError::NotFound)
}

fn check_album_name(name: &str) -> Result<String, AppError> {
    let n = name.trim();
    if n.is_empty() || n.chars().count() > 100 {
        return Err(AppError::BadRequest("Albumname: 1 bis 100 Zeichen".into()));
    }
    Ok(n.to_owned())
}

pub async fn albums(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
) -> Result<Json<Value>, AppError> {
    let albums = state
        .db
        .call(move |c| {
            let ids: Vec<(i64, String, i64)> = c
                .prepare("SELECT id, name, created_at FROM albums WHERE owner_id = ? ORDER BY created_at DESC")?
                .query_map([user.id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
                .collect::<Result<_, _>>()?;
            let mut out = Vec::new();
            for (id, name, created) in ids {
                let sql = format!(
                    "SELECT {MEDIA_COLS} FROM media m JOIN album_items i ON i.media_id = m.id
                     WHERE i.album_id = ?4 AND {VISIBLE} ORDER BY m.taken_at DESC LIMIT 1"
                );
                let cover = query_entries(c, &user, &sql, &[&id])?.into_iter().next();
                let count: i64 =
                    c.query_row("SELECT count(*) FROM album_items WHERE album_id = ?", [id], |r| r.get(0))?;
                out.push(json!({ "id": id, "name": name, "created_at": created, "count": count, "cover": cover }));
            }
            Ok(out)
        })
        .await?;
    Ok(Json(json!({ "albums": albums })))
}

#[derive(Deserialize)]
pub struct AlbumBody {
    name: String,
}

pub async fn create_album(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Json(b): Json<AlbumBody>,
) -> Result<Json<Value>, AppError> {
    let name = check_album_name(&b.name)?;
    let id = state
        .db
        .call(move |c| {
            c.execute(
                "INSERT INTO albums (owner_id, name, created_at) VALUES (?, ?, ?)",
                params![user.id, name, db::now()],
            )?;
            Ok(c.last_insert_rowid())
        })
        .await?;
    Ok(Json(json!({ "id": id })))
}

pub async fn album(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    UrlPath(id): UrlPath<i64>,
) -> Result<Json<Value>, AppError> {
    let (name, entries) = state
        .db
        .call(move |c| {
            let name = own_album(c, &user, id)?;
            let sql = format!(
                "SELECT {MEDIA_COLS} FROM media m JOIN album_items i ON i.media_id = m.id
                 WHERE i.album_id = ?4 AND {VISIBLE} ORDER BY m.taken_at DESC, m.id DESC"
            );
            Ok((name, query_entries(c, &user, &sql, &[&id])?))
        })
        .await?;
    Ok(Json(json!({ "id": id, "name": name, "entries": entries })))
}

pub async fn rename_album(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    UrlPath(id): UrlPath<i64>,
    Json(b): Json<AlbumBody>,
) -> Result<Json<Value>, AppError> {
    let name = check_album_name(&b.name)?;
    state
        .db
        .call(move |c| {
            own_album(c, &user, id)?;
            c.execute("UPDATE albums SET name = ? WHERE id = ?", params![name, id])?;
            Ok(())
        })
        .await?;
    Ok(Json(json!({ "ok": true })))
}

pub async fn delete_album(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    UrlPath(id): UrlPath<i64>,
) -> Result<Json<Value>, AppError> {
    state
        .db
        .call(move |c| {
            own_album(c, &user, id)?;
            // nur das Album, die Fotos bleiben
            c.execute("DELETE FROM albums WHERE id = ?", [id])?;
            Ok(())
        })
        .await?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
pub struct ItemsBody {
    paths: Vec<String>,
}

pub async fn add_items(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    UrlPath(id): UrlPath<i64>,
    Json(b): Json<ItemsBody>,
) -> Result<Json<Value>, AppError> {
    let db_paths = b
        .paths
        .iter()
        .map(|p| to_db(&state, &user.name, p))
        .collect::<Result<Vec<_>, _>>()?;
    let data_dir = state.data_dir.clone();
    let added = state
        .db
        .call(move |c| {
            let tx = c.transaction()?;
            own_album(&tx, &user, id)?;
            let mut added = 0;
            for p in &db_paths {
                let media = index::ensure_media(&tx, &data_dir, p)?;
                added += tx.execute(
                    "INSERT OR IGNORE INTO album_items (album_id, media_id, added_at) VALUES (?, ?, ?)",
                    params![id, media, db::now()],
                )?;
            }
            tx.commit()?;
            Ok(added)
        })
        .await?;
    Ok(Json(json!({ "added": added })))
}

pub async fn remove_items(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    UrlPath(id): UrlPath<i64>,
    Json(b): Json<ItemsBody>,
) -> Result<Json<Value>, AppError> {
    let db_paths = b
        .paths
        .iter()
        .map(|p| to_db(&state, &user.name, p))
        .collect::<Result<Vec<_>, _>>()?;
    state
        .db
        .call(move |c| {
            own_album(c, &user, id)?;
            for p in &db_paths {
                c.execute(
                    "DELETE FROM album_items WHERE album_id = ?
                     AND media_id = (SELECT id FROM media WHERE path = ?)",
                    params![id, p],
                )?;
            }
            Ok(())
        })
        .await?;
    Ok(Json(json!({ "ok": true })))
}

// ---------- Index-Status (Admin) ----------

pub async fn index_status(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
) -> Result<Json<Value>, AppError> {
    if !user.admin {
        return Err(AppError::Forbidden);
    }
    // Hauptdatei plus WAL, sonst wirkt die Datenbank fast leer
    let db_size: u64 = ["", "-wal"]
        .iter()
        .map(|suffix| {
            let mut p = state.db.path().as_os_str().to_owned();
            p.push(suffix);
            std::fs::metadata(p).map_or(0, |m| m.len())
        })
        .sum();
    Ok(Json(json!({ "index": state.index.status(), "db_size": db_size, "transcoder": state.transcoder.status() })))
}

pub async fn rescan(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
) -> Result<Json<Value>, AppError> {
    if !user.admin {
        return Err(AppError::Forbidden);
    }
    state.index.request_scan();
    Ok(Json(json!({ "ok": true })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn virtuelle_pfade() {
        assert_eq!(to_virtual("anna", "Gemeinsam/a.jpg").as_deref(), Some("Gemeinsam/a.jpg"));
        assert_eq!(to_virtual("anna", "Privat/anna/x/a.jpg").as_deref(), Some("Privat/x/a.jpg"));
        assert_eq!(to_virtual("anna", "Privat/ben/a.jpg"), None);
        assert_eq!(to_virtual("an", "Privat/anna/a.jpg"), None);
    }
}
