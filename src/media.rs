//! Original-Dateien (mit HTTP-Range), Thumbnails, Vorschauen für HEIC/TIFF
//! und Live-Photo-Videos ausliefern.

use std::{
    fs::Metadata,
    path::{Path, PathBuf},
    process::Stdio,
    sync::OnceLock,
    time::UNIX_EPOCH,
};

use axum::{
    Extension,
    body::Body,
    extract::{Query, Request, State},
    http::{HeaderValue, header},
    response::{IntoResponse, Response},
};
use image::{DynamicImage, ImageDecoder, ImageReader};
use serde::Deserialize;
use tower::ServiceExt;
use tower_http::services::ServeFile;

use crate::{
    AppState,
    auth::CurrentUser,
    error::AppError,
    fs_api::{Kind, is_heif, kind_of, live_companion, needs_preview},
    paths,
};

const THUMB_SIZE: u32 = 480;
/// Längste Kante der Vorschau für HEIC/TIFF
const PREVIEW_SIZE: u32 = 4096;
/// Längste Kante der umgewandelten Live-Photo-Videos
const LIVE_SIZE: u32 = 1440;

pub const IMMUTABLE: &str = "private, max-age=31536000, immutable";

#[derive(Deserialize)]
pub struct FileQuery {
    #[serde(default)]
    path: String,
    #[serde(default)]
    download: bool,
}

/// Liefert eine Datei mit Range-Support (Vorspulen), Content-Type und Last-Modified aus.
pub async fn serve_path(path: &Path, req: Request, cache: &'static str) -> Result<Response, AppError> {
    let mut res = ServeFile::new(path)
        .oneshot(req)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?
        .map(Body::new);
    res.headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static(cache));
    Ok(res)
}

pub async fn serve_file(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Query(q): Query<FileQuery>,
    req: Request,
) -> Result<Response, AppError> {
    let path = paths::resolve_in_space(&state.data_dir, &user.name, &q.path)?.full;
    send_file(&path, req, q.download).await
}

/// Original-Datei, optional als Download (auch für Teilen-Links).
pub async fn send_file(path: &Path, req: Request, download: bool) -> Result<Response, AppError> {
    if !tokio::fs::metadata(path).await?.is_file() {
        return Err(AppError::NotFound);
    }
    let mut res = serve_path(path, req, "private, no-cache").await?;
    // Hochgeladene HTML/SVG/XML-Dateien dürfen im Browser keinen Code
    // ausführen (sonst könnte eine Datei im Namen des Betrachters handeln)
    let ct = res.headers().get(header::CONTENT_TYPE).and_then(|v| v.to_str().ok()).unwrap_or("").to_owned();
    if ct.starts_with("text/") || ct.contains("xml") || ct.contains("javascript") {
        res.headers_mut().insert(header::CONTENT_SECURITY_POLICY, HeaderValue::from_static("sandbox"));
    }
    res.headers_mut().insert(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    if download {
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        let ascii: String = name
            .chars()
            .map(|c| if c.is_ascii_graphic() && c != '"' || c == ' ' { c } else { '_' })
            .collect();
        let value = format!(
            "attachment; filename=\"{ascii}\"; filename*=UTF-8''{}",
            percent_encode(&name)
        );
        if let Ok(v) = HeaderValue::from_str(&value) {
            res.headers_mut().insert(header::CONTENT_DISPOSITION, v);
        }
    }
    Ok(res)
}

fn percent_encode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

// ---------- Cache ----------

/// Cache-Datei für eine Quelle. Der Schlüssel ändert sich, sobald die
/// Quelle verändert wird.
pub fn cache_path(cache_dir: &Path, bucket: &str, src: &Path, meta: &Metadata, ext: &str) -> PathBuf {
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_nanos());
    let key = blake3::hash(format!("{}|{}|{}", src.display(), mtime, meta.len()).as_bytes())
        .to_hex();
    cache_dir
        .join(bucket)
        .join(&key[..2])
        .join(format!("{key}.{ext}"))
}

/// Erzeugt `out` einmalig mit `make`, begrenzt durch den Semaphore.
/// Gleichzeitige Anfragen für dieselbe Datei warten aufeinander nicht,
/// schreiben aber atomar (tmp + rename), daher ist doppelte Arbeit harmlos.
async fn cached<F, Fut>(state: &AppState, out: &Path, make: F) -> Result<(), AppError>
where
    F: FnOnce(PathBuf) -> Fut,
    Fut: Future<Output = Result<(), String>>,
{
    if tokio::fs::metadata(out).await.is_ok() {
        return Ok(());
    }
    let _permit = state.thumb_limit.acquire().await.unwrap();
    if tokio::fs::metadata(out).await.is_ok() {
        return Ok(());
    }
    tokio::fs::create_dir_all(out.parent().unwrap()).await?;
    make(out.to_path_buf()).await.map_err(|e| {
        tracing::warn!("Konvertierung fehlgeschlagen ({}): {e}", out.display());
        AppError::NotFound
    })
}

fn blocking<F>(f: F) -> impl Future<Output = Result<(), String>>
where
    F: FnOnce() -> Result<(), String> + Send + 'static,
{
    async move {
        tokio::task::spawn_blocking(f)
            .await
            .map_err(|e| e.to_string())
            .and_then(|r| r)
    }
}

pub fn tmp_for(out: &Path) -> PathBuf {
    let ext = out.extension().unwrap_or_default().to_string_lossy();
    out.with_extension(format!("{:x}.tmp.{ext}", rand::random::<u32>()))
}

// ---------- Thumbnails ----------

pub async fn thumb(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Query(q): Query<FileQuery>,
) -> Result<Response, AppError> {
    let src = paths::resolve_in_space(&state.data_dir, &user.name, &q.path)?.full;
    thumb_of(&state, src).await
}

/// Thumbnail einer Datei (auch für Papierkorb und Teilen-Links).
pub async fn thumb_of(state: &AppState, src: PathBuf) -> Result<Response, AppError> {
    let state = state.clone();
    let meta = tokio::fs::metadata(&src).await?;
    let kind = kind_of(&src);
    let is_doc = crate::docs::has_page_thumb(&src);
    if !meta.is_file() || !(matches!(kind, Kind::Image | Kind::Video) || is_doc) {
        return Err(AppError::NotFound);
    }

    let out = cache_path(&state.cache_dir, "thumbs", &src, &meta, "jpg");
    if tokio::fs::metadata(&out).await.is_err() {
        if is_doc {
            // erste Seite von PDF/Office-Dokument
            let st = state.clone();
            cached(&state, &out, |out| async move { crate::docs::page_thumb(&st, &src, &out).await }).await?;
        } else if kind == Kind::Video {
            cached(&state, &out, |out| async move { video_thumb(&src, &out).await }).await?;
        } else {
            // HEIC/TIFF: Thumbnail aus der (ohnehin benötigten) Vorschau
            let source = if needs_preview(&src) {
                ensure_preview(&state, &src, &meta).await?
            } else {
                src
            };
            cached(&state, &out, |out| {
                blocking(move || {
                    let img = open_oriented(&source)?.thumbnail(THUMB_SIZE, THUMB_SIZE);
                    write_jpeg(&img, &out, 80)
                })
            })
            .await?;
        }
    }
    let bytes = tokio::fs::read(&out).await?;
    Ok(([(header::CONTENT_TYPE, "image/jpeg"), (header::CACHE_CONTROL, IMMUTABLE)], bytes).into_response())
}

// ---------- Vorschau (HEIC, TIFF) ----------

pub async fn preview(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Query(q): Query<FileQuery>,
    req: Request,
) -> Result<Response, AppError> {
    let src = paths::resolve_in_space(&state.data_dir, &user.name, &q.path)?.full;
    preview_of(&state, &src, req).await
}

pub async fn preview_of(state: &AppState, src: &Path, req: Request) -> Result<Response, AppError> {
    let meta = tokio::fs::metadata(src).await?;
    if !meta.is_file() || !needs_preview(src) {
        return Err(AppError::NotFound);
    }
    let out = ensure_preview(state, src, &meta).await?;
    serve_path(&out, req, IMMUTABLE).await
}

async fn ensure_preview(state: &AppState, src: &Path, meta: &Metadata) -> Result<PathBuf, AppError> {
    let out = cache_path(&state.cache_dir, "previews", src, meta, "jpg");
    let src = src.to_path_buf();
    let tmp_root = state.cache_dir.join("tmp");
    cached(state, &out, |out| {
        blocking(move || {
            let img = if is_heif(&src) {
                decode_heif(&src, &tmp_root)?
            } else {
                open_oriented(&src)?
            };
            let img = if img.width().max(img.height()) > PREVIEW_SIZE {
                img.resize(PREVIEW_SIZE, PREVIEW_SIZE, image::imageops::FilterType::CatmullRom)
            } else {
                img
            };
            write_jpeg(&img, &out, 88)
        })
    })
    .await?;
    Ok(out)
}

/// `heif-dec` (neuere libheif) bzw. `heif-convert` (ältere) suchen.
fn heif_cmd() -> Option<&'static str> {
    static CMD: OnceLock<Option<&'static str>> = OnceLock::new();
    *CMD.get_or_init(|| {
        ["heif-dec", "heif-convert"].into_iter().find(|c| {
            std::process::Command::new(c)
                .arg("--version")
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .is_ok()
        })
    })
}

/// HEIC -> Bild über libheif. Drehung/Spiegelung wendet libheif selbst an.
fn decode_heif(src: &Path, tmp_root: &Path) -> Result<DynamicImage, String> {
    let cmd = heif_cmd().ok_or("libheif (heif-dec) ist nicht installiert")?;
    // eigener Ordner, weil libheif evtl. Zusatzbilder (Tiefenkarte) daneben schreibt
    let dir = tmp_root.join(format!("{:016x}", rand::random::<u64>()));
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let out = dir.join("out.jpg");
    let result = std::process::Command::new(cmd)
        .args(["-q", "95"])
        .arg(src)
        .arg(&out)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| e.to_string())
        .and_then(|o| {
            if o.status.success() {
                open_oriented(&out)
            } else {
                Err(String::from_utf8_lossy(&o.stderr).trim().to_owned())
            }
        });
    let _ = std::fs::remove_dir_all(&dir);
    result
}

/// Öffnet ein Bild und dreht es gemäß EXIF (Handyfotos).
fn open_oriented(src: &Path) -> Result<DynamicImage, String> {
    let mut decoder = ImageReader::open(src)
        .map_err(|e| e.to_string())?
        .with_guessed_format()
        .map_err(|e| e.to_string())?
        .into_decoder()
        .map_err(|e| e.to_string())?;
    let orientation = decoder.orientation().ok();
    let mut img = DynamicImage::from_decoder(decoder).map_err(|e| e.to_string())?;
    if let Some(o) = orientation {
        img.apply_orientation(o);
    }
    Ok(img)
}

fn write_jpeg(img: &DynamicImage, out: &Path, quality: u8) -> Result<(), String> {
    let tmp = tmp_for(out);
    let mut buf = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buf, quality)
        .encode_image(&img.to_rgb8())
        .map_err(|e| e.to_string())?;
    std::fs::write(&tmp, buf).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, out).map_err(|e| e.to_string())
}

// ---------- Videos ----------

pub async fn ffmpeg(args: &[&std::ffi::OsStr]) -> Result<(), String> {
    let out = tokio::process::Command::new("ffmpeg")
        .args(["-v", "error", "-y"])
        .args(args)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .output()
        .await
        .map_err(|e| format!("ffmpeg nicht verfügbar: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_owned())
    }
}

async fn video_thumb(src: &Path, out: &Path) -> Result<(), String> {
    let tmp = tmp_for(out);
    let vf = format!("scale={THUMB_SIZE}:{THUMB_SIZE}:force_original_aspect_ratio=decrease");
    // erst Frame bei 1s, bei sehr kurzen Videos den ersten Frame
    for seek in ["1", "0"] {
        let ok = ffmpeg(&[
            "-ss".as_ref(), seek.as_ref(), "-i".as_ref(), src.as_os_str(),
            "-frames:v".as_ref(), "1".as_ref(), "-vf".as_ref(), vf.as_ref(),
            "-q:v".as_ref(), "4".as_ref(), tmp.as_os_str(),
        ])
        .await;
        if ok.is_ok() && tokio::fs::metadata(&tmp).await.is_ok_and(|m| m.len() > 0) {
            return tokio::fs::rename(&tmp, out).await.map_err(|e| e.to_string());
        }
    }
    let _ = tokio::fs::remove_file(&tmp).await;
    Err("ffmpeg konnte kein Bild erzeugen".into())
}

/// Video eines Live Photos. iPhones speichern es meist als HEVC, was Chrome
/// und Firefox oft nicht abspielen. Daher einmalig nach H.264 umwandeln
/// (dauert bei 3 Sekunden Clip nur kurz).
pub async fn live(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Query(q): Query<FileQuery>,
    req: Request,
) -> Result<Response, AppError> {
    let still = paths::resolve_in_space(&state.data_dir, &user.name, &q.path)?.full;
    live_of(&state, &still, req).await
}

pub async fn live_of(state: &AppState, still: &Path, req: Request) -> Result<Response, AppError> {
    let video = live_companion(still).await.ok_or(AppError::NotFound)?;
    let meta = tokio::fs::metadata(&video).await?;
    let out = cache_path(&state.cache_dir, "live", &video, &meta, "mp4");
    cached(state, &out, |out| async move {
        let tmp = tmp_for(&out);
        let vf = format!(
            "scale={LIVE_SIZE}:{LIVE_SIZE}:force_original_aspect_ratio=decrease:force_divisible_by=2,format=yuv420p"
        );
        let res = ffmpeg(&[
            "-i".as_ref(), video.as_os_str(),
            "-map".as_ref(), "0:v:0".as_ref(), "-map".as_ref(), "0:a:0?".as_ref(),
            "-vf".as_ref(), vf.as_ref(),
            "-c:v".as_ref(), "libx264".as_ref(), "-preset".as_ref(), "veryfast".as_ref(),
            "-crf".as_ref(), "23".as_ref(),
            "-c:a".as_ref(), "aac".as_ref(), "-b:a".as_ref(), "96k".as_ref(),
            "-movflags".as_ref(), "+faststart".as_ref(),
            tmp.as_os_str(),
        ])
        .await;
        match res {
            Ok(()) => tokio::fs::rename(&tmp, &out).await.map_err(|e| e.to_string()),
            Err(e) => {
                let _ = tokio::fs::remove_file(&tmp).await;
                Err(e)
            }
        }
    })
    .await?;
    serve_path(&out, req, IMMUTABLE).await
}
