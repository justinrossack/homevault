//! Speicher-Übersicht: freier Platz auf der Platte und Belegung pro Bereich.
//! Die Belegung zählt der Medien-Index bei jedem Scan mit.

use std::path::Path;

use axum::{Extension, Json, extract::State};
use serde_json::{Value, json};

use crate::{AppState, auth::CurrentUser, error::AppError, paths};

/// Warnung, wenn weniger als 10 % oder weniger als 20 GB frei sind.
const WARN_FRACTION: f64 = 0.10;
const WARN_BYTES: u64 = 20 * 1024 * 1024 * 1024;

/// (gesamt, frei) für das Dateisystem von `path`.
pub fn disk_space(path: &Path) -> Option<(u64, u64)> {
    use std::{ffi::CString, os::unix::ffi::OsStrExt};
    let c = CString::new(path.as_os_str().as_bytes()).ok()?;
    let mut st: libc::statvfs = unsafe { std::mem::zeroed() };
    // SAFETY: c ist ein gültiger, nullterminierter Pfad, st ein beschreibbarer Puffer
    if unsafe { libc::statvfs(c.as_ptr(), &mut st) } != 0 {
        return None;
    }
    let frsize = st.f_frsize as u64;
    Some((st.f_blocks as u64 * frsize, st.f_bavail as u64 * frsize))
}

pub async fn overview(State(state): State<AppState>, Extension(user): Extension<CurrentUser>) -> Result<Json<Value>, AppError> {
    let data_dir = state.data_dir.clone();
    let (total, free) = tokio::task::spawn_blocking(move || disk_space(&data_dir)).await.ok().flatten().unwrap_or((0, 0));
    let status = state.index.status();
    let my_area = format!("{}/{}", paths::PRIVATE, user.name);
    let uid = user.id;
    let quota: Option<i64> = state
        .db
        .call(move |c| Ok(c.query_row("SELECT quota_bytes FROM users WHERE id = ?", [uid], |r| r.get(0))?))
        .await?;

    let mut out = json!({
        "disk": { "total": total, "free": free },
        "warning": total > 0 && (free < WARN_BYTES || (free as f64) < total as f64 * WARN_FRACTION),
        "me": {
            "used": status.usage.get(&my_area).map_or(0, |u| u.bytes),
            "quota": quota,
        },
        "shared": status.usage.get(paths::SHARED).cloned().unwrap_or_default(),
        "last_scan": status.last_scan,
    });

    if user.admin {
        let names: Vec<(String, Option<i64>)> = state
            .db
            .call(|c| {
                Ok(c.prepare("SELECT name, quota_bytes FROM users ORDER BY name")?
                    .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
                    .collect::<Result<_, _>>()?)
            })
            .await?;
        let users: Vec<Value> = names
            .into_iter()
            .map(|(name, quota)| {
                let u = status.usage.get(&format!("{}/{name}", paths::PRIVATE)).cloned().unwrap_or_default();
                json!({ "name": name, "quota": quota, "usage": u })
            })
            .collect();
        out["users"] = json!(users);
        out["trash"] = json!(status.trash_bytes);
        out["cache"] = json!(status.cache_bytes);
    }
    Ok(Json(out))
}
