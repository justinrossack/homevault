//! Papierkorb: Gelöschtes wird nach `DATA_DIR/.papierkorb/<id>/` verschoben
//! und nach einer einstellbaren Zeit (Standard 30 Tage) endgültig gelöscht.
//!
//! „Gemeinsam“ hat einen gemeinsamen Papierkorb, jeder private Bereich
//! einen eigenen. Im Index werden die Einträge nur umgeparkt, damit Alben
//! und Favoriten beim Wiederherstellen erhalten bleiben.

use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use axum::{
    Extension, Json,
    extract::{Path as UrlPath, State},
    response::Response,
};
use rusqlite::{Connection, OptionalExtension, params};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{
    AppState,
    auth::CurrentUser,
    db::{self, Db},
    error::AppError,
    fs_api::{Kind, kind_of, live_companion},
    index, media,
    paths::{self, Resolved},
    upload::move_to_free_name,
};

pub const TRASH_DIR: &str = ".papierkorb";
pub const DEFAULT_DAYS: i64 = 30;

pub fn trash_days(conn: &Connection) -> i64 {
    db::get_setting(conn, "trash_days")
        .ok()
        .flatten()
        .and_then(|v| v.parse().ok())
        .unwrap_or(DEFAULT_DAYS)
}

fn kind_name(path: &Path, is_dir: bool) -> &'static str {
    if is_dir {
        return "dir";
    }
    match kind_of(path) {
        Kind::Image => "image",
        Kind::Video => "video",
        _ => "other",
    }
}

/// Verschiebt eine Datei oder einen Ordner (auch mit Inhalt) in den Papierkorb.
pub async fn move_to_trash(state: &AppState, user: &CurrentUser, r: Resolved) -> Result<(), AppError> {
    let meta = tokio::fs::symlink_metadata(&r.full).await?;
    let is_dir = meta.is_dir();
    let name = r.full.file_name().unwrap_or_default().to_string_lossy().into_owned();
    let size = if is_dir {
        let p = r.full.clone();
        tokio::task::spawn_blocking(move || index::dir_size(&p)).await.unwrap_or(0)
    } else {
        meta.len()
    };
    let companion = if is_dir { None } else { live_companion(&r.full).await };
    let original = paths::relative(&state.data_dir, &r.full);
    let live_original = companion.as_ref().map(|c| paths::relative(&state.data_dir, c));
    let owner = (r.space == paths::PRIVATE).then_some(user.id);
    let (kind, space, deleted_by) = (kind_name(&r.full, is_dir), r.space, user.name.clone());

    // Erst den Eintrag anlegen (für die ID), dann verschieben
    let (orig, lo, nm) = (original.clone(), live_original.clone(), name.clone());
    let id = state
        .db
        .call(move |c| {
            c.execute(
                "INSERT INTO trash (space, owner_id, deleted_by, name, kind, size, original_path, trash_path, live_original, deleted_at)
                 VALUES (?, ?, ?, ?, ?, ?, ?, '', ?, ?)",
                params![space, owner, deleted_by, nm, kind, size as i64, orig, lo, db::now()],
            )?;
            Ok(c.last_insert_rowid())
        })
        .await?;

    let rel_dir = format!("{TRASH_DIR}/{id}");
    let abs_dir = state.data_dir.join(&rel_dir);
    let mut main_moved = false;
    let moved = async {
        tokio::fs::create_dir_all(&abs_dir).await?;
        tokio::fs::rename(&r.full, abs_dir.join(&name)).await?;
        main_moved = true;
        if let Some(c) = &companion {
            tokio::fs::rename(c, abs_dir.join(c.file_name().unwrap())).await?;
        }
        Ok::<_, std::io::Error>(())
    }
    .await;
    if let Err(e) = moved {
        // alles zurück an seinen Platz, nichts verwaist im Papierkorb
        if main_moved {
            let _ = tokio::fs::rename(abs_dir.join(&name), &r.full).await;
        }
        let _ = state.db.call(move |c| Ok(c.execute("DELETE FROM trash WHERE id = ?", [id])?)).await;
        let _ = tokio::fs::remove_dir(&abs_dir).await;
        return Err(e.into());
    }

    let trash_path = format!("{rel_dir}/{name}");
    let live_trash = companion.map(|c| format!("{rel_dir}/{}", c.file_name().unwrap().to_string_lossy()));
    state
        .db
        .call(move |c| {
            c.execute(
                "UPDATE trash SET trash_path = ?, live_trash = ? WHERE id = ?",
                params![trash_path, live_trash, id],
            )?;
            index::rename_paths(c, &original, &trash_path)?;
            if let (Some(lo), Some(lt)) = (&live_original, &live_trash) {
                index::rename_paths(c, lo, lt)?;
            }
            Ok(())
        })
        .await
}

struct Item {
    id: i64,
    space: String,
    owner_id: Option<i64>,
    original: String,
    trash_path: String,
    live_original: Option<String>,
    live_trash: Option<String>,
}

fn load_item(conn: &Connection, id: i64) -> Result<Item, AppError> {
    conn.query_row(
        "SELECT id, space, owner_id, original_path, trash_path, live_original, live_trash FROM trash WHERE id = ?",
        [id],
        |r| {
            Ok(Item {
                id: r.get(0)?,
                space: r.get(1)?,
                owner_id: r.get(2)?,
                original: r.get(3)?,
                trash_path: r.get(4)?,
                live_original: r.get(5)?,
                live_trash: r.get(6)?,
            })
        },
    )
    .optional()?
    .ok_or(AppError::NotFound)
}

/// Gemeinsam: alle Benutzer; Privat: nur der Besitzer.
fn may_access(item: &Item, user: &CurrentUser) -> bool {
    item.space == paths::SHARED || item.owner_id == Some(user.id)
}

/// Endgültig löschen (Dateien + Index + Eintrag).
async fn purge(db: &Arc<Db>, data_dir: &Path, item: Item) -> Result<(), AppError> {
    let dir = data_dir.join(TRASH_DIR).join(item.id.to_string());
    match tokio::fs::remove_dir_all(&dir).await {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.into()),
    }
    db.call(move |c| {
        index::remove_paths(c, &item.trash_path)?;
        if let Some(lt) = &item.live_trash {
            index::remove_paths(c, lt)?;
        }
        c.execute("DELETE FROM trash WHERE id = ?", [item.id])?;
        Ok(())
    })
    .await
}

/// Einen Eintrag endgültig löschen (z. B. wenn sein Benutzer gelöscht wird).
pub async fn purge_id(state: &AppState, id: i64) -> Result<(), AppError> {
    let item = state.db.call(move |c| load_item(c, id)).await?;
    purge(&state.db, &state.data_dir, item).await
}

/// Entfernt abgelaufene Einträge, prüft stündlich.
pub fn start_cleanup(db: Arc<Db>, data_dir: PathBuf) {
    tokio::spawn(async move {
        loop {
            let expired = db
                .call(|c| {
                    let cutoff = db::now() - trash_days(c) * 86400;
                    let ids: Vec<i64> = c
                        .prepare("SELECT id FROM trash WHERE trash_path != '' AND deleted_at < ?")?
                        .query_map([cutoff], |r| r.get(0))?
                        .collect::<Result<_, _>>()?;
                    ids.into_iter().map(|id| load_item(c, id)).collect::<Result<Vec<_>, _>>()
                })
                .await;
            match expired {
                Ok(items) if !items.is_empty() => {
                    let n = items.len();
                    for item in items {
                        if let Err(e) = purge(&db, &data_dir, item).await {
                            tracing::warn!("Papierkorb: {e}");
                        }
                    }
                    tracing::info!("Papierkorb: {n} abgelaufene Einträge endgültig gelöscht");
                }
                Ok(_) => {}
                Err(e) => tracing::error!("Papierkorb-Aufräumen: {e}"),
            }
            tokio::time::sleep(Duration::from_secs(3600)).await;
        }
    });
}

// ---------- HTTP ----------

pub async fn list(State(state): State<AppState>, Extension(user): Extension<CurrentUser>) -> Result<Json<Value>, AppError> {
    let (items, days) = state
        .db
        .call(move |c| {
            let days = trash_days(c);
            let items = c
                .prepare(
                    "SELECT id, space, name, kind, size, original_path, deleted_by, deleted_at, live_trash IS NOT NULL
                     FROM trash WHERE trash_path != '' AND (space = ?1 OR owner_id = ?2)
                     ORDER BY deleted_at DESC",
                )?
                .query_map(params![paths::SHARED, user.id], |r| {
                    let original: String = r.get(5)?;
                    let deleted_at: i64 = r.get(7)?;
                    // Pfad des Ordners aus Sicht des Benutzers
                    let parent = original.rsplit_once('/').map_or("", |(p, _)| p).to_owned();
                    let location = crate::library::to_virtual(&user.name, &format!("{parent}/x"))
                        .map(|v| v.trim_end_matches("/x").to_owned())
                        .unwrap_or(parent);
                    Ok(json!({
                        "id": r.get::<_, i64>(0)?,
                        "space": r.get::<_, String>(1)?,
                        "name": r.get::<_, String>(2)?,
                        "kind": r.get::<_, String>(3)?,
                        "size": r.get::<_, i64>(4)?,
                        "location": location,
                        "deleted_by": r.get::<_, String>(6)?,
                        "deleted_at": deleted_at,
                        "expires_at": deleted_at + days * 86400,
                        "live": r.get::<_, bool>(8)?,
                    }))
                })?
                .collect::<Result<Vec<_>, _>>()?;
            Ok((items, days))
        })
        .await?;
    Ok(Json(json!({ "items": items, "days": days })))
}

#[derive(Deserialize)]
pub struct IdsBody {
    ids: Vec<i64>,
}

async fn load_checked(state: &AppState, user: &CurrentUser, id: i64) -> Result<Item, AppError> {
    let item = state.db.call(move |c| load_item(c, id)).await?;
    if !may_access(&item, user) {
        return Err(AppError::NotFound);
    }
    Ok(item)
}

pub async fn restore(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Json(b): Json<IdsBody>,
) -> Result<Json<Value>, AppError> {
    let mut restored = Vec::new();
    for id in b.ids {
        let item = load_checked(&state, &user, id).await?;
        let original = state.data_dir.join(&item.original);
        let parent = original.parent().ok_or(AppError::NotFound)?.to_path_buf();
        // Ordner wiederherstellen, falls er inzwischen gelöscht wurde
        tokio::fs::create_dir_all(&parent).await?;
        let name = original.file_name().unwrap_or_default().to_string_lossy().into_owned();
        let trashed = state.data_dir.join(&item.trash_path);
        let target = move_to_free_name(&trashed, &parent, &name).await?;
        let target_rel = paths::relative(&state.data_dir, &target);

        // Live-Video mit passendem (ggf. neuem) Namen zurücklegen, ohne ein
        // inzwischen vorhandenes gleichnamiges Video zu überschreiben
        let mut live_move = None;
        if let (Some(lt), Some(lo)) = (&item.live_trash, &item.live_original) {
            let ext = Path::new(lo).extension().unwrap_or_default().to_string_lossy();
            let stem = target.file_stem().unwrap_or_default().to_string_lossy();
            match move_to_free_name(&state.data_dir.join(lt), &parent, &format!("{stem}.{ext}")).await {
                Ok(live_target) => live_move = Some((lt.clone(), paths::relative(&state.data_dir, &live_target))),
                Err(e) => {
                    // Foto zurück in den Papierkorb, damit nichts verloren geht
                    let _ = tokio::fs::rename(&target, &trashed).await;
                    return Err(e.into());
                }
            }
        }
        let (tp, tr) = (item.trash_path.clone(), target_rel.clone());
        state
            .db
            .call(move |c| {
                index::rename_paths(c, &tp, &tr)?;
                if let Some((from, to)) = &live_move {
                    index::rename_paths(c, from, to)?;
                }
                c.execute("DELETE FROM trash WHERE id = ?", [item.id])?;
                Ok(())
            })
            .await?;
        let _ = tokio::fs::remove_dir_all(state.data_dir.join(TRASH_DIR).join(item.id.to_string())).await;
        restored.push(crate::library::to_virtual(&user.name, &target_rel).unwrap_or(target_rel));
    }
    state.index.request_scan();
    Ok(Json(json!({ "restored": restored })))
}

pub async fn delete_forever(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Json(b): Json<IdsBody>,
) -> Result<Json<Value>, AppError> {
    for id in b.ids {
        let item = load_checked(&state, &user, id).await?;
        purge(&state.db, &state.data_dir, item).await?;
    }
    state.index.request_scan();
    Ok(Json(json!({ "ok": true })))
}

/// Alles leeren, was dieser Benutzer sehen darf.
pub async fn empty(State(state): State<AppState>, Extension(user): Extension<CurrentUser>) -> Result<Json<Value>, AppError> {
    let uid = user.id;
    let items = state
        .db
        .call(move |c| {
            let ids: Vec<i64> = c
                .prepare("SELECT id FROM trash WHERE trash_path != '' AND (space = ?1 OR owner_id = ?2)")?
                .query_map(params![paths::SHARED, uid], |r| r.get(0))?
                .collect::<Result<_, _>>()?;
            ids.into_iter().map(|id| load_item(c, id)).collect::<Result<Vec<_>, _>>()
        })
        .await?;
    let n = items.len();
    for item in items {
        purge(&state.db, &state.data_dir, item).await?;
    }
    state.index.request_scan();
    Ok(Json(json!({ "deleted": n })))
}

pub async fn thumb(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    UrlPath(id): UrlPath<i64>,
) -> Result<Response, AppError> {
    let item = load_checked(&state, &user, id).await?;
    media::thumb_of(&state, state.data_dir.join(&item.trash_path)).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zugriff() {
        let item = |space: &str, owner: Option<i64>| Item {
            id: 1,
            space: space.into(),
            owner_id: owner,
            original: String::new(),
            trash_path: String::new(),
            live_original: None,
            live_trash: None,
        };
        let anna = CurrentUser { id: 1, name: "anna".into(), admin: false, token: String::new() };
        assert!(may_access(&item("Gemeinsam", None), &anna));
        assert!(may_access(&item("Privat", Some(1)), &anna));
        assert!(!may_access(&item("Privat", Some(2)), &anna));
    }
}
