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

        let tmp = TempFile(dir.join(format!(".upload-{:016x}.part", rand::thread_rng().r#gen::<u64>())));
        let result = async {
            let mut out = BufWriter::with_capacity(1 << 20, tokio::fs::File::create(&tmp.0).await?);
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
            Ok::<_, AppError>(move_to_free_name(&tmp.0, &dir, &name).await?)
        }
        .await;
        // Temp-Datei wird beim Verlassen entfernt (auch bei Abbruch der Verbindung)
        saved.push(r.virtual_of(&result?));
    }
    if !saved.is_empty() {
        state.index.request_scan();
    }
    Ok(Json(json!({ "saved": saved })))
}

/// Entfernt die Temp-Datei, wenn der Upload nicht fertig wird – auch wenn
/// der Browser die Verbindung abbricht und die Anfrage verworfen wird.
struct TempFile(PathBuf);

impl Drop for TempFile {
    fn drop(&mut self) {
        // nach dem Umbenennen gibt es sie nicht mehr
        let _ = std::fs::remove_file(&self.0);
    }
}

/// "bild.jpg", "bild (1).jpg", "bild (2).jpg" …
fn name_candidates(name: &str) -> impl Iterator<Item = String> + '_ {
    let (stem, ext) = match name.rsplit_once('.') {
        Some((s, e)) if !s.is_empty() => (s, format!(".{e}")),
        _ => (name, String::new()),
    };
    std::iter::once(name.to_owned()).chain((1..).map(move |i| format!("{stem} ({i}){ext}")))
}

/// Verschiebt `from` unter dem ersten freien Namen nach `dir`. Überschreibt
/// nie etwas, auch nicht, wenn gleichzeitig eine gleichnamige Datei ankommt.
pub async fn move_to_free_name(from: &Path, dir: &Path, name: &str) -> std::io::Result<PathBuf> {
    let (from, dir, name) = (from.to_owned(), dir.to_owned(), name.to_owned());
    tokio::task::spawn_blocking(move || {
        for candidate in name_candidates(&name).take(100_000) {
            let to = dir.join(candidate);
            match rename_noreplace(&from, &to) {
                Ok(()) => return Ok(to),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(e),
            }
        }
        Err(std::io::Error::other("kein freier Dateiname gefunden"))
    })
    .await?
}

/// Umbenennen, aber nur wenn das Ziel noch nicht existiert.
pub fn rename_noreplace(from: &Path, to: &Path) -> std::io::Result<()> {
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::ffi::OsStrExt;
        let (f, t) = (std::ffi::CString::new(from.as_os_str().as_bytes())?, std::ffi::CString::new(to.as_os_str().as_bytes())?);
        const RENAME_NOREPLACE: libc::c_uint = 1;
        let r = unsafe { libc::syscall(libc::SYS_renameat2, libc::AT_FDCWD, f.as_ptr(), libc::AT_FDCWD, t.as_ptr(), RENAME_NOREPLACE) };
        if r == 0 {
            return Ok(());
        }
        let e = std::io::Error::last_os_error();
        // Dateisystem ohne renameat2: wie unten prüfen und umbenennen
        if !matches!(e.raw_os_error(), Some(libc::EINVAL) | Some(libc::ENOSYS)) {
            return Err(e);
        }
    }
    if std::fs::symlink_metadata(to).is_ok() {
        return Err(std::io::ErrorKind::AlreadyExists.into());
    }
    std::fs::rename(from, to)
}
