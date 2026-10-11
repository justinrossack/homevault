//! Inhalt von Archiven (ZIP, TAR, TAR.GZ) anzeigen und einzelne Dateien
//! daraus herunterladen, ohne das ganze Archiv zu entpacken.
//!
//! Beim Auflisten wird nur das Inhaltsverzeichnis gelesen (bei ZIP ohne
//! zu entpacken), daher auch bei großen Archiven schnell. Eine einzelne
//! Datei wird beim Herunterladen direkt aus dem Archiv gestreamt.

use std::{
    fs::File,
    io::{self, BufReader, Read, Write},
    path::Path,
};

use axum::{
    Extension, Json,
    body::{Body, Bytes},
    extract::{Query, State},
    http::{HeaderValue, header},
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::sync::{mpsc, oneshot};
use tokio_stream::wrappers::ReceiverStream;

use crate::{AppState, auth::CurrentUser, error::AppError, paths};

/// Mehr Einträge werden nicht angezeigt (Schutz vor riesigen Archiven).
const MAX_ENTRIES: usize = 10_000;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Format {
    Zip,
    Tar,
    TarGz,
}

fn format_of(path: &Path) -> Option<Format> {
    let name = path.file_name()?.to_string_lossy().to_ascii_lowercase();
    if name.ends_with(".zip") {
        Some(Format::Zip)
    } else if name.ends_with(".tar.gz") || name.ends_with(".tgz") {
        Some(Format::TarGz)
    } else if name.ends_with(".tar") {
        Some(Format::Tar)
    } else {
        None
    }
}

pub fn is_archive(path: &Path) -> bool {
    format_of(path).is_some()
}

#[derive(Serialize)]
struct Item {
    /// Pfad im Archiv, z. B. "Urlaub/IMG_1.jpg"
    path: String,
    dir: bool,
    size: u64,
    /// gepackte Größe (nur ZIP)
    #[serde(skip_serializing_if = "Option::is_none")]
    compressed: Option<u64>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    encrypted: bool,
}

/// Pfade im Archiv vereinheitlichen (Windows-Trenner, führendes "./" oder "/").
fn clean(name: &str) -> String {
    name.replace('\\', "/").trim_start_matches("./").trim_start_matches('/').to_owned()
}

fn is_mac_junk(path: &str) -> bool {
    path.starts_with("__MACOSX/") || path.split('/').any(|p| p.starts_with("._") || p == ".DS_Store")
}

fn tar_reader(path: &Path, format: Format) -> io::Result<tar::Archive<Box<dyn Read>>> {
    let file = BufReader::new(File::open(path)?);
    let reader: Box<dyn Read> = if format == Format::TarGz { Box::new(flate2::read::GzDecoder::new(file)) } else { Box::new(file) };
    Ok(tar::Archive::new(reader))
}

fn list_sync(path: &Path) -> Result<(Vec<Item>, bool), String> {
    let format = format_of(path).ok_or("Kein unterstütztes Archiv")?;
    let mut items = Vec::new();
    let mut truncated = false;
    if format == Format::Zip {
        let mut zip = zip::ZipArchive::new(BufReader::new(File::open(path).map_err(|e| e.to_string())?))
            .map_err(|e| format!("ZIP-Datei ist beschädigt: {e}"))?;
        for i in 0..zip.len() {
            if items.len() >= MAX_ENTRIES {
                truncated = true;
                break;
            }
            // raw: nur Kopfdaten lesen, nichts entpacken
            let f = zip.by_index_raw(i).map_err(|e| e.to_string())?;
            items.push(Item {
                path: f.name().map(|n| clean(&n)).unwrap_or_default(),
                dir: f.is_dir(),
                size: f.size(),
                compressed: Some(f.compressed_size()),
                encrypted: f.encrypted(),
            });
        }
    } else {
        let mut tar = tar_reader(path, format).map_err(|e| e.to_string())?;
        for entry in tar.entries().map_err(|e| format!("Archiv ist beschädigt: {e}"))? {
            if items.len() >= MAX_ENTRIES {
                truncated = true;
                break;
            }
            let entry = entry.map_err(|e| format!("Archiv ist beschädigt: {e}"))?;
            let header = entry.header();
            let kind = header.entry_type();
            if !(kind.is_file() || kind.is_dir()) {
                continue; // Links, Gerätedateien usw. überspringen
            }
            items.push(Item {
                path: clean(&entry.path().map_err(|e| e.to_string())?.to_string_lossy()),
                dir: kind.is_dir(),
                size: header.size().unwrap_or(0),
                compressed: None,
                encrypted: false,
            });
        }
    }
    // macOS-Hilfsdateien ausblenden (wie im Finder)
    items.retain(|i| !i.path.is_empty() && !is_mac_junk(&i.path));
    Ok((items, truncated))
}

#[derive(Deserialize)]
pub struct ListQuery {
    path: String,
}

pub async fn list(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Query(q): Query<ListQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    let src = paths::resolve_in_space(&state.data_dir, &user.name, &q.path)?.full;
    if !is_archive(&src) {
        return Err(AppError::Unsupported("Kein unterstütztes Archiv (ZIP, TAR, TAR.GZ)".into()));
    }
    let (items, truncated) = tokio::task::spawn_blocking(move || list_sync(&src))
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?
        .map_err(AppError::Unsupported)?;
    let files = items.iter().filter(|i| !i.dir).count();
    let total: u64 = items.iter().map(|i| i.size).sum();
    Ok(Json(json!({ "entries": items, "files": files, "total_size": total, "truncated": truncated })))
}

/// Schreibt in einen Kanal, aus dem die HTTP-Antwort gestreamt wird.
struct ChannelWriter(mpsc::Sender<io::Result<Bytes>>);

impl Write for ChannelWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0
            .blocking_send(Ok(Bytes::copy_from_slice(buf)))
            .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "Download abgebrochen"))?;
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// Sucht `entry` im Archiv, meldet über `found` ob es ihn gibt, und streamt ihn dann.
fn extract_sync(path: &Path, entry: &str, found: oneshot::Sender<Result<u64, String>>, out: mpsc::Sender<io::Result<Bytes>>) {
    let format = match format_of(path) {
        Some(f) => f,
        None => return drop(found.send(Err("Kein Archiv".into()))),
    };
    let mut writer = io::BufWriter::with_capacity(64 * 1024, ChannelWriter(out));
    if format == Format::Zip {
        let file = match File::open(path) {
            Ok(f) => f,
            Err(e) => return drop(found.send(Err(e.to_string()))),
        };
        let mut zip = match zip::ZipArchive::new(BufReader::new(file)) {
            Ok(z) => z,
            Err(e) => return drop(found.send(Err(e.to_string()))),
        };
        let index = (0..zip.len()).find(|&i| zip.name_for_index(i).and_then(|n| n.ok()).is_some_and(|n| clean(&n) == entry));
        let Some(index) = index else { return drop(found.send(Err("Datei nicht im Archiv".into()))) };
        let mut f = match zip.by_index(index) {
            Ok(f) if f.is_file() => f,
            Ok(_) => return drop(found.send(Err("Das ist ein Ordner".into()))),
            Err(e) => {
                return drop(found.send(Err(if e.to_string().contains("assword") {
                    "Die Datei ist verschlüsselt".to_owned()
                } else {
                    e.to_string()
                })));
            }
        };
        let _ = found.send(Ok(f.size()));
        let _ = io::copy(&mut f, &mut writer).and_then(|_| writer.flush());
    } else {
        let mut tar = match tar_reader(path, format) {
            Ok(t) => t,
            Err(e) => return drop(found.send(Err(e.to_string()))),
        };
        let Ok(entries) = tar.entries() else { return drop(found.send(Err("Archiv ist beschädigt".into()))) };
        for e in entries.flatten() {
            let matches = e.path().is_ok_and(|p| clean(&p.to_string_lossy()) == entry) && e.header().entry_type().is_file();
            if matches {
                let size = e.header().size().unwrap_or(0);
                let _ = found.send(Ok(size));
                let mut e = e;
                let _ = io::copy(&mut e, &mut writer).and_then(|_| writer.flush());
                return;
            }
        }
        let _ = found.send(Err("Datei nicht im Archiv".into()));
    }
}

#[derive(Deserialize)]
pub struct FileQuery {
    path: String,
    entry: String,
}

/// Eine einzelne Datei aus dem Archiv herunterladen.
pub async fn file(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Query(q): Query<FileQuery>,
) -> Result<Response, AppError> {
    let src = paths::resolve_in_space(&state.data_dir, &user.name, &q.path)?.full;
    if !is_archive(&src) {
        return Err(AppError::NotFound);
    }
    let entry = clean(&q.entry);
    let name = entry.rsplit('/').next().unwrap_or("datei").to_owned();
    let (found_tx, found_rx) = oneshot::channel();
    let (tx, rx) = mpsc::channel(16);
    tokio::task::spawn_blocking(move || extract_sync(&src, &entry, found_tx, tx));
    let size = found_rx
        .await
        .map_err(|_| AppError::Internal("Entpacken abgebrochen".into()))?
        .map_err(|e| if e.contains("nicht im Archiv") { AppError::NotFound } else { AppError::Unsupported(e) })?;

    let mut res = Body::from_stream(ReceiverStream::new(rx)).into_response();
    let mime = mime_guess::from_path(&name).first_or_octet_stream();
    let h = res.headers_mut();
    h.insert(header::CONTENT_TYPE, HeaderValue::from_str(mime.as_ref()).unwrap_or(HeaderValue::from_static("application/octet-stream")));
    h.insert(header::CONTENT_LENGTH, HeaderValue::from(size));
    h.insert(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    let ascii: String = name.chars().map(|c| if c.is_ascii_graphic() && c != '"' || c == ' ' { c } else { '_' }).collect();
    let encoded: String = name
        .bytes()
        .map(|b| if b.is_ascii_alphanumeric() || b"-._~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") })
        .collect();
    if let Ok(v) = HeaderValue::from_str(&format!("attachment; filename=\"{ascii}\"; filename*=UTF-8''{encoded}")) {
        h.insert(header::CONTENT_DISPOSITION, v);
    }
    Ok(res)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zip_auflisten_und_entpacken() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("test.zip");
        {
            let mut w = zip::ZipWriter::new(File::create(&p).unwrap());
            let opt = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
            w.add_directory("Urlaub/", opt).unwrap();
            w.start_file("Urlaub/notiz.txt", opt).unwrap();
            w.write_all(b"hallo aus dem zip").unwrap();
            w.finish().unwrap();
        }
        let (items, truncated) = list_sync(&p).unwrap();
        assert!(!truncated);
        assert_eq!(items.len(), 2);
        assert!(items.iter().any(|i| i.path == "Urlaub/notiz.txt" && i.size == 17 && !i.dir));

        let (ftx, frx) = oneshot::channel();
        let (tx, mut rx) = mpsc::channel(16);
        std::thread::spawn(move || extract_sync(&p, "Urlaub/notiz.txt", ftx, tx));
        assert_eq!(frx.blocking_recv().unwrap(), Ok(17));
        let mut data = Vec::new();
        while let Some(chunk) = rx.blocking_recv() {
            data.extend_from_slice(&chunk.unwrap());
        }
        assert_eq!(data, b"hallo aus dem zip");
    }

    #[test]
    fn formate() {
        assert_eq!(format_of(Path::new("a.ZIP")), Some(Format::Zip));
        assert_eq!(format_of(Path::new("backup.tar.gz")), Some(Format::TarGz));
        assert_eq!(format_of(Path::new("x.tgz")), Some(Format::TarGz));
        assert_eq!(format_of(Path::new("x.gz")), None);
        assert_eq!(format_of(Path::new("x.rar")), None);
        assert!(is_mac_junk("__MACOSX/Urlaub/._a.jpg"));
        assert!(is_mac_junk("Urlaub/._a.jpg"));
        assert!(is_mac_junk("Urlaub/.DS_Store"));
        assert!(!is_mac_junk("Urlaub/a.jpg"));
    }
}
