//! Medien-Index: erfasst alle Fotos und Videos in der Datenbank, damit
//! Zeitleiste, Favoriten und Alben über alle Ordner hinweg funktionieren.
//!
//! Läuft in einem eigenen Thread mit eigener DB-Verbindung. Ein Scan prüft
//! nur Größe und Änderungszeit; Aufnahmedatum (EXIF/ffprobe) und Video-Codec
//! werden nur für neue oder geänderte Dateien gelesen. Dateien, die außerhalb
//! der App verschoben wurden (z. B. per SMB), werden an Größe und
//! Änderungszeit wiedererkannt, damit Favoriten und Alben erhalten bleiben.
//!
//! Nebenbei zählt der Scan die Speicherbelegung pro Bereich und Benutzer.

use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fs::File,
    io::BufReader,
    path::{Path, PathBuf},
    process::Stdio,
    sync::{
        Mutex,
        mpsc::{self, RecvTimeoutError, Sender},
    },
    time::{Duration, Instant, UNIX_EPOCH},
};

use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;

use crate::{
    db::{self, Db},
    error::AppError,
    fs_api::{Kind, is_live_motion, is_live_still, kind_of, stem_lower},
    paths,
    trash::TRASH_DIR,
};

const CHUNK: usize = 200;

/// Speicherbelegung eines Bereichs ("Gemeinsam" oder "Privat/<name>")
#[derive(Serialize, Clone, Default)]
pub struct AreaUsage {
    pub bytes: u64,
    pub files: u64,
    pub photo_bytes: u64,
    pub video_bytes: u64,
}

#[derive(Serialize, Clone, Default)]
pub struct Status {
    pub scanning: bool,
    pub items: i64,
    /// Unix-Zeit des letzten abgeschlossenen Scans
    pub last_scan: Option<i64>,
    pub last_duration_ms: Option<u64>,
    pub last_error: Option<String>,
    #[serde(skip)]
    pub usage: BTreeMap<String, AreaUsage>,
    #[serde(skip)]
    pub trash_bytes: u64,
    #[serde(skip)]
    pub cache_bytes: u64,
}

pub struct Index {
    tx: Mutex<Sender<()>>,
    status: Mutex<Status>,
    /// angehalten, z. B. während der Speicher umgezogen wird
    paused: std::sync::atomic::AtomicBool,
}

impl Index {
    /// Startet den Index-Thread. Scannt sofort, danach alle `interval` und
    /// immer dann, wenn `request_scan` aufgerufen wird.
    pub fn start(db_path: PathBuf, data_dir: PathBuf, cache_dir: PathBuf, interval: Duration, paused: bool) -> std::sync::Arc<Self> {
        let (tx, rx) = mpsc::channel::<()>();
        let index = std::sync::Arc::new(Self {
            tx: Mutex::new(tx),
            status: Mutex::new(Status::default()),
            paused: std::sync::atomic::AtomicBool::new(paused),
        });
        let me = index.clone();
        std::thread::Builder::new()
            .name("index".into())
            .spawn(move || {
                let mut conn = match Db::connect(&db_path) {
                    Ok(c) => c,
                    Err(e) => {
                        tracing::error!("Index: keine Datenbankverbindung: {e}");
                        return;
                    }
                };
                loop {
                    if me.paused.load(std::sync::atomic::Ordering::SeqCst) {
                        std::thread::sleep(Duration::from_secs(2));
                        continue;
                    }
                    me.status.lock().unwrap().scanning = true;
                    let started = Instant::now();
                    let mut usage = BTreeMap::new();
                    let result = scan(&mut conn, &data_dir, &mut usage);
                    let items = conn.query_row("SELECT count(*) FROM media", [], |r| r.get(0)).unwrap_or(0);
                    let trash_bytes = dir_size(&data_dir.join(TRASH_DIR));
                    let cache_bytes = dir_size(&cache_dir);
                    {
                        let mut s = me.status.lock().unwrap();
                        s.scanning = false;
                        s.items = items;
                        s.last_scan = Some(db::now());
                        s.last_duration_ms = Some(started.elapsed().as_millis() as u64);
                        s.last_error = result.as_ref().err().map(|e| e.to_string());
                        s.usage = usage;
                        s.trash_bytes = trash_bytes;
                        s.cache_bytes = cache_bytes;
                    }
                    match result {
                        Ok(st) if st.changed() => tracing::info!(
                            "Index: {} neu, {} geändert, {} verschoben, {} entfernt ({} ms)",
                            st.added,
                            st.updated,
                            st.moved,
                            st.removed,
                            started.elapsed().as_millis()
                        ),
                        Ok(_) => {}
                        Err(e) => tracing::error!("Index-Scan fehlgeschlagen: {e}"),
                    }
                    match rx.recv_timeout(interval) {
                        Ok(()) => {
                            // kurz warten und mehrere Anfragen zusammenfassen
                            std::thread::sleep(Duration::from_millis(1500));
                            while rx.try_recv().is_ok() {}
                        }
                        Err(RecvTimeoutError::Timeout) => {}
                        Err(RecvTimeoutError::Disconnected) => break,
                    }
                }
            })
            .expect("Index-Thread konnte nicht gestartet werden");
        index
    }

    pub fn pause(&self, paused: bool) {
        self.paused.store(paused, std::sync::atomic::Ordering::SeqCst);
    }

    pub fn request_scan(&self) {
        let _ = self.tx.lock().unwrap().send(());
    }

    pub fn status(&self) -> Status {
        self.status.lock().unwrap().clone()
    }

    /// Belegung eines Bereichs laut letztem Scan.
    pub fn usage_of(&self, area: &str) -> u64 {
        self.status.lock().unwrap().usage.get(area).map_or(0, |u| u.bytes)
    }
}

/// Größe eines Ordners (rekursiv, ohne Symlinks zu verfolgen).
pub fn dir_size(dir: &Path) -> u64 {
    let Ok(rd) = std::fs::read_dir(dir) else { return 0 };
    rd.flatten()
        .map(|e| match e.file_type() {
            Ok(t) if t.is_dir() => dir_size(&e.path()),
            Ok(t) if t.is_file() => e.metadata().map_or(0, |m| m.len()),
            _ => 0,
        })
        .sum()
}

#[derive(Default)]
struct ScanStats {
    added: usize,
    updated: usize,
    moved: usize,
    removed: usize,
}

impl ScanStats {
    fn changed(&self) -> bool {
        self.added + self.updated + self.moved + self.removed > 0
    }
}

struct Found {
    path: String,
    abs: PathBuf,
    kind: &'static str,
    size: i64,
    mtime: i64,
    live: Option<String>,
}

struct Row {
    id: i64,
    kind: String,
    size: i64,
    mtime: i64,
    live: Option<String>,
    probed: bool,
}

fn kind_str(path: &Path) -> Option<&'static str> {
    match kind_of(path) {
        Kind::Image => Some("image"),
        Kind::Video => Some("video"),
        _ => None,
    }
}

fn mtime_secs(meta: &std::fs::Metadata) -> i64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_secs() as i64)
}

/// "Gemeinsam/a/b.jpg" -> "Gemeinsam", "Privat/anna/x.jpg" -> "Privat/anna"
pub fn area_of(rel: &str) -> Option<String> {
    let mut parts = rel.split('/');
    match parts.next()? {
        s if s == paths::SHARED => Some(s.to_owned()),
        p if p == paths::PRIVATE => parts.next().filter(|_| parts.clone().next().is_some()).map(|u| format!("{p}/{u}")),
        _ => None,
    }
}

/// Läuft rekursiv durch einen Ordner. Symlinks werden nicht verfolgt
/// (Schutz vor Endlosschleifen und Pfaden außerhalb des Datenordners).
/// `incomplete` wird gesetzt, wenn etwas nicht lesbar war; dann darf
/// nichts als gelöscht gelten.
fn walk(dir: &Path, data_dir: &Path, out: &mut Vec<Found>, usage: &mut BTreeMap<String, AreaUsage>, incomplete: &mut bool) {
    let rd = match std::fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return,
        Err(e) => {
            tracing::warn!("Index: {} nicht lesbar: {e}", dir.display());
            *incomplete = true;
            return;
        }
    };
    let mut files = Vec::new();
    for e in rd {
        let Ok(e) = e else {
            *incomplete = true;
            continue;
        };
        let name = e.file_name().to_string_lossy().into_owned();
        let Ok(ft) = e.file_type() else {
            *incomplete = true;
            continue;
        };
        if ft.is_dir() {
            if !name.starts_with('.') {
                walk(&e.path(), data_dir, out, usage, incomplete);
            }
        } else if ft.is_file() {
            // Reste abgebrochener Uploads (z. B. Server-Neustart) aufräumen
            if name.starts_with(".upload-") && name.ends_with(".part") {
                let old = e.metadata().ok().and_then(|m| m.modified().ok()).and_then(|t| t.elapsed().ok());
                if old.is_some_and(|age| age > Duration::from_secs(86400)) {
                    let _ = std::fs::remove_file(e.path());
                    continue;
                }
            }
            // auch versteckte Dateien belegen Platz
            let size = e.metadata().map_or(0, |m| m.len());
            let rel = paths::relative(data_dir, &e.path());
            if let Some(area) = area_of(&rel) {
                let u = usage.entry(area).or_default();
                u.bytes += size;
                u.files += 1;
                match kind_of(&e.path()) {
                    Kind::Image => u.photo_bytes += size,
                    Kind::Video => u.video_bytes += size,
                    _ => {}
                }
            }
            if !name.starts_with('.') {
                files.push(e.path());
            }
        }
    }

    // Live Photos: das .MOV gehört zum Foto und wird nicht einzeln erfasst
    let motions: HashMap<String, &PathBuf> = files.iter().filter(|p| is_live_motion(p)).map(|p| (stem_lower(p), p)).collect();
    let mut used = HashSet::new();
    let mut lives = HashMap::new();
    for p in &files {
        if kind_of(p) == Kind::Image && is_live_still(p) {
            if let Some(m) = motions.get(&stem_lower(p)) {
                if used.insert((*m).clone()) {
                    lives.insert(p.clone(), paths::relative(data_dir, m));
                }
            }
        }
    }

    for p in files {
        if used.contains(&p) {
            continue;
        }
        let Some(kind) = kind_str(&p) else { continue };
        let meta = match std::fs::metadata(&p) {
            Ok(m) => m,
            Err(e) => {
                if e.kind() != std::io::ErrorKind::NotFound {
                    *incomplete = true;
                }
                continue;
            }
        };
        out.push(Found {
            path: paths::relative(data_dir, &p),
            live: lives.remove(&p),
            abs: p,
            kind,
            size: meta.len() as i64,
            mtime: mtime_secs(&meta),
        });
    }
}

/// Bereich, Art, Größe, Änderungszeit: daran wird eine verschobene Datei erkannt
type MoveKey = (Option<String>, String, i64, i64);
/// neue Datei: verschoben von (ID, alter Pfad) oder neu mit Metadaten
type NewFile<'a> = (&'a Found, Option<(i64, String)>, Option<Meta>);

fn scan(conn: &mut Connection, data_dir: &Path, usage: &mut BTreeMap<String, AreaUsage>) -> Result<ScanStats, AppError> {
    let mut found = Vec::new();
    let mut incomplete = false;
    for space in [paths::SHARED, paths::PRIVATE] {
        walk(&data_dir.join(space), data_dir, &mut found, usage, &mut incomplete);
    }

    // Einträge im Papierkorb bleiben „geparkt“, damit Alben und Favoriten
    // beim Wiederherstellen erhalten bleiben.
    let mut existing: HashMap<String, Row> = HashMap::new();
    {
        let mut stmt = conn.prepare(&format!(
            "SELECT id, path, kind, size, mtime, live_path, codec IS NOT NULL OR kind != 'video'
             FROM media WHERE path NOT LIKE '{TRASH_DIR}/%'"
        ))?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(1)?,
                Row { id: r.get(0)?, kind: r.get(2)?, size: r.get(3)?, mtime: r.get(4)?, live: r.get(5)?, probed: r.get(6)? },
            ))
        })?;
        for row in rows {
            let (path, row) = row?;
            existing.insert(path, row);
        }
    }

    let mut stats = ScanStats::default();
    let mut new = Vec::new();
    let mut changed = Vec::new();
    let mut live_updates = Vec::new();
    for f in found {
        match existing.remove(&f.path) {
            // Videos aus älteren Versionen ohne Codec-Info nachträglich erfassen
            Some(row) if row.size == f.size && row.mtime == f.mtime && row.probed => {
                if row.live != f.live {
                    live_updates.push((row.id, f.live));
                }
            }
            Some(row) => changed.push((row.id, f)),
            None => new.push(f),
        }
    }
    // Was jetzt noch in `existing` steht, ist verschwunden oder verschoben.
    // Verschieben nur innerhalb desselben Bereichs erkennen: sonst würde z. B.
    // ein Teilen-Link einer Datei folgen, die in einen fremden Privat-Ordner
    // gewandert ist.
    let mut missing: HashMap<MoveKey, Vec<(i64, String)>> = HashMap::new();
    for (path, row) in existing {
        missing.entry((area_of(&path), row.kind, row.size, row.mtime)).or_default().push((row.id, path));
    }

    for chunk in new.chunks(CHUNK) {
        // Metadaten vor der Transaktion lesen, damit die DB nicht blockiert
        let prepared: Vec<NewFile> = chunk
            .iter()
            // inzwischen gelöscht (z. B. in den Papierkorb gelegt)
            .filter(|f| f.abs.exists())
            .map(|f| {
                let key = (area_of(&f.path), f.kind.to_owned(), f.size, f.mtime);
                match missing.get_mut(&key).and_then(|ids| ids.pop()) {
                    Some(id) => (f, Some(id), None),
                    None => (f, None, Some(read_meta(&f.abs, f.kind))),
                }
            })
            .collect();
        let tx = conn.transaction()?;
        for (f, moved_id, meta) in prepared {
            if let Some((id, old)) = moved_id {
                // nur, wenn der Eintrag nicht inzwischen woanders steht (Papierkorb)
                stats.moved += tx.execute(
                    "UPDATE media SET path = ?, live_path = ?, indexed_at = ? WHERE id = ? AND path = ?",
                    params![f.path, f.live, db::now(), id, old],
                )?;
            } else {
                let m = meta.unwrap_or_default();
                tx.execute(
                    "INSERT INTO media (path, kind, size, mtime, taken_at, live_path, indexed_at, codec, duration, hdr)
                     VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                    params![f.path, f.kind, f.size, f.mtime, m.taken.unwrap_or(f.mtime), f.live, db::now(), m.codec, m.duration, m.hdr],
                )?;
                stats.added += 1;
            }
        }
        tx.commit()?;
    }

    for chunk in changed.chunks(CHUNK) {
        let prepared: Vec<_> = chunk.iter().map(|(id, f)| (*id, f, read_meta(&f.abs, f.kind))).collect();
        let tx = conn.transaction()?;
        for (id, f, m) in prepared {
            tx.execute(
                "UPDATE media SET kind = ?, size = ?, mtime = ?, taken_at = ?, live_path = ?, indexed_at = ?,
                     codec = ?, duration = ?, hdr = ?
                 WHERE id = ?",
                params![f.kind, f.size, f.mtime, m.taken.unwrap_or(f.mtime), f.live, db::now(), m.codec, m.duration, m.hdr, id],
            )?;
            stats.updated += 1;
        }
        tx.commit()?;
    }

    let tx = conn.transaction()?;
    for (id, live) in live_updates {
        tx.execute("UPDATE media SET live_path = ? WHERE id = ?", params![live, id])?;
    }
    if incomplete {
        // lieber einen Durchlauf später aufräumen als Alben/Favoriten verlieren
        tracing::warn!("Index: nicht alles lesbar, es wird nichts als gelöscht markiert");
    } else {
        for (id, path) in missing.into_values().flatten() {
            // Während des Durchlaufs wiederhergestellt oder in den Papierkorb
            // gelegt? Dann stimmt der Pfad nicht mehr bzw. die Datei ist da.
            if data_dir.join(&path).exists() {
                continue;
            }
            // Favoriten/Album-Einträge verschwinden per ON DELETE CASCADE
            stats.removed += tx.execute("DELETE FROM media WHERE id = ? AND path = ?", params![id, path])?;
        }
    }
    tx.commit()?;
    Ok(stats)
}

// ---------- Einzelne Dateien (aus Web-Aktionen) ----------

/// Sucht das Live-Video zu einem Foto (gleicher Name, .MOV) im selben Ordner.
fn live_companion_sync(still: &Path) -> Option<PathBuf> {
    if !is_live_still(still) {
        return None;
    }
    let stem = stem_lower(still);
    std::fs::read_dir(still.parent()?)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .find(|p| is_live_motion(p) && stem_lower(p) == stem)
}

/// ID einer Datei im Index. Ist sie (z. B. direkt nach dem Upload) noch
/// nicht erfasst, wird sie sofort erfasst.
pub fn ensure_media(conn: &Connection, data_dir: &Path, db_path: &str) -> Result<i64, AppError> {
    if let Some(id) = conn.query_row("SELECT id FROM media WHERE path = ?", [db_path], |r| r.get(0)).optional()? {
        return Ok(id);
    }
    let abs = data_dir.join(db_path);
    let meta = std::fs::metadata(&abs)?;
    let kind = kind_str(&abs)
        .filter(|_| meta.is_file())
        .ok_or_else(|| AppError::BadRequest("Nur Fotos und Videos".into()))?;
    let live = live_companion_sync(&abs).map(|p| paths::relative(data_dir, &p));
    let mtime = mtime_secs(&meta);
    let m = read_meta(&abs, kind);
    conn.execute(
        "INSERT OR IGNORE INTO media (path, kind, size, mtime, taken_at, live_path, indexed_at, codec, duration, hdr)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        params![db_path, kind, meta.len() as i64, mtime, m.taken.unwrap_or(mtime), live, db::now(), m.codec, m.duration, m.hdr],
    )?;
    Ok(conn.query_row("SELECT id FROM media WHERE path = ?", [db_path], |r| r.get(0))?)
}

/// Pfade nach Umbenennen in der App nachziehen (auch für ganze Ordner),
/// damit Favoriten und Alben ihre Einträge behalten.
pub fn rename_paths(conn: &Connection, from: &str, to: &str) -> rusqlite::Result<()> {
    for col in ["path", "live_path"] {
        conn.execute(
            &format!(
                "UPDATE media SET {col} = ?2 || substr({col}, length(?1) + 1)
                 WHERE {col} = ?1 OR ({col} >= ?1 || '/' AND {col} < ?1 || '0')"
            ),
            params![from, to],
        )?;
    }
    Ok(())
}

/// Pfad (Datei oder Ordner) aus dem Index entfernen.
pub fn remove_paths(conn: &Connection, path: &str) -> rusqlite::Result<()> {
    conn.execute("DELETE FROM media WHERE path = ?1 OR (path >= ?1 || '/' AND path < ?1 || '0')", [path])?;
    Ok(())
}

// ---------- Metadaten ----------

#[derive(Default)]
pub struct Meta {
    /// Aufnahmezeit (Ortszeit als Unix-Sekunden)
    pub taken: Option<i64>,
    pub codec: Option<String>,
    pub duration: Option<f64>,
    pub hdr: bool,
}

fn read_meta(path: &Path, kind: &str) -> Meta {
    if kind == "video" {
        probe_video(path)
    } else {
        Meta { taken: exif_taken(path), ..Default::default() }
    }
}

/// Tage seit 1970-01-01 für ein Datum (proleptischer Gregorianischer Kalender).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

/// Ortszeit der Aufnahme als Unix-Sekunden (ohne Zeitzone, so wie die
/// Kamera sie anzeigt). Für die Zeitleiste zählt nur die Reihenfolge.
fn naive_epoch(y: i64, mo: i64, d: i64, h: i64, mi: i64, s: i64) -> Option<i64> {
    let valid = (1900..=2200).contains(&y) && (1..=12).contains(&mo) && (1..=31).contains(&d) && h < 24 && mi < 60 && s < 61;
    valid.then(|| days_from_civil(y, mo, d) * 86400 + h * 3600 + mi * 60 + s)
}

fn exif_taken(path: &Path) -> Option<i64> {
    let mut reader = BufReader::new(File::open(path).ok()?);
    let ex = exif::Reader::new().read_from_container(&mut reader).ok()?;
    for tag in [exif::Tag::DateTimeOriginal, exif::Tag::DateTimeDigitized, exif::Tag::DateTime] {
        let Some(field) = ex.get_field(tag, exif::In::PRIMARY) else { continue };
        let exif::Value::Ascii(ref v) = field.value else { continue };
        let Some(dt) = v.first().and_then(|b| exif::DateTime::from_ascii(b).ok()) else { continue };
        if let Some(t) = naive_epoch(dt.year as i64, dt.month as i64, dt.day as i64, dt.hour as i64, dt.minute as i64, dt.second as i64) {
            return Some(t);
        }
    }
    None
}

/// "2024-05-01T12:34:56..." -> Unix-Sekunden (Wanduhrzeit)
fn parse_iso(s: &str) -> Option<i64> {
    if s.len() < 19 {
        return None;
    }
    let n = |r: std::ops::Range<usize>| s.get(r)?.parse::<i64>().ok();
    naive_epoch(n(0..4)?, n(5..7)?, n(8..10)?, n(11..13)?, n(14..16)?, n(17..19)?)
}

/// Codec, Dauer, HDR und Aufnahmedatum eines Videos per ffprobe. iPhones
/// speichern die Ortszeit in `com.apple.quicktime.creationdate`, andere
/// Kameras `creation_time` (UTC).
pub fn probe_video(path: &Path) -> Meta {
    let out = std::process::Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-show_entries",
            "stream=codec_name,color_transfer:format=duration:format_tags=com.apple.quicktime.creationdate,creation_time",
            "-of",
            "json",
        ])
        .arg(path)
        .stderr(Stdio::null())
        .output();
    let Ok(out) = out else { return Meta::default() };
    let Ok(v) = serde_json::from_slice::<serde_json::Value>(&out.stdout) else { return Meta::default() };
    let stream = &v["streams"][0];
    let tags = &v["format"]["tags"];
    let tag = |k: &str| tags[k].as_str().and_then(parse_iso);
    Meta {
        taken: tag("com.apple.quicktime.creationdate").or_else(|| tag("creation_time")),
        codec: stream["codec_name"].as_str().map(str::to_owned),
        duration: v["format"]["duration"].as_str().and_then(|d| d.parse().ok()),
        // HLG (iPhone) oder PQ (HDR10)
        hdr: matches!(stream["color_transfer"].as_str(), Some("arib-std-b67" | "smpte2084")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn datum() {
        assert_eq!(naive_epoch(1970, 1, 1, 0, 0, 0), Some(0));
        assert_eq!(naive_epoch(2024, 2, 29, 12, 0, 0), Some(1709208000));
        assert_eq!(parse_iso("2024-02-29T12:00:00+0100"), Some(1709208000));
        assert_eq!(naive_epoch(0, 0, 0, 0, 0, 0), None);
    }

    #[test]
    fn bereiche() {
        assert_eq!(area_of("Gemeinsam/a/b.jpg").as_deref(), Some("Gemeinsam"));
        assert_eq!(area_of("Privat/anna/x.jpg").as_deref(), Some("Privat/anna"));
        assert_eq!(area_of("Privat/lose.jpg"), None);
        assert_eq!(area_of(".papierkorb/1/x.jpg"), None);
    }

    #[test]
    fn scan_erkennt_neu_verschoben_entfernt_und_live() {
        let d = tempfile::tempdir().unwrap();
        let data = d.path().canonicalize().unwrap();
        let shared = data.join("Gemeinsam/Urlaub");
        std::fs::create_dir_all(&shared).unwrap();
        std::fs::create_dir_all(data.join("Privat/anna")).unwrap();
        std::fs::write(shared.join("a.jpg"), b"aaaa").unwrap();
        std::fs::write(shared.join("IMG_1.HEIC"), b"bb").unwrap();
        std::fs::write(shared.join("IMG_1.MOV"), b"cc").unwrap();
        std::fs::write(data.join("Privat/anna/v.mp4"), b"ddd").unwrap();
        std::fs::write(shared.join("notiz.txt"), b"x").unwrap();

        let db = Db::open(&data.join("t.db")).unwrap();
        let mut conn = Db::connect(db.path()).unwrap();
        let mut usage = BTreeMap::new();
        let st = scan(&mut conn, &data, &mut usage).unwrap();
        assert_eq!(st.added, 3); // a.jpg, IMG_1.HEIC (mit Live), v.mp4
        assert_eq!(usage["Gemeinsam"].bytes, 4 + 2 + 2 + 1);
        assert_eq!(usage["Gemeinsam"].files, 4);
        assert_eq!(usage["Privat/anna"].video_bytes, 3);
        let live: Option<String> = conn
            .query_row("SELECT live_path FROM media WHERE path = 'Gemeinsam/Urlaub/IMG_1.HEIC'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(live.as_deref(), Some("Gemeinsam/Urlaub/IMG_1.MOV"));

        // Favorit setzen, dann Datei "per SMB" verschieben
        conn.execute(
            "INSERT INTO users (id, name, admin, uid, password_hash, nt_hash, created_at) VALUES (1, 'anna', 0, 20000, '', '', 0)",
            [],
        )
        .unwrap();
        let id: i64 = conn.query_row("SELECT id FROM media WHERE path LIKE '%a.jpg'", [], |r| r.get(0)).unwrap();
        conn.execute("INSERT INTO favorites VALUES (1, ?, 0)", [id]).unwrap();
        std::fs::create_dir_all(data.join("Gemeinsam/Neu")).unwrap();
        std::fs::rename(shared.join("a.jpg"), data.join("Gemeinsam/Neu/a.jpg")).unwrap();
        std::fs::remove_file(data.join("Privat/anna/v.mp4")).unwrap();

        let st = scan(&mut conn, &data, &mut BTreeMap::new()).unwrap();
        assert_eq!((st.moved, st.removed, st.added), (1, 1, 0));
        let fav_path: String =
            conn.query_row("SELECT m.path FROM favorites f JOIN media m ON m.id = f.media_id", [], |r| r.get(0)).unwrap();
        assert_eq!(fav_path, "Gemeinsam/Neu/a.jpg");

        // Im Papierkorb geparkte Einträge bleiben beim Scan erhalten
        rename_paths(&conn, "Gemeinsam/Neu/a.jpg", ".papierkorb/7/a.jpg").unwrap();
        let st = scan(&mut conn, &data, &mut BTreeMap::new()).unwrap();
        assert_eq!(st.removed, 0);
        let favs: i64 = conn.query_row("SELECT count(*) FROM favorites", [], |r| r.get(0)).unwrap();
        assert_eq!(favs, 1);

        // Umbenennen eines Ordners in der App
        rename_paths(&conn, "Gemeinsam/Urlaub", "Gemeinsam/Sommer").unwrap();
        let p: String =
            conn.query_row("SELECT live_path FROM media WHERE path LIKE 'Gemeinsam/Sommer/%'", [], |r| r.get(0)).unwrap();
        assert_eq!(p, "Gemeinsam/Sommer/IMG_1.MOV");
        // Präfix darf nicht "Sommer2" treffen
        conn.execute(
            "INSERT INTO media (path, kind, size, mtime, taken_at, indexed_at) VALUES ('Gemeinsam/Sommer2/x.jpg', 'image', 1, 1, 1, 1)",
            [],
        )
        .unwrap();
        remove_paths(&conn, "Gemeinsam/Sommer").unwrap();
        let left: i64 = conn.query_row("SELECT count(*) FROM media WHERE path LIKE 'Gemeinsam/Sommer2/%'", [], |r| r.get(0)).unwrap();
        assert_eq!(left, 1);
    }
}
