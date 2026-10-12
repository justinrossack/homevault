//! Eingangs-Ordner: Was in `Gemeinsam/Eingang` oder `Privat/<name>/Eingang`
//! landet (Browser, Netzlaufwerk, Scanner), wird automatisch einsortiert.
//!
//! - Fotos und Videos: nach Aufnahmedatum und Ort, ohne KI
//!   → `Fotos/2026/2026-07 Lissabon/IMG_1234.HEIC` (Live-Video kommt mit)
//! - Dokumente: Text lesen (bei Scans per Texterkennung), eine lokale KI
//!   (Ollama) schlägt Datum, Absender, Titel und Ordner vor
//!   → `Dokumente/Rechnungen/2026-03-12 Stadtwerke Stromrechnung.pdf`
//!
//! Alles läuft nacheinander im Hintergrund. Jede Aktion steht im Verlauf und
//! lässt sich rückgängig machen; zurückgelegte Dateien kommen nach
//! `Eingang/Zurückgelegt` und werden nicht noch einmal sortiert.

use std::{
    collections::HashMap,
    io::{BufRead, BufReader, Read},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant, SystemTime},
};

use axum::{
    Extension, Json,
    extract::{Path as UrlPath, State},
};
use chrono::{DateTime, Datelike, Local, NaiveDate, NaiveDateTime};
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Value, json};
use tokio::sync::Notify;

use crate::{
    AppState,
    auth::CurrentUser,
    db::{self, Db},
    error::AppError,
    fs_api::{self, Kind},
    index, library, paths, upload,
};

pub const INBOX: &str = "Eingang";
/// hierher legt „Rückgängig“ – wird nicht noch einmal sortiert
const PUT_BACK: &str = "Zurückgelegt";
const PHOTOS: &str = "Fotos";
const DOCUMENTS: &str = "Dokumente";
/// Dateien erst anfassen, wenn sie so lange unverändert sind (Scanner, Netzlaufwerk)
const STABLE: Duration = Duration::from_secs(20);
/// Ordner, die die KI immer zur Auswahl hat (dazu kommen vorhandene)
const FOLDERS: &[&str] = &[
    "Rechnungen", "Verträge", "Versicherungen", "Bank", "Steuer", "Gesundheit", "Behörden", "Arbeit", "Schule", "Auto",
    "Wohnen", "Kaufbelege", "Briefe", "Sonstiges",
];
const DOC_EXT: &[&str] = &[
    "pdf", "doc", "docx", "odt", "rtf", "txt", "md", "xls", "xlsx", "ods", "csv", "ppt", "pptx", "odp",
];

pub fn enabled(c: &Connection) -> bool {
    db::get_setting(c, "inbox_enabled").ok().flatten().as_deref() != Some("0")
}

// ---------- KI (Ollama) ----------

#[derive(Clone, serde::Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum AiState {
    Unknown,
    Unreachable { error: String },
    /// Modell wird heruntergeladen
    Pulling { done: u64, total: u64 },
    Ready,
}

pub struct Ai {
    url: String,
    model: String,
    state: Mutex<AiState>,
    http: ureq::Agent,
}

impl Ai {
    fn from_env() -> Option<Arc<Self>> {
        let url = std::env::var("AI_URL").ok().filter(|u| !u.is_empty())?;
        let model = std::env::var("AI_MODEL").ok().filter(|m| !m.is_empty()).unwrap_or_else(|| "qwen2.5:3b".into());
        let http = ureq::Agent::config_builder()
            .http_status_as_error(false)
            // ein Dokument kann auf schwacher Hardware ein paar Minuten dauern
            .timeout_global(Some(Duration::from_secs(900)))
            .build()
            .into();
        Some(Arc::new(Self { url: url.trim_end_matches('/').to_owned(), model, state: Mutex::new(AiState::Unknown), http }))
    }

    fn ready(&self) -> bool {
        matches!(*self.state.lock().unwrap(), AiState::Ready)
    }

    /// Ist das Modell da? Sonst herunterladen (blockiert, mit Fortschritt).
    fn ensure_model(&self) {
        if matches!(*self.state.lock().unwrap(), AiState::Ready | AiState::Pulling { .. }) {
            return;
        }
        let tags = self.http.get(&format!("{}/api/tags", self.url)).call();
        let mut res = match tags {
            Ok(r) if r.status() == 200 => r,
            Ok(r) => return self.set(AiState::Unreachable { error: format!("KI-Dienst antwortet mit {}", r.status()) }),
            Err(e) => return self.set(AiState::Unreachable { error: format!("KI-Dienst nicht erreichbar: {e}") }),
        };
        let v: Value = res.body_mut().read_json().unwrap_or(Value::Null);
        let have = v["models"].as_array().into_iter().flatten().any(|m| m["name"].as_str() == Some(&self.model));
        if have {
            return self.set(AiState::Ready);
        }
        tracing::info!("KI-Modell {} wird heruntergeladen …", self.model);
        self.set(AiState::Pulling { done: 0, total: 0 });
        let res = self.http.post(&format!("{}/api/pull", self.url)).send_json(json!({ "model": self.model, "stream": true }));
        let mut res = match res {
            Ok(r) if r.status() == 200 => r,
            Ok(r) => return self.set(AiState::Unreachable { error: format!("Modell-Download: {}", r.status()) }),
            Err(e) => return self.set(AiState::Unreachable { error: format!("Modell-Download: {e}") }),
        };
        for line in BufReader::new(res.body_mut().as_reader()).lines() {
            let Ok(line) = line else { break };
            let Ok(v) = serde_json::from_str::<Value>(&line) else { continue };
            if let Some(e) = v["error"].as_str() {
                return self.set(AiState::Unreachable { error: format!("Modell-Download: {e}") });
            }
            if let (Some(done), Some(total)) = (v["completed"].as_u64(), v["total"].as_u64()) {
                self.set(AiState::Pulling { done, total });
            }
            if v["status"] == "success" {
                tracing::info!("KI-Modell {} ist bereit", self.model);
                return self.set(AiState::Ready);
            }
        }
        self.set(AiState::Unreachable { error: "Modell-Download abgebrochen".into() });
    }

    fn set(&self, s: AiState) {
        *self.state.lock().unwrap() = s;
    }

    /// Fragt das Modell; Antwort ist garantiert JSON nach `schema`.
    fn ask(&self, prompt: &str, schema: Value) -> Result<Value, String> {
        let body = json!({
            "model": self.model,
            "prompt": prompt,
            "stream": false,
            "format": schema,
            "keep_alive": "10m",
            "options": { "temperature": 0, "num_ctx": 4096, "num_predict": 200 },
        });
        let mut res = self.http.post(&format!("{}/api/generate", self.url)).send_json(body).map_err(|e| {
            self.set(AiState::Unknown);
            format!("KI-Dienst nicht erreichbar: {e}")
        })?;
        let v: Value = res.body_mut().read_json().map_err(|e| e.to_string())?;
        if let Some(e) = v["error"].as_str() {
            self.set(AiState::Unknown);
            return Err(format!("KI: {e}"));
        }
        serde_json::from_str(v["response"].as_str().unwrap_or("")).map_err(|e| format!("KI-Antwort unlesbar: {e}"))
    }
}

// ---------- Dienst ----------

struct Seen {
    size: u64,
    mtime: SystemTime,
    /// nicht unterstützt oder fehlgeschlagen: erst wieder anfassen, wenn sich die Datei ändert
    skip: bool,
}

pub struct Inbox {
    db: Arc<Db>,
    data_dir: PathBuf,
    maintenance: Arc<AtomicBool>,
    pub ai: Option<Arc<Ai>>,
    seen: Mutex<HashMap<PathBuf, Seen>>,
    /// gerade in Arbeit (für die Anzeige)
    current: Mutex<Option<String>>,
    wake: Notify,
}

impl Inbox {
    pub fn new(db: Arc<Db>, data_dir: PathBuf, maintenance: Arc<AtomicBool>) -> Arc<Self> {
        Arc::new(Self {
            db,
            data_dir,
            maintenance,
            ai: Ai::from_env(),
            seen: Mutex::default(),
            current: Mutex::default(),
            wake: Notify::new(),
        })
    }

    pub fn wake(&self) {
        self.wake.notify_one();
    }

    /// Startet den Hintergrund-Ablauf (braucht den fertigen AppState für
    /// die Office-Umwandlung).
    pub fn start(self: &Arc<Self>, state: AppState, watcher: &crate::watch::Watcher) {
        let weak = Arc::downgrade(self);
        watcher.subscribe(move |rel| {
            if inbox_area(rel).is_some() {
                if let Some(me) = weak.upgrade() {
                    me.wake();
                }
            }
        });
        if let Some(ai) = self.ai.clone() {
            // Modell im Hintergrund prüfen/laden, bei Fehlern regelmäßig erneut
            std::thread::spawn(move || {
                loop {
                    ai.ensure_model();
                    std::thread::sleep(Duration::from_secs(if ai.ready() { 600 } else { 60 }));
                }
            });
        }
        let me = self.clone();
        tokio::spawn(async move {
            loop {
                if !me.maintenance.load(Ordering::SeqCst) {
                    let on = me.db.call(|c| Ok(enabled(c))).await.unwrap_or(false);
                    if on {
                        // auch für neue Benutzer (kostet nichts, wenn es sie schon gibt)
                        me.ensure_dirs().await;
                        me.work(&state).await;
                    }
                }
                // spätestens alle 30 s nachsehen (falls eine Änderung nicht gemeldet wurde)
                let _ = tokio::time::timeout(Duration::from_secs(30), me.wake.notified()).await;
                // Dateien brauchen ohnehin ein paar Sekunden Ruhe
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
        });
    }

    /// Eingangs-Ordner für Gemeinsam und jeden Benutzer anlegen.
    async fn ensure_dirs(&self) {
        let users: Vec<String> = self
            .db
            .call(|c| Ok(c.prepare("SELECT name FROM users")?.query_map([], |r| r.get(0))?.collect::<Result<_, _>>()?))
            .await
            .unwrap_or_default();
        let mut dirs = vec![self.data_dir.join(paths::SHARED).join(INBOX)];
        dirs.extend(users.iter().map(|u| self.data_dir.join(paths::PRIVATE).join(u).join(INBOX)));
        for d in dirs {
            let _ = tokio::fs::create_dir_all(d).await;
        }
    }

    /// Alle Eingänge durchgehen und fertige Dateien einsortieren.
    async fn work(self: &Arc<Self>, state: &AppState) {
        let files = self.waiting_files();
        let mut changed = false;
        for (rel, abs) in files {
            let Ok(meta) = std::fs::metadata(&abs) else { continue };
            let mtime = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
            // erst, wenn die Datei seit dem letzten Blick gleich geblieben ist
            let stable = {
                let mut seen = self.seen.lock().unwrap();
                match seen.get(&abs) {
                    Some(s) if s.size == meta.len() && s.mtime == mtime => {
                        !s.skip && mtime.elapsed().unwrap_or_default() >= STABLE
                    }
                    _ => {
                        seen.insert(abs.clone(), Seen { size: meta.len(), mtime, skip: false });
                        false
                    }
                }
            };
            if !stable {
                continue;
            }
            *self.current.lock().unwrap() = Some(rel.clone());
            let outcome = self.process(state, &rel, &abs).await;
            *self.current.lock().unwrap() = None;
            match outcome {
                Outcome::Moved => {
                    self.seen.lock().unwrap().remove(&abs);
                    changed = true;
                }
                // KI noch nicht bereit: liegen lassen, später erneut
                Outcome::Later => {}
                Outcome::Skip => {
                    if let Some(s) = self.seen.lock().unwrap().get_mut(&abs) {
                        s.skip = true;
                    }
                }
            }
        }
        if changed {
            state.index.request_scan();
        }
        // Vergessene Einträge (Datei weg) aufräumen
        self.seen.lock().unwrap().retain(|p, _| p.exists());
    }

    /// (relativer Pfad, absoluter Pfad) aller Dateien direkt in einem Eingang
    fn waiting_files(&self) -> Vec<(String, PathBuf)> {
        let mut dirs = vec![format!("{}/{INBOX}", paths::SHARED)];
        if let Ok(rd) = std::fs::read_dir(self.data_dir.join(paths::PRIVATE)) {
            for e in rd.flatten() {
                let name = e.file_name().to_string_lossy().into_owned();
                if !name.starts_with('.') {
                    dirs.push(format!("{}/{name}/{INBOX}", paths::PRIVATE));
                }
            }
        }
        let mut out = Vec::new();
        for d in dirs {
            let Ok(rd) = std::fs::read_dir(self.data_dir.join(&d)) else { continue };
            for e in rd.flatten() {
                let name = e.file_name().to_string_lossy().into_owned();
                if name.starts_with('.') || !e.file_type().is_ok_and(|t| t.is_file()) {
                    continue;
                }
                out.push((format!("{d}/{name}"), e.path()));
            }
        }
        out.sort();
        out
    }

    async fn process(&self, state: &AppState, rel: &str, abs: &Path) -> Outcome {
        let Some(area) = inbox_area(rel) else { return Outcome::Skip };
        let name = abs.file_name().unwrap_or_default().to_string_lossy().into_owned();
        let result = match fs_api::kind_of(abs) {
            Kind::Image | Kind::Video => {
                // Live-Video wartet auf sein Foto (wird mit ihm verschoben)
                if fs_api::is_live_motion(abs) && has_still(abs) {
                    return Outcome::Later;
                }
                self.sort_photo(&area, abs).await
            }
            _ if is_document(abs) => {
                let Some(ai) = self.ai.clone().filter(|a| a.ready()) else { return Outcome::Later };
                self.sort_document(state, &ai, &area, abs).await
            }
            _ => {
                self.log(&area, &name, None, None, "other", "skipped", Some("Dateityp wird nicht automatisch sortiert".into())).await;
                return Outcome::Skip;
            }
        };
        match result {
            Ok((kind, new_rel, live_rel)) => {
                tracing::info!("Eingang: {rel} → {new_rel}");
                self.log(&area, &name, Some(new_rel), live_rel, kind, "done", None).await;
                Outcome::Moved
            }
            Err(Retry(e)) => {
                tracing::warn!("Eingang: {rel}: {e} (später erneut)");
                Outcome::Later
            }
            Err(Fail(e)) => {
                tracing::warn!("Eingang: {rel}: {e}");
                let kind = if is_document(abs) { "document" } else { "photo" };
                self.log(&area, &name, None, None, kind, "error", Some(e)).await;
                Outcome::Skip
            }
        }
    }

    // ---------- Fotos ----------

    async fn sort_photo(&self, area: &str, abs: &Path) -> Result<(&'static str, String, Option<String>), Error> {
        let src = abs.to_path_buf();
        let (taken, place) = tokio::task::spawn_blocking(move || photo_info(&src)).await.map_err(|e| Fail(e.to_string()))?;
        let month = taken.format("%Y-%m").to_string();
        let folder = match place {
            Some(p) => format!("{month} {}", clean_name(p)),
            None => month,
        };
        let dir = format!("{area}/{PHOTOS}/{}/{folder}", taken.year());
        let companion = if fs_api::is_live_still(abs) { fs_api::live_companion(abs).await } else { None };
        let (new_rel, live_rel) = self.move_into(abs, &dir, None, companion.as_deref()).await?;
        Ok(("photo", new_rel, live_rel))
    }

    // ---------- Dokumente ----------

    async fn sort_document(&self, state: &AppState, ai: &Arc<Ai>, area: &str, abs: &Path) -> Result<(&'static str, String, Option<String>), Error> {
        let mut text = {
            let src = abs.to_path_buf();
            tokio::task::spawn_blocking(move || extract_text(&src)).await.map_err(|e| Fail(e.to_string()))?
        };
        // alte Office-Formate über die PDF-Vorschau lesen
        if text.trim().is_empty() && matches!(ext(abs).as_str(), "doc" | "xls" | "ppt" | "rtf") {
            if let Ok(Some(pdf)) = crate::docs::office_pdf(state, abs).await {
                text = tokio::task::spawn_blocking(move || pdf_text(&pdf)).await.unwrap_or_default();
            }
        }
        let name = abs.file_name().unwrap_or_default().to_string_lossy().into_owned();
        let docs_dir = self.data_dir.join(area).join(DOCUMENTS);
        if text.trim().chars().count() < 20 {
            // nichts Lesbares (leerer Scan, Bild ohne Text): nicht raten
            let dir = format!("{area}/{DOCUMENTS}/Unsortiert");
            let (new_rel, _) = self.move_into(abs, &dir, None, None).await?;
            return Ok(("document", new_rel, None));
        }
        let folders = folder_choices(&docs_dir);
        let prompt = document_prompt(&name, &text, &folders);
        let schema = json!({
            "type": "object",
            "properties": {
                "datum": { "type": "string" },
                "absender": { "type": "string" },
                "titel": { "type": "string" },
                "ordner": { "type": "string", "enum": folders },
            },
            "required": ["datum", "absender", "titel", "ordner"],
        });
        let ai = ai.clone();
        let answer = tokio::task::spawn_blocking(move || ai.ask(&prompt, schema))
            .await
            .map_err(|e| Retry(e.to_string()))?
            .map_err(Retry)?;
        let (new_name, folder) = document_name(&answer, abs, &folders);
        let dir = format!("{area}/{DOCUMENTS}/{folder}");
        let (new_rel, _) = self.move_into(abs, &dir, Some(&new_name), None).await?;
        Ok(("document", new_rel, None))
    }

    /// Verschiebt (mit Live-Video) in `dir`, ohne etwas zu überschreiben, und
    /// zieht den Index nach. Gibt die neuen relativen Pfade zurück.
    async fn move_into(&self, abs: &Path, dir: &str, name: Option<&str>, companion: Option<&Path>) -> Result<(String, Option<String>), Error> {
        let target_dir = self.data_dir.join(dir);
        tokio::fs::create_dir_all(&target_dir).await.map_err(|e| Fail(e.to_string()))?;
        let name = name.map(str::to_owned).unwrap_or_else(|| abs.file_name().unwrap_or_default().to_string_lossy().into_owned());
        let new = upload::move_to_free_name(abs, &target_dir, &name).await.map_err(|e| Fail(e.to_string()))?;
        let mut moves = vec![(paths::relative(&self.data_dir, abs), paths::relative(&self.data_dir, &new))];
        let mut live_rel = None;
        if let Some(c) = companion {
            let ext = c.extension().unwrap_or_default().to_string_lossy();
            let stem = new.file_stem().unwrap_or_default().to_string_lossy();
            if let Ok(nc) = upload::move_to_free_name(c, &target_dir, &format!("{stem}.{ext}")).await {
                let r = paths::relative(&self.data_dir, &nc);
                moves.push((paths::relative(&self.data_dir, c), r.clone()));
                live_rel = Some(r);
            }
        }
        let new_rel = moves[0].1.clone();
        let _ = self
            .db
            .call(move |c| {
                for (a, b) in &moves {
                    index::rename_paths(c, a, b)?;
                }
                Ok(())
            })
            .await;
        Ok((new_rel, live_rel))
    }

    #[allow(clippy::too_many_arguments)]
    async fn log(&self, area: &str, original: &str, new: Option<String>, live: Option<String>, kind: &str, status: &str, msg: Option<String>) {
        let (area, original, kind, status) = (area.to_owned(), original.to_owned(), kind.to_owned(), status.to_owned());
        let _ = self
            .db
            .call(move |c| {
                c.execute(
                    "INSERT INTO inbox_log (area, original_name, new_path, live_path, kind, status, message, created_at)
                     VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
                    params![area, original, new, live, kind, status, msg, db::now()],
                )?;
                // Verlauf begrenzen
                c.execute("DELETE FROM inbox_log WHERE id NOT IN (SELECT id FROM inbox_log ORDER BY id DESC LIMIT 2000)", [])?;
                Ok(())
            })
            .await;
    }
}

enum Outcome {
    Moved,
    Later,
    Skip,
}

enum Error {
    /// vorübergehend (KI beschäftigt/aus): Datei liegen lassen
    Retry(String),
    /// endgültig: im Verlauf melden, Datei liegen lassen
    Fail(String),
}
use Error::{Fail, Retry};

/// "Gemeinsam/Eingang/x.pdf" -> "Gemeinsam", "Privat/anna/Eingang/x" -> "Privat/anna"
/// (nur Dateien direkt im Eingang, nicht in Unterordnern wie „Zurückgelegt“)
pub fn inbox_area(rel: &str) -> Option<String> {
    let parts: Vec<&str> = rel.split('/').collect();
    match parts.as_slice() {
        [s, i, _] if *s == paths::SHARED && *i == INBOX => Some(paths::SHARED.to_owned()),
        [p, u, i, _] if *p == paths::PRIVATE && *i == INBOX => Some(format!("{p}/{u}")),
        _ => None,
    }
}

fn ext(p: &Path) -> String {
    p.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default()
}

fn is_document(p: &Path) -> bool {
    DOC_EXT.contains(&ext(p).as_str())
}

/// Gibt es zum Live-Video ein Foto mit gleichem Namen daneben?
fn has_still(mov: &Path) -> bool {
    let stem = fs_api::stem_lower(mov);
    std::fs::read_dir(mov.parent().unwrap_or(Path::new(".")))
        .into_iter()
        .flatten()
        .flatten()
        .any(|e| fs_api::is_live_still(&e.path()) && fs_api::stem_lower(&e.path()) == stem)
}

/// Aufnahmezeitpunkt (Ortszeit der Kamera) und Ort eines Fotos/Videos.
fn photo_info(p: &Path) -> (NaiveDateTime, Option<&'static str>) {
    let kind = if fs_api::kind_of(p) == Kind::Video { "video" } else { "image" };
    let taken = index::read_meta(p, kind).taken.and_then(|t| DateTime::from_timestamp(t, 0)).map(|d| d.naive_utc());
    // ohne Aufnahmedatum: Änderungsdatum der Datei
    let taken = taken.unwrap_or_else(|| {
        let m = std::fs::metadata(p).and_then(|m| m.modified()).unwrap_or(SystemTime::now());
        DateTime::<Local>::from(m).naive_local()
    });
    let gps = if kind == "video" { video_gps(p) } else { exif_gps(p) };
    (taken, gps.and_then(|(lat, lon)| crate::geo::place_name(lat, lon)))
}

fn exif_gps(p: &Path) -> Option<(f64, f64)> {
    let mut r = BufReader::new(std::fs::File::open(p).ok()?);
    let ex = exif::Reader::new().read_from_container(&mut r).ok()?;
    let coord = |tag, reftag| -> Option<f64> {
        let exif::Value::Rational(ref v) = ex.get_field(tag, exif::In::PRIMARY)?.value else { return None };
        let deg = v.first()?.to_f64() + v.get(1).map_or(0.0, |m| m.to_f64()) / 60.0 + v.get(2).map_or(0.0, |s| s.to_f64()) / 3600.0;
        let r = ex.get_field(reftag, exif::In::PRIMARY).map(|f| f.display_value().to_string()).unwrap_or_default();
        Some(if r.starts_with('S') || r.starts_with('W') { -deg } else { deg })
    };
    Some((coord(exif::Tag::GPSLatitude, exif::Tag::GPSLatitudeRef)?, coord(exif::Tag::GPSLongitude, exif::Tag::GPSLongitudeRef)?))
}

/// iPhone/Android speichern den Ort im Video als ISO 6709, z. B. "+38.7223-009.1393+010.000/"
fn video_gps(p: &Path) -> Option<(f64, f64)> {
    let out = Command::new("ffprobe")
        .args(["-v", "error", "-show_entries", "format_tags=location,com.apple.quicktime.location.ISO6709", "-of", "json"])
        .arg(p)
        .stderr(Stdio::null())
        .output()
        .ok()?;
    let v: Value = serde_json::from_slice(&out.stdout).ok()?;
    let tags = &v["format"]["tags"];
    let s = tags["com.apple.quicktime.location.ISO6709"].as_str().or_else(|| tags["location"].as_str())?;
    parse_iso6709(s)
}

fn parse_iso6709(s: &str) -> Option<(f64, f64)> {
    let starts: Vec<usize> = s.char_indices().filter(|(_, c)| *c == '+' || *c == '-').map(|(i, _)| i).collect();
    let lat = s.get(*starts.first()?..*starts.get(1)?)?.parse().ok()?;
    let end = starts.get(2).copied().unwrap_or_else(|| s.find('/').unwrap_or(s.len()));
    let lon = s.get(*starts.get(1)?..end)?.parse().ok()?;
    Some((lat, lon))
}

// ---------- Text aus Dokumenten ----------

/// Bis zu ~3000 Zeichen Text (mehr braucht die KI nicht).
fn extract_text(p: &Path) -> String {
    let text = match ext(p).as_str() {
        "pdf" => {
            let t = pdf_text(p);
            if t.trim().chars().count() >= 40 { t } else { ocr_pdf(p) }
        }
        "docx" => zip_xml(p, &["word/document.xml"]),
        "odt" | "ods" | "odp" => zip_xml(p, &["content.xml"]),
        "xlsx" => zip_xml(p, &["xl/sharedStrings.xml"]),
        "pptx" => zip_xml(p, &["ppt/slides/slide1.xml", "ppt/slides/slide2.xml", "ppt/slides/slide3.xml"]),
        "txt" | "md" | "csv" => {
            let mut buf = Vec::new();
            let _ = std::fs::File::open(p).and_then(|f| f.take(8192).read_to_end(&mut buf));
            String::from_utf8_lossy(&buf).into_owned()
        }
        _ => String::new(),
    };
    let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
    collapsed.chars().take(3000).collect()
}

fn pdf_text(p: &Path) -> String {
    run_capture("pdftotext", &["-l".as_ref(), "3".as_ref(), "-q".as_ref(), p.as_os_str(), "-".as_ref()], 60).unwrap_or_default()
}

/// Gescannte PDFs: erste zwei Seiten in Bilder umwandeln und Text erkennen.
fn ocr_pdf(p: &Path) -> String {
    let dir = std::env::temp_dir().join(format!("hv-ocr-{}", rand::random::<u64>()));
    if std::fs::create_dir_all(&dir).is_err() {
        return String::new();
    }
    let prefix = dir.join("seite");
    let _ = run_capture(
        "pdftoppm",
        &["-r".as_ref(), "200".as_ref(), "-l".as_ref(), "2".as_ref(), "-gray".as_ref(), "-png".as_ref(), p.as_os_str(), prefix.as_os_str()],
        120,
    );
    let mut pages: Vec<PathBuf> = std::fs::read_dir(&dir).into_iter().flatten().flatten().map(|e| e.path()).collect();
    pages.sort();
    let langs = ocr_langs();
    let text = pages
        .iter()
        .filter_map(|img| run_capture("tesseract", &[img.as_os_str(), "stdout".as_ref(), "-l".as_ref(), langs.as_ref()], 180))
        .collect::<Vec<_>>()
        .join("\n");
    let _ = std::fs::remove_dir_all(&dir);
    text
}

/// Installierte Sprachen für die Texterkennung, z. B. "deu+eng"
fn ocr_langs() -> &'static str {
    static LANGS: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    LANGS.get_or_init(|| {
        let list = run_capture("tesseract", &["--list-langs".as_ref()], 10).unwrap_or_default();
        let have: Vec<&str> = ["deu", "eng"].into_iter().filter(|l| list.lines().any(|x| x.trim() == *l)).collect();
        if have.is_empty() { "eng".into() } else { have.join("+") }
    })
}

/// Text aus den XML-Dateien eines Office-Dokuments (docx, odt, xlsx …)
fn zip_xml(p: &Path, entries: &[&str]) -> String {
    let Ok(f) = std::fs::File::open(p) else { return String::new() };
    let Ok(mut z) = zip::ZipArchive::new(f) else { return String::new() };
    let mut out = String::new();
    for name in entries {
        let Ok(mut e) = z.by_name(name) else { continue };
        let mut xml = String::new();
        if e.by_ref().take(4 << 20).read_to_string(&mut xml).is_err() {
            continue;
        }
        let mut in_tag = false;
        for c in xml.chars() {
            match c {
                '<' => in_tag = true,
                '>' => {
                    in_tag = false;
                    out.push(' ');
                }
                _ if !in_tag => out.push(c),
                _ => {}
            }
        }
        if out.len() > 20_000 {
            break;
        }
    }
    out.replace("&amp;", "&").replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'")
}

/// Programm mit Zeitlimit ausführen, stdout als Text.
fn run_capture(cmd: &str, args: &[&std::ffi::OsStr], secs: u64) -> Option<String> {
    let mut child = Command::new(cmd).args(args).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().ok()?;
    let mut stdout = child.stdout.take()?;
    let reader = std::thread::spawn(move || {
        let mut s = Vec::new();
        let _ = stdout.read_to_end(&mut s);
        s
    });
    let deadline = Instant::now() + Duration::from_secs(secs);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(100)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break;
            }
        }
    }
    reader.join().ok().map(|b| String::from_utf8_lossy(&b).into_owned())
}

// ---------- Name und Ordner ----------

/// Feste Ordner plus die, die es in Dokumente/ schon gibt
fn folder_choices(docs_dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = FOLDERS.iter().map(|s| s.to_string()).collect();
    if let Ok(rd) = std::fs::read_dir(docs_dir) {
        for e in rd.flatten() {
            let n = e.file_name().to_string_lossy().into_owned();
            if e.file_type().is_ok_and(|t| t.is_dir()) && paths::valid_name(&n) && n != "Unsortiert" && !v.iter().any(|x| x.eq_ignore_ascii_case(&n)) {
                v.push(n);
            }
        }
    }
    v
}

fn document_prompt(name: &str, text: &str, folders: &[String]) -> String {
    format!(
        "Du ordnest Dokumente für eine Familie. Lies den Text und antworte nur mit JSON.\n\n\
         Felder:\n\
         - datum: Datum des Dokuments (Brief-, Rechnungs- oder Vertragsdatum) als JJJJ-MM-TT, \"\" wenn keins erkennbar\n\
         - absender: wer es geschickt oder erstellt hat, kurz (z. B. \"Stadtwerke München\", \"Finanzamt\", \"AOK\"), \"\" wenn unklar\n\
         - titel: worum es geht, 2 bis 5 Wörter (z. B. \"Stromrechnung März\", \"Kündigung Handyvertrag\")\n\
         - ordner: genau einer aus: {}\n\n\
         Dateiname: {name}\n\
         Text:\n\"\"\"\n{text}\n\"\"\"",
        folders.join(", ")
    )
}

/// Aus der KI-Antwort: neuer Dateiname und Ordner (abgesichert).
fn document_name(answer: &Value, src: &Path, folders: &[String]) -> (String, String) {
    let field = |k: &str| clean_name(answer[k].as_str().unwrap_or(""));
    let date = answer["datum"]
        .as_str()
        .and_then(|d| NaiveDate::parse_from_str(d.trim(), "%Y-%m-%d").ok())
        .filter(|d| (1950..=Local::now().year() + 1).contains(&d.year()))
        .map(|d| d.format("%Y-%m-%d").to_string());
    let mut parts: Vec<String> = date.into_iter().collect();
    let (sender, title) = (field("absender"), field("titel"));
    if !sender.is_empty() {
        parts.push(sender.clone());
    }
    // Titel nicht doppeln, wenn er mit dem Absender beginnt
    let repeats_sender = !sender.is_empty() && title.to_lowercase().starts_with(&sender.to_lowercase());
    if !title.is_empty() && !repeats_sender {
        parts.push(title);
    }
    let mut stem = parts.into_iter().filter(|p| !p.is_empty()).collect::<Vec<_>>().join(" ");
    if stem.chars().count() > 100 {
        stem = stem.chars().take(100).collect::<String>().trim_end().to_owned();
    }
    if stem.is_empty() || !paths::valid_name(&stem) {
        stem = src.file_stem().unwrap_or_default().to_string_lossy().into_owned();
    }
    let e = ext(src);
    let name = if e.is_empty() { stem } else { format!("{stem}.{e}") };
    let wanted = answer["ordner"].as_str().unwrap_or("");
    let folder = folders.iter().find(|f| f.eq_ignore_ascii_case(wanted)).cloned().unwrap_or_else(|| "Sonstiges".into());
    (name, folder)
}

/// Für Datei- und Ordnernamen ungeeignete Zeichen entfernen.
fn clean_name(s: &str) -> String {
    let s: String = s.chars().map(|c| if c.is_control() || "/\\:*?\"<>|".contains(c) { ' ' } else { c }).collect();
    s.split_whitespace().collect::<Vec<_>>().join(" ").trim_matches(['.', ' ']).to_owned()
}

// ---------- HTTP ----------

/// Bereiche, die der Benutzer sieht
fn visible_areas(me: &CurrentUser) -> [String; 2] {
    [paths::SHARED.to_owned(), format!("{}/{}", paths::PRIVATE, me.name)]
}

pub async fn status(State(state): State<AppState>, Extension(me): Extension<CurrentUser>) -> Result<Json<Value>, AppError> {
    let inbox = state.inbox.clone();
    let [shared, private] = visible_areas(&me);
    let my_name = me.name.clone();
    let (on, items) = state
        .db
        .call(move |c| {
            let items: Vec<Value> = c
                .prepare(
                    "SELECT id, area, original_name, new_path, kind, status, message, created_at FROM inbox_log
                     WHERE area IN (?1, ?2) ORDER BY id DESC LIMIT 100",
                )?
                .query_map(params![shared, private], |r| {
                    let new: Option<String> = r.get(3)?;
                    Ok(json!({
                        "id": r.get::<_, i64>(0)?,
                        "area": r.get::<_, String>(1)?,
                        "original_name": r.get::<_, String>(2)?,
                        "new_path": new.and_then(|p| library::to_virtual(&my_name, &p)),
                        "kind": r.get::<_, String>(4)?,
                        "status": r.get::<_, String>(5)?,
                        "message": r.get::<_, Option<String>>(6)?,
                        "created_at": r.get::<_, i64>(7)?,
                    }))
                })?
                .collect::<Result<_, _>>()?;
            Ok((enabled(c), items))
        })
        .await?;
    let [shared, private] = visible_areas(&me);
    let waiting: Vec<Value> = inbox
        .waiting_files()
        .into_iter()
        .filter_map(|(rel, abs)| {
            let area = inbox_area(&rel)?;
            (area == shared || area == private).then(|| {
                json!({
                    "name": abs.file_name().unwrap_or_default().to_string_lossy(),
                    "area": area,
                    "document": is_document(&abs),
                })
            })
        })
        .collect();
    let ai = inbox.ai.as_ref().map(|a| json!({ "model": a.model, "status": *a.state.lock().unwrap() }));
    let current = inbox.current.lock().unwrap().clone();
    Ok(Json(json!({ "enabled": on, "ai": ai, "waiting": waiting, "current": current, "items": items })))
}

/// Einsortierte Datei zurück in den Eingang (Unterordner „Zurückgelegt“).
pub async fn undo(State(state): State<AppState>, Extension(me): Extension<CurrentUser>, UrlPath(id): UrlPath<i64>) -> Result<Json<Value>, AppError> {
    let row: Option<(String, String, Option<String>, Option<String>, String)> = state
        .db
        .call(move |c| {
            Ok(c.query_row("SELECT area, original_name, new_path, live_path, status FROM inbox_log WHERE id = ?", [id], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
            })
            .optional()?)
        })
        .await?;
    let Some((area, original, Some(new_path), live, status)) = row else { return Err(AppError::NotFound) };
    if !visible_areas(&me).contains(&area) {
        return Err(AppError::NotFound);
    }
    if status != "done" {
        return Err(AppError::BadRequest("Schon rückgängig gemacht".into()));
    }
    let from = state.data_dir.join(&new_path);
    if !from.exists() {
        return Err(AppError::BadRequest("Die Datei liegt nicht mehr dort (verschoben oder gelöscht)".into()));
    }
    let dir = state.data_dir.join(&area).join(INBOX).join(PUT_BACK);
    tokio::fs::create_dir_all(&dir).await?;
    let back = upload::move_to_free_name(&from, &dir, &original).await?;
    let mut moves = vec![(new_path, paths::relative(&state.data_dir, &back))];
    if let Some(l) = live.filter(|l| state.data_dir.join(l).exists()) {
        let ext = Path::new(&l).extension().unwrap_or_default().to_string_lossy().into_owned();
        let stem = back.file_stem().unwrap_or_default().to_string_lossy().into_owned();
        if let Ok(b) = upload::move_to_free_name(&state.data_dir.join(&l), &dir, &format!("{stem}.{ext}")).await {
            moves.push((l, paths::relative(&state.data_dir, &b)));
        }
    }
    let back_rel = moves[0].1.clone();
    state
        .db
        .call(move |c| {
            for (a, b) in &moves {
                index::rename_paths(c, a, b)?;
            }
            c.execute("UPDATE inbox_log SET status = 'undone', new_path = ? WHERE id = ?", params![back_rel, id])?;
            Ok(())
        })
        .await?;
    state.index.request_scan();
    Ok(Json(json!({ "ok": true })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inbox_area_only_direct_children() {
        assert_eq!(inbox_area("Gemeinsam/Eingang/scan.pdf").as_deref(), Some("Gemeinsam"));
        assert_eq!(inbox_area("Privat/anna/Eingang/IMG_1.HEIC").as_deref(), Some("Privat/anna"));
        assert_eq!(inbox_area("Privat/anna/Eingang/Zurückgelegt/x.pdf"), None);
        assert_eq!(inbox_area("Gemeinsam/Fotos/x.jpg"), None);
    }

    #[test]
    fn iso6709_location() {
        assert_eq!(parse_iso6709("+38.7223-009.1393+010.000/"), Some((38.7223, -9.1393)));
        assert_eq!(parse_iso6709("+48.1372+011.5756/"), Some((48.1372, 11.5756)));
        assert_eq!(parse_iso6709("kaputt"), None);
    }

    #[test]
    fn document_names_are_safe() {
        let folders: Vec<String> = FOLDERS.iter().map(|s| s.to_string()).collect();
        let a = json!({ "datum": "2026-03-12", "absender": "Stadtwerke München", "titel": "Stromrechnung März", "ordner": "rechnungen" });
        assert_eq!(
            document_name(&a, Path::new("/x/scan_0042.PDF"), &folders),
            ("2026-03-12 Stadtwerke München Stromrechnung März.pdf".into(), "Rechnungen".into())
        );
        // Unsinn von der KI: kein Pfad-Ausbruch, unbekannter Ordner -> Sonstiges, falsches Datum weg
        let b = json!({ "datum": "gestern", "absender": "../../etc", "titel": "a/b:c", "ordner": "Geheim" });
        let (name, folder) = document_name(&b, Path::new("/x/brief.pdf"), &folders);
        assert!(!name.contains('/') && !name.starts_with('.'), "{name}");
        assert_eq!(folder, "Sonstiges");
        // leere Antwort -> alter Name
        let c = json!({ "datum": "", "absender": "", "titel": "", "ordner": "Briefe" });
        assert_eq!(document_name(&c, Path::new("/x/brief.pdf"), &folders).0, "brief.pdf");
    }

    #[test]
    fn office_text_from_docx() {
        let dir = std::env::temp_dir().join(format!("hv-docx-{}", rand::random::<u32>()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("t.docx");
        let mut z = zip::ZipWriter::new(std::fs::File::create(&p).unwrap());
        z.start_file("word/document.xml", zip::write::SimpleFileOptions::default()).unwrap();
        std::io::Write::write_all(&mut z, b"<w:p><w:t>Rechnung Nr. 42</w:t></w:p><w:p><w:t>Betrag &amp; Steuer</w:t></w:p>").unwrap();
        z.finish().unwrap();
        assert_eq!(extract_text(&p), "Rechnung Nr. 42 Betrag & Steuer");
        let _ = std::fs::remove_dir_all(dir);
    }
}
