//! Ordner auflisten, anlegen, umbenennen, löschen.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};

use axum::{
    Extension, Json,
    extract::{Query, State},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{AppState, auth::CurrentUser, error::AppError, index, library, paths, upload};

#[derive(Deserialize)]
pub struct PathQuery {
    #[serde(default)]
    pub path: String,
}

#[derive(Serialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Dir,
    Image,
    Video,
    Other,
}

fn ext_lower(path: &Path) -> String {
    path.extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default()
}

pub fn kind_of(path: &Path) -> Kind {
    match ext_lower(path).as_str() {
        "jpg" | "jpeg" | "png" | "webp" | "gif" | "bmp" | "tif" | "tiff" | "heic" | "heif"
        | "hif" => Kind::Image,
        "mp4" | "m4v" | "mov" | "webm" | "mkv" | "avi" | "3gp" => Kind::Video,
        _ => Kind::Other,
    }
}

/// Formate, die Browser nicht (zuverlässig) anzeigen können. Für diese
/// erzeugt der Server eine JPEG-Vorschau.
pub fn needs_preview(path: &Path) -> bool {
    matches!(ext_lower(path).as_str(), "heic" | "heif" | "hif" | "tif" | "tiff")
}

pub fn is_heif(path: &Path) -> bool {
    matches!(ext_lower(path).as_str(), "heic" | "heif" | "hif")
}

/// Fotos, die Teil eines iPhone Live Photos sein können.
pub fn is_live_still(path: &Path) -> bool {
    matches!(ext_lower(path).as_str(), "heic" | "heif" | "jpg" | "jpeg")
}

/// Live Photo = Foto + .MOV mit gleichem Namen im selben Ordner
/// (z. B. IMG_1234.HEIC + IMG_1234.MOV).
pub fn is_live_motion(path: &Path) -> bool {
    ext_lower(path) == "mov"
}

pub fn stem_lower(path: &Path) -> String {
    path.file_stem()
        .map(|s| s.to_string_lossy().to_lowercase())
        .unwrap_or_default()
}

/// Sucht das Video zu einem Live Photo.
pub async fn live_companion(still: &Path) -> Option<PathBuf> {
    if !is_live_still(still) {
        return None;
    }
    let stem = stem_lower(still);
    let mut rd = tokio::fs::read_dir(still.parent()?).await.ok()?;
    while let Ok(Some(e)) = rd.next_entry().await {
        let p = e.path();
        if is_live_motion(&p) && stem_lower(&p) == stem {
            return Some(p);
        }
    }
    None
}

#[derive(Serialize)]
pub struct Entry {
    name: String,
    path: String,
    kind: Kind,
    size: u64,
    /// Unix-Zeit in Sekunden
    modified: u64,
    /// Browser braucht eine Server-Vorschau (HEIC, TIFF)
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    preview: bool,
    /// Pfad des Videos, falls Live Photo
    #[serde(skip_serializing_if = "Option::is_none")]
    live: Option<String>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    favorite: bool,
    /// Art der Dokument-Vorschau: pdf, office, text, audio
    #[serde(skip_serializing_if = "Option::is_none")]
    doc: Option<&'static str>,
}

fn unix_secs(meta: &std::fs::Metadata) -> u64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_secs())
}

/// Übersicht: die beiden Bereiche als Ordner.
async fn list_spaces(state: &AppState, user: &str) -> Result<Json<Value>, AppError> {
    let mut entries = Vec::new();
    for (space, dir) in [
        (paths::PRIVATE, state.data_dir.join(paths::PRIVATE).join(user)),
        (paths::SHARED, state.data_dir.join(paths::SHARED)),
    ] {
        let meta = tokio::fs::metadata(&dir).await?;
        entries.push(Entry {
            name: space.into(),
            path: space.into(),
            kind: Kind::Dir,
            size: 0,
            modified: unix_secs(&meta),
            preview: false,
            live: None,
            favorite: false,
            doc: None,
        });
    }
    Ok(Json(json!({ "path": "", "entries": entries })))
}

pub async fn list(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Query(q): Query<PathQuery>,
) -> Result<Json<Value>, AppError> {
    let Some(r) = paths::resolve_for(&state.data_dir, &user.name, &q.path)? else {
        return list_spaces(&state, &user.name).await;
    };
    let dir = r.full.clone();
    let mut rd = tokio::fs::read_dir(&dir).await?;
    let mut entries = Vec::new();
    while let Some(e) = rd.next_entry().await? {
        let name = e.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue; // versteckte Dateien und laufende Uploads
        }
        // metadata() folgt Symlinks, damit verlinkte Ordner als Ordner erscheinen
        let Ok(meta) = tokio::fs::metadata(e.path()).await else {
            continue;
        };
        let full = dir.join(&name);
        let kind = if meta.is_dir() { Kind::Dir } else { kind_of(&full) };
        entries.push(Entry {
            path: r.virtual_of(&full),
            preview: kind == Kind::Image && needs_preview(&full),
            live: None,
            favorite: false,
            doc: (kind == Kind::Other).then(|| crate::docs::doc_kind(&full).map(|d| d.as_str())).flatten(),
            name,
            kind,
            size: if meta.is_dir() { 0 } else { meta.len() },
            modified: unix_secs(&meta),
        });
    }

    // Live Photos zusammenführen: Das Video wird beim Foto angehängt und
    // nicht separat angezeigt.
    let motions: HashMap<String, usize> = entries
        .iter()
        .enumerate()
        .filter(|(_, e)| e.kind == Kind::Video && is_live_motion(Path::new(&e.name)))
        .map(|(i, e)| (stem_lower(Path::new(&e.name)), i))
        .collect();
    let mut hidden = vec![false; entries.len()];
    for i in 0..entries.len() {
        let name = Path::new(&entries[i].name);
        if entries[i].kind == Kind::Image && is_live_still(name) {
            if let Some(&m) = motions.get(&stem_lower(name)) {
                if !hidden[m] {
                    hidden[m] = true;
                    entries[i].live = Some(entries[m].path.clone());
                }
            }
        }
    }
    let mut entries: Vec<Entry> = entries
        .into_iter()
        .zip(hidden)
        .filter(|(_, h)| !h)
        .map(|(e, _)| e)
        .collect();

    // Favoriten des Benutzers in diesem Ordner markieren
    let dir_db = paths::relative(&state.data_dir, &dir);
    let user_id = user.id;
    let favs = state
        .db
        .call(move |c| Ok(library::favorites_in_dir(c, user_id, &dir_db)?))
        .await?;
    if !favs.is_empty() {
        let favs: std::collections::HashSet<String> = favs
            .into_iter()
            .filter_map(|p| library::to_virtual(&user.name, &p))
            .collect();
        for e in &mut entries {
            e.favorite = favs.contains(&e.path);
        }
    }

    entries.sort_by(|a, b| {
        (a.kind != Kind::Dir)
            .cmp(&(b.kind != Kind::Dir))
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    Ok(Json(json!({
        "path": r.virtual_of(&dir),
        "entries": entries,
    })))
}

pub async fn mkdir(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Json(q): Json<PathQuery>,
) -> Result<Json<Value>, AppError> {
    let r = paths::resolve_in_space(&state.data_dir, &user.name, &q.path)?;
    if r.is_root() {
        return Err(AppError::Conflict("Existiert bereits".into()));
    }
    let target = r.full;
    if !target.file_name().is_some_and(|n| paths::valid_name(&n.to_string_lossy())) {
        return Err(AppError::BadRequest("Ungültiger Ordnername".into()));
    }
    tokio::fs::create_dir(&target).await?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
pub struct RenameBody {
    from: String,
    to: String,
}

async fn exists(p: &Path) -> bool {
    tokio::fs::symlink_metadata(p).await.is_ok()
}

pub async fn rename(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Json(b): Json<RenameBody>,
) -> Result<Json<Value>, AppError> {
    let rf = paths::resolve_in_space(&state.data_dir, &user.name, &b.from)?;
    let rt = paths::resolve_in_space(&state.data_dir, &user.name, &b.to)?;
    if rf.is_root() || rt.is_root() {
        return Err(AppError::BadRequest("Bereiche können nicht umbenannt werden".into()));
    }
    if rf.root != rt.root {
        return Err(AppError::BadRequest("Verschieben zwischen Bereichen ist nicht möglich".into()));
    }
    let (from, to) = (rf.full, rt.full);
    if !to.file_name().is_some_and(|n| paths::valid_name(&n.to_string_lossy())) {
        return Err(AppError::BadRequest("Ungültiger Name".into()));
    }
    tokio::fs::metadata(&from).await?;
    if exists(&to).await {
        return Err(AppError::Conflict("Ziel existiert bereits".into()));
    }

    // Live-Video mit umbenennen, sonst geht die Verbindung verloren
    let companion = match live_companion(&from).await {
        Some(video) => {
            let ext = video.extension().unwrap_or_default().to_string_lossy().into_owned();
            let stem = to.file_stem().unwrap_or_default().to_string_lossy().into_owned();
            let target = to.with_file_name(format!("{stem}.{ext}"));
            // z. B. „a.jpg“ -> „b.mov“: das Video würde danach das Foto überschreiben
            if target == to {
                return Err(AppError::Conflict("Dieser Name gehört zum Live-Video des Fotos".into()));
            }
            if target != video && exists(&target).await {
                return Err(AppError::Conflict(format!(
                    "{} existiert bereits",
                    target.file_name().unwrap().to_string_lossy()
                )));
            }
            Some((video, target))
        }
        None => None,
    };

    let (f, t) = (from.clone(), to.clone());
    tokio::task::spawn_blocking(move || upload::rename_noreplace(&f, &t))
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?
        .map_err(|e| match e.kind() {
            std::io::ErrorKind::AlreadyExists => AppError::Conflict("Ziel existiert bereits".into()),
            _ => e.into(),
        })?;
    let mut moves = vec![(paths::relative(&state.data_dir, &from), paths::relative(&state.data_dir, &to))];
    if let Some((video, target)) = companion {
        // schlägt das fehl, bleibt das Video unter altem Namen liegen (der Index folgt trotzdem dem Foto)
        let (v, t) = (video.clone(), target.clone());
        match tokio::task::spawn_blocking(move || upload::rename_noreplace(&v, &t)).await {
            Ok(Ok(())) => moves.push((paths::relative(&state.data_dir, &video), paths::relative(&state.data_dir, &target))),
            _ => tracing::warn!("Live-Video {} konnte nicht umbenannt werden", video.display()),
        }
    }
    // Index sofort nachziehen, damit Favoriten und Alben erhalten bleiben
    state
        .db
        .call(move |c| {
            for (a, b) in &moves {
                index::rename_paths(c, a, b)?;
            }
            Ok(())
        })
        .await?;
    Ok(Json(json!({ "ok": true })))
}

pub async fn delete(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Query(q): Query<PathQuery>,
) -> Result<Json<Value>, AppError> {
    let r = paths::resolve_in_space(&state.data_dir, &user.name, &q.path)?;
    if r.is_root() {
        return Err(AppError::BadRequest("Bereiche können nicht gelöscht werden".into()));
    }
    // In den Papierkorb, auch Ordner mit Inhalt (lässt sich wiederherstellen)
    crate::trash::move_to_trash(&state, &user, r).await?;
    state.index.request_scan();
    Ok(Json(json!({ "ok": true })))
}
