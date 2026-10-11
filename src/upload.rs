//! Streaming-Upload: jede Datei wird stückweise in eine versteckte
//! Temp-Datei geschrieben und erst am Ende atomar umbenannt.

use std::path::{Path, PathBuf};

use axum::{
    Extension, Json,
    extract::{Multipart, Query, State},
};
use rand::Rng;
use serde_json::{Value, json};
use tokio::io::{AsyncWriteExt, BufWriter};

use crate::{AppState, auth::CurrentUser, error::AppError, fs_api::PathQuery, paths};

pub async fn upload(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Query(q): Query<PathQuery>,
    mut multipart: Multipart,
) -> Result<Json<Value>, AppError> {
    let r = paths::resolve_in_space(&state.data_dir, &user.name, &q.path)?;
    let dir = r.full.clone();

    // Speicherlimit gilt für den privaten Bereich
    let quota = if r.space == paths::PRIVATE {
        let uid = user.id;
        state
            .db
            .call(move |c| Ok(c.query_row("SELECT quota_bytes FROM users WHERE id = ?", [uid], |r| r.get::<_, Option<i64>>(0))?))
            .await?
    } else {
        None
    };
    let mut used = state.index.usage_of(&format!("{}/{}", paths::PRIVATE, user.name)) as i64;
    if quota.is_some_and(|q| used >= q) {
        return Err(AppError::BadRequest("Dein Speicherlimit ist erreicht".into()));
    }
    if !tokio::fs::metadata(&dir).await?.is_dir() {
        return Err(AppError::BadRequest("Ziel ist kein Ordner".into()));
    }

    let mut saved = Vec::new();
    while let Some(mut field) = multipart
        .next_field()
        .await
        .map_err(|e| AppError::BadRequest(e.to_string()))?
    {
        let Some(raw_name) = field.file_name().map(str::to_owned) else {
            continue;
        };
        // Browser schicken teils Pfade mit, wir nehmen nur den Dateinamen
        let name = raw_name.rsplit(['/', '\\']).next().unwrap_or("").trim().to_owned();
        if !paths::valid_name(&name) {
            return Err(AppError::BadRequest(format!("Ungültiger Dateiname: {raw_name}")));
        }

        let tmp = dir.join(format!(".upload-{:016x}.part", rand::thread_rng().r#gen::<u64>()));
        let result = async {
            let mut out = BufWriter::with_capacity(1 << 20, tokio::fs::File::create(&tmp).await?);
            while let Some(chunk) = field
                .chunk()
                .await
                .map_err(|e| AppError::BadRequest(e.to_string()))?
            {
                out.write_all(&chunk).await?;
                used += chunk.len() as i64;
                if quota.is_some_and(|q| used > q) {
                    return Err(AppError::BadRequest("Dein Speicherlimit ist erreicht".into()));
                }
            }
            out.flush().await?;
            let target = free_name(&dir, &name).await;
            tokio::fs::rename(&tmp, &target).await?;
            Ok::<_, AppError>(target)
        }
        .await;

        match result {
            Ok(target) => saved.push(r.virtual_of(&target)),
            Err(e) => {
                let _ = tokio::fs::remove_file(&tmp).await;
                return Err(e);
            }
        }
    }
    if !saved.is_empty() {
        state.index.request_scan();
    }
    Ok(Json(json!({ "saved": saved })))
}

/// "bild.jpg" -> "bild (1).jpg", falls schon vorhanden.
pub async fn free_name(dir: &Path, name: &str) -> PathBuf {
    let candidate = dir.join(name);
    if tokio::fs::symlink_metadata(&candidate).await.is_err() {
        return candidate;
    }
    let (stem, ext) = match name.rsplit_once('.') {
        Some((s, e)) if !s.is_empty() => (s, format!(".{e}")),
        _ => (name, String::new()),
    };
    for i in 1.. {
        let c = dir.join(format!("{stem} ({i}){ext}"));
        if tokio::fs::symlink_metadata(&c).await.is_err() {
            return c;
        }
    }
    unreachable!()
}
