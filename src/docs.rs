//! Vorschau für Dokumente: PDF zeigt der Browser selbst an, Office-Dateien
//! (Word, Excel, PowerPoint, OpenDocument) wandelt LibreOffice einmalig in
//! PDF um. Für PDFs und Office-Dateien gibt es Vorschaubilder der ersten
//! Seite (poppler `pdftoppm`). Fehlen die Werkzeuge, meldet der Server
//! „nicht unterstützt“, statt zu scheitern.

use std::{
    path::{Path, PathBuf},
    process::Stdio,
    sync::OnceLock,
    time::Duration,
};

use axum::{
    Extension, Json,
    extract::{Query, Request, State},
    http::{HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use serde::Deserialize;
use serde_json::json;
use tokio::process::Command;

use crate::{AppState, auth::CurrentUser, error::AppError, media, paths};

/// Welche Art Vorschau eine Datei bekommt.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DocKind {
    Pdf,
    Office,
    Text,
    Audio,
    /// ZIP/TAR: Inhalt anzeigen
    Archive,
}

impl DocKind {
    pub fn as_str(self) -> &'static str {
        match self {
            DocKind::Pdf => "pdf",
            DocKind::Office => "office",
            DocKind::Text => "text",
            DocKind::Audio => "audio",
            DocKind::Archive => "archive",
        }
    }
}

pub fn doc_kind(path: &Path) -> Option<DocKind> {
    if crate::archive::is_archive(path) {
        return Some(DocKind::Archive);
    }
    let ext = path.extension()?.to_string_lossy().to_ascii_lowercase();
    Some(match ext.as_str() {
        "pdf" => DocKind::Pdf,
        "doc" | "docx" | "odt" | "rtf" | "xls" | "xlsx" | "ods" | "ppt" | "pptx" | "odp" | "pages" | "numbers" | "key" => {
            DocKind::Office
        }
        "txt" | "md" | "markdown" | "log" | "csv" | "tsv" | "json" | "xml" | "yaml" | "yml" | "toml" | "ini" | "conf" | "cfg"
        | "env" | "sh" | "bash" | "zsh" | "py" | "rs" | "go" | "js" | "ts" | "tsx" | "jsx" | "java" | "kt" | "c" | "h" | "cpp"
        | "hpp" | "cs" | "rb" | "php" | "swift" | "sql" | "html" | "htm" | "css" | "scss" | "srt" | "vtt" | "gpx" => DocKind::Text,
        "mp3" | "m4a" | "aac" | "wav" | "flac" | "ogg" | "oga" | "opus" => DocKind::Audio,
        _ => return None,
    })
}

/// Hat die Datei ein Vorschaubild (erste Seite)?
pub fn has_page_thumb(path: &Path) -> bool {
    matches!(doc_kind(path), Some(DocKind::Pdf | DocKind::Office))
}

fn tool_available(cmd: &str) -> bool {
    std::process::Command::new(cmd)
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok()
}

/// LibreOffice: `soffice` (Linux) bzw. das App-Bundle auf dem Mac
fn soffice() -> Option<&'static str> {
    static CMD: OnceLock<Option<&'static str>> = OnceLock::new();
    *CMD.get_or_init(|| {
        ["soffice", "libreoffice", "/Applications/LibreOffice.app/Contents/MacOS/soffice"]
            .into_iter()
            .find(|c| tool_available(c))
    })
}

fn pdftoppm() -> bool {
    static OK: OnceLock<bool> = OnceLock::new();
    *OK.get_or_init(|| {
        // pdftoppm kennt kein --version mit Exit 0 in allen Versionen, -v reicht
        std::process::Command::new("pdftoppm").arg("-v").stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok()
    })
}

/// Office-Datei als PDF (im Cache). None, wenn LibreOffice fehlt.
pub async fn office_pdf(state: &AppState, src: &Path) -> Result<Option<PathBuf>, AppError> {
    let Some(soffice) = soffice() else { return Ok(None) };
    let meta = tokio::fs::metadata(src).await?;
    let out = media::cache_path(&state.cache_dir, "docs", src, &meta, "pdf");
    if tokio::fs::metadata(&out).await.is_ok() {
        return Ok(Some(out));
    }
    // LibreOffice ist schwergewichtig: höchstens eine Umwandlung gleichzeitig
    static LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
    let _guard = LOCK.lock().await;
    if tokio::fs::metadata(&out).await.is_ok() {
        return Ok(Some(out));
    }
    let work = state.cache_dir.join("tmp").join(format!("lo-{:016x}", rand::random::<u64>()));
    tokio::fs::create_dir_all(&work).await?;
    // eigenes Profil, damit parallele/abgestürzte Instanzen nicht stören
    let profile = format!("-env:UserInstallation=file://{}", work.join("profile").display());
    let run = Command::new(soffice)
        .args([profile.as_str(), "--headless", "--norestore", "--convert-to", "pdf", "--outdir"])
        .arg(&work)
        .arg(src)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .output();
    let result = tokio::time::timeout(Duration::from_secs(180), run).await;
    let converted = work.join(src.with_extension("pdf").file_name().unwrap_or_default());
    let outcome = match result {
        Err(_) => Err("Umwandlung dauert zu lange".to_owned()),
        Ok(Err(e)) => Err(e.to_string()),
        Ok(Ok(o)) if !o.status.success() || !converted.exists() => {
            Err(String::from_utf8_lossy(&o.stderr).lines().last().unwrap_or("LibreOffice-Fehler").to_owned())
        }
        Ok(Ok(_)) => {
            tokio::fs::create_dir_all(out.parent().unwrap()).await?;
            tokio::fs::rename(&converted, &out).await.map_err(|e| e.to_string())
        }
    };
    let _ = tokio::fs::remove_dir_all(&work).await;
    match outcome {
        Ok(()) => Ok(Some(out)),
        Err(e) => {
            tracing::warn!("Dokument-Umwandlung fehlgeschlagen ({}): {e}", src.display());
            Err(AppError::Unsupported("Dieses Dokument konnte nicht umgewandelt werden".into()))
        }
    }
}

/// Vorschaubild der ersten Seite eines PDFs oder Office-Dokuments.
pub async fn page_thumb(state: &AppState, src: &Path, out: &Path) -> Result<(), String> {
    if !pdftoppm() {
        return Err("pdftoppm (poppler) ist nicht installiert".into());
    }
    let pdf = match doc_kind(src) {
        Some(DocKind::Office) => office_pdf(state, src)
            .await
            .map_err(|e| e.to_string())?
            .ok_or("LibreOffice ist nicht installiert")?,
        _ => src.to_path_buf(),
    };
    // pdftoppm hängt ".jpg" selbst an: Präfix ist der Temp-Name ohne diese Endung
    let tmp = media::tmp_for(out);
    let prefix = tmp.with_extension("");
    let status = Command::new("pdftoppm")
        .args(["-jpeg", "-jpegopt", "quality=80", "-f", "1", "-l", "1", "-singlefile", "-scale-to", "480"])
        .arg(&pdf)
        .arg(&prefix)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .status()
        .await
        .map_err(|e| e.to_string())?;
    let produced = tmp;
    if !status.success() || !produced.exists() {
        return Err("pdftoppm konnte keine Seite erzeugen".into());
    }
    tokio::fs::rename(&produced, out).await.map_err(|e| e.to_string())
}

#[derive(Deserialize)]
pub struct PathQuery {
    path: String,
}

/// Dokument als PDF zum Anzeigen im Browser (PDF direkt, Office umgewandelt).
pub async fn view(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Query(q): Query<PathQuery>,
    req: Request,
) -> Result<Response, AppError> {
    let src = paths::resolve_in_space(&state.data_dir, &user.name, &q.path)?.full;
    if !tokio::fs::metadata(&src).await?.is_file() {
        return Err(AppError::NotFound);
    }
    let pdf = match doc_kind(&src) {
        Some(DocKind::Pdf) => src,
        Some(DocKind::Office) => office_pdf(&state, &src)
            .await?
            .ok_or_else(|| AppError::Unsupported("Vorschau für Office-Dokumente ist auf diesem Server nicht eingerichtet".into()))?,
        _ => return Err(AppError::Unsupported("Für diesen Dateityp gibt es keine Dokument-Vorschau".into())),
    };
    let mut res = media::serve_path(&pdf, req, "private, no-cache").await?;
    res.headers_mut().insert(header::CONTENT_TYPE, HeaderValue::from_static("application/pdf"));
    Ok(res)
}

/// Was dieser Server an Vorschauen kann (für Hinweise in der Oberfläche).
pub async fn capabilities() -> Response {
    let (office, thumbs) = tokio::task::spawn_blocking(|| (soffice().is_some(), pdftoppm())).await.unwrap_or((false, false));
    (StatusCode::OK, Json(json!({ "office": office, "page_thumbs": thumbs }))).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dateitypen() {
        assert_eq!(doc_kind(Path::new("a.PDF")), Some(DocKind::Pdf));
        assert_eq!(doc_kind(Path::new("Bewerbung.docx")), Some(DocKind::Office));
        assert_eq!(doc_kind(Path::new("Tabelle.xlsx")), Some(DocKind::Office));
        assert_eq!(doc_kind(Path::new("notizen.md")), Some(DocKind::Text));
        assert_eq!(doc_kind(Path::new("lied.mp3")), Some(DocKind::Audio));
        assert_eq!(doc_kind(Path::new("archiv.zip")), Some(DocKind::Archive));
        assert_eq!(doc_kind(Path::new("archiv.rar")), None);
        assert!(has_page_thumb(Path::new("x.pptx")));
        assert!(!has_page_thumb(Path::new("x.txt")));
    }
}
