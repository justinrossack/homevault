//! Backup auf andere NAS im Haus (per SMB).
//!
//! homevault steuert dafür rclone: Es überträgt nur Änderungen und legt
//! gelöschte oder überschriebene Dateien auf dem Ziel unter
//! `.versionen/<Zeitpunkt>/` ab. Diese Versionen werden nach `keep_days`
//! Tagen aufgeräumt. Auf dem Ziel liegen normale Ordner, ein Backup lässt
//! sich also auch ohne homevault zurückkopieren.
//!
//! Aufbau auf dem Ziel:
//!   <freigabe>/<ordner>/Gemeinsam/…
//!   <freigabe>/<ordner>/Privat/<benutzer>/…
//!   <freigabe>/<ordner>/Datenbank/homevault.db
//!   <freigabe>/<ordner>/.versionen/2026-10-11_030000/…

use std::{
    path::{Path, PathBuf},
    process::Stdio,
    sync::{Arc, Mutex},
    time::Duration,
};

use axum::{
    Extension, Json,
    extract::{Path as UrlPath, State},
};
use chrono::{Datelike, Local, NaiveTime, TimeZone};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::Command,
    sync::Notify,
};

use crate::{
    AppState,
    auth::CurrentUser,
    db::{self, Db},
    error::AppError,
    paths,
};

pub(crate) const VERSIONS_DIR: &str = ".versionen";
pub(crate) const DB_DIR: &str = "Datenbank";

// ---------- Ziele ----------

#[derive(Clone)]
pub(crate) struct Target {
    id: i64,
    name: String,
    host: String,
    port: i64,
    share: String,
    folder: String,
    username: String,
    password: String,
    include_shared: bool,
    include_private: bool,
    include_db: bool,
    schedule: String,
    time: String,
    weekday: i64,
    keep_days: i64,
    enabled: bool,
    created_at: i64,
}

const TARGET_COLS: &str = "id, name, host, port, share, folder, username, password, include_shared, include_private, \
                           include_db, schedule, time, weekday, keep_days, enabled, created_at";

fn read_target(r: &rusqlite::Row) -> rusqlite::Result<Target> {
    Ok(Target {
        id: r.get(0)?,
        name: r.get(1)?,
        host: r.get(2)?,
        port: r.get(3)?,
        share: r.get(4)?,
        folder: r.get(5)?,
        username: r.get(6)?,
        password: r.get(7)?,
        include_shared: r.get(8)?,
        include_private: r.get(9)?,
        include_db: r.get(10)?,
        schedule: r.get(11)?,
        time: r.get(12)?,
        weekday: r.get(13)?,
        keep_days: r.get(14)?,
        enabled: r.get(15)?,
        created_at: r.get(16)?,
    })
}

pub(crate) fn load_target(conn: &Connection, id: i64) -> Result<Target, AppError> {
    conn.query_row(&format!("SELECT {TARGET_COLS} FROM backup_targets WHERE id = ?"), [id], read_target)
        .optional()?
        .ok_or(AppError::NotFound)
}

impl Target {
    /// rclone-Pfad auf dem Ziel, z. B. ":smb:backup/homevault/Gemeinsam"
    pub(crate) fn remote(&self, sub: &str) -> String {
        let mut p = format!(":smb:{}", self.share);
        for part in [self.folder.as_str(), sub] {
            if !part.is_empty() {
                p.push('/');
                p.push_str(part);
            }
        }
        p
    }

    pub(crate) fn display(&self) -> String {
        let folder = if self.folder.is_empty() { String::new() } else { format!("\\{}", self.folder.replace('/', "\\")) };
        format!("\\\\{}\\{}{folder}", self.host, self.share)
    }

    fn public(&self) -> Value {
        json!({
            "id": self.id,
            "name": self.name,
            "host": self.host,
            "port": self.port,
            "share": self.share,
            "folder": self.folder,
            "username": self.username,
            "has_password": !self.password.is_empty(),
            "include_shared": self.include_shared,
            "include_private": self.include_private,
            "include_db": self.include_db,
            "schedule": self.schedule,
            "time": self.time,
            "weekday": self.weekday,
            "keep_days": self.keep_days,
            "enabled": self.enabled,
            "location": self.display(),
        })
    }

    /// Letzter geplanter Zeitpunkt (Unix-Sekunden) bis jetzt, None bei „manuell“.
    fn last_slot(&self, now: chrono::DateTime<Local>) -> Option<i64> {
        let time = NaiveTime::parse_from_str(&self.time, "%H:%M").ok()?;
        let today = now.date_naive();
        let mut date = match self.schedule.as_str() {
            "daily" => today,
            "weekly" => {
                let back = (today.weekday().num_days_from_monday() as i64 - self.weekday).rem_euclid(7);
                today - chrono::Duration::days(back)
            }
            _ => return None,
        };
        let step = if self.schedule == "weekly" { 7 } else { 1 };
        loop {
            // .earliest(): bei der Zeitumstellung gibt es Uhrzeiten doppelt oder gar nicht
            if let Some(slot) = Local.from_local_datetime(&date.and_time(time)).earliest() {
                if slot <= now {
                    return Some(slot.timestamp());
                }
            }
            date -= chrono::Duration::days(step);
        }
    }
}

// ---------- Laufender Backup ----------

#[derive(Serialize, Clone, Default)]
pub struct Progress {
    pub(crate) target_id: i64,
    run_id: i64,
    /// Gerade gesicherter Teil: Gemeinsam, Privat, Datenbank, Aufräumen, Wiederherstellen
    pub(crate) part: String,
    bytes: i64,
    total_bytes: i64,
    transfers: i64,
    total_transfers: i64,
    /// Bytes pro Sekunde
    speed: f64,
    pub(crate) started_at: i64,
}

pub struct Backups {
    db: Arc<Db>,
    data_dir: PathBuf,
    work_dir: PathBuf,
    pub(crate) progress: Mutex<Option<Progress>>,
    cancel: Notify,
    /// Es läuft immer nur ein Backup (oder eine Wiederherstellung) gleichzeitig
    pub(crate) busy: Arc<tokio::sync::Mutex<()>>,
    /// Ergebnis der letzten Wiederherstellung
    pub(crate) restore_result: Mutex<Option<Value>>,
}

impl Progress {
    /// Neuer Fortschritt für einen Vorgang (Backup oder Wiederherstellung)
    pub(crate) fn start(target_id: i64) -> Self {
        Self { target_id, started_at: db::now(), ..Default::default() }
    }
}

impl Backups {
    pub fn new(db: Arc<Db>, data_dir: PathBuf, cache_dir: &Path) -> Arc<Self> {
        // Läufe, die bei einem Neustart unterbrochen wurden, als Fehler markieren
        let _ = db.blocking(|c| {
            c.execute(
                "UPDATE backup_runs SET status = 'error', finished_at = ?, message = 'Unterbrochen (Neustart)'
                 WHERE status = 'running'",
                [db::now()],
            )
        });
        Arc::new(Self {
            db,
            data_dir,
            work_dir: cache_dir.join("backup"),
            progress: Mutex::new(None),
            cancel: Notify::new(),
            busy: Arc::new(tokio::sync::Mutex::new(())),
            restore_result: Mutex::new(None),
        })
    }

    /// Prüft alle 30 Sekunden, ob ein geplanter Backup fällig ist.
    pub fn start_scheduler(self: &Arc<Self>) {
        let me = self.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(30)).await;
                if let Err(e) = me.run_due().await {
                    tracing::error!("Backup-Planer: {e}");
                }
            }
        });
    }

    async fn run_due(self: &Arc<Self>) -> Result<(), AppError> {
        let due = self
            .db
            .call(|c| {
                let targets: Vec<Target> = c
                    .prepare(&format!("SELECT {TARGET_COLS} FROM backup_targets WHERE enabled = 1 AND schedule != 'manual'"))?
                    .query_map([], read_target)?
                    .collect::<Result<_, _>>()?;
                let now = Local::now();
                let mut due = Vec::new();
                for t in targets {
                    let Some(slot) = t.last_slot(now) else { continue };
                    let last: Option<i64> = c.query_row(
                        "SELECT max(started_at) FROM backup_runs WHERE target_id = ?",
                        [t.id],
                        |r| r.get(0),
                    )?;
                    // nicht sofort nach dem Anlegen für einen vergangenen Termin starten
                    if slot > last.unwrap_or(0).max(t.created_at) {
                        due.push(t.id);
                    }
                }
                Ok(due)
            })
            .await?;
        for id in due {
            // nacheinander; ist gerade ein manueller Backup aktiv, beim nächsten Durchlauf
            if self.busy.try_lock().is_ok() {
                tracing::info!("Geplanter Backup für Ziel {id} startet");
                self.clone().run(id).await;
            }
        }
        Ok(())
    }

    pub fn progress(&self) -> Option<Progress> {
        self.progress.lock().unwrap().clone()
    }

    /// Startet einen Backup im Hintergrund. Fehler, wenn schon einer läuft.
    pub fn spawn(self: &Arc<Self>, target_id: i64) -> Result<(), AppError> {
        if self.busy.try_lock().is_err() {
            return Err(AppError::Conflict("Es läuft bereits ein Backup".into()));
        }
        let me = self.clone();
        tokio::spawn(async move { me.run(target_id).await });
        Ok(())
    }

    async fn run(self: Arc<Self>, target_id: i64) {
        let _guard = self.busy.lock().await;
        let started = db::now();
        let prepared = self
            .db
            .call(move |c| {
                let t = load_target(c, target_id)?;
                c.execute(
                    "INSERT INTO backup_runs (target_id, started_at, status) VALUES (?, ?, 'running')",
                    params![target_id, started],
                )?;
                Ok((t, c.last_insert_rowid()))
            })
            .await;
        let (target, run_id) = match prepared {
            Ok(x) => x,
            Err(e) => {
                tracing::error!("Backup {target_id}: {e}");
                return;
            }
        };
        *self.progress.lock().unwrap() = Some(Progress { target_id, run_id, started_at: started, ..Default::default() });

        let result = self.run_parts(&target, run_id).await;
        let (status, message, files, bytes) = match result {
            Ok((files, bytes)) => ("ok", None, files, bytes),
            Err(RunError::Cancelled) => ("cancelled", Some("Abgebrochen".to_owned()), 0, 0),
            Err(RunError::Failed(m)) => ("error", Some(m), 0, 0),
        };
        match status {
            "ok" => tracing::info!("Backup „{}“ fertig: {files} Dateien, {bytes} Bytes übertragen", target.name),
            _ => tracing::warn!("Backup „{}“: {}", target.name, message.as_deref().unwrap_or("")),
        }
        let _ = self
            .db
            .call(move |c| {
                c.execute(
                    "UPDATE backup_runs SET status = ?, finished_at = ?, files = ?, bytes = ?, message = ? WHERE id = ?",
                    params![status, db::now(), files, bytes, message, run_id],
                )?;
                Ok(())
            })
            .await;
        *self.progress.lock().unwrap() = None;
    }

    async fn run_parts(&self, t: &Target, _run_id: i64) -> Result<(i64, i64), RunError> {
        let env = rclone_env(t).await.map_err(RunError::Failed)?;
        let stamp = Local::now().format("%Y-%m-%d_%H%M%S").to_string();

        let mut parts: Vec<(&str, PathBuf)> = Vec::new();
        if t.include_shared {
            parts.push((paths::SHARED, self.data_dir.join(paths::SHARED)));
        }
        if t.include_private {
            parts.push((paths::PRIVATE, self.data_dir.join(paths::PRIVATE)));
        }
        if t.include_db {
            // konsistenter Schnappschuss, auch wenn die DB gerade benutzt wird
            let dir = self.work_dir.join(DB_DIR);
            let _ = tokio::fs::remove_dir_all(&dir).await;
            tokio::fs::create_dir_all(&dir).await.map_err(|e| RunError::Failed(e.to_string()))?;
            let file = dir.join("homevault.db");
            let f = file.to_string_lossy().into_owned();
            self.db
                .call(move |c| Ok(c.execute("VACUUM INTO ?", [f])?))
                .await
                .map_err(|e| RunError::Failed(format!("Datenbank-Schnappschuss: {e}")))?;
            parts.push((DB_DIR, dir));
        }
        if parts.is_empty() {
            return Err(RunError::Failed("Nichts zum Sichern ausgewählt".into()));
        }

        let (mut files, mut bytes) = (0, 0);
        for (name, src) in parts {
            self.set_part(name);
            let backup_dir = t.remote(&format!("{VERSIONS_DIR}/{stamp}/{name}"));
            let mut args: Vec<String> = vec!["sync".into(), src.to_string_lossy().into_owned(), t.remote(name)];
            args.extend(
                [
                    "--backup-dir", &backup_dir,
                    "--exclude", ".upload-*.part",
                    "--exclude", ".DS_Store",
                    "--exclude", "._*",
                    // auch leere Ordner übertragen, damit die Struktur vollständig ist
                    "--create-empty-src-dirs",
                    "--transfers", "4",
                    "--use-json-log",
                    "--stats", "2s",
                    "--stats-log-level", "NOTICE",
                ]
                .map(String::from),
            );
            let (f, b) = self.rclone_tracked(&args, &env).await?;
            files += f;
            bytes += b;
        }

        self.set_part("Aufräumen");
        self.prune_versions(t, &env).await;
        Ok((files, bytes))
    }

    pub(crate) fn set_part(&self, part: &str) {
        if let Some(p) = self.progress.lock().unwrap().as_mut() {
            p.part = part.to_owned();
            p.bytes = 0;
            p.total_bytes = 0;
            p.transfers = 0;
            p.total_transfers = 0;
            p.speed = 0.0;
        }
    }

    /// rclone ausführen, Fortschritt aus dem JSON-Log lesen, abbrechbar.
    /// Gibt (übertragene Dateien, Bytes) zurück.
    pub(crate) async fn rclone_tracked(&self, args: &[String], env: &[(String, String)]) -> Result<(i64, i64), RunError> {
        let mut child = Command::new("rclone")
            .args(args)
            .envs(env.iter().cloned())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| RunError::Failed(format!("rclone nicht verfügbar: {e}")))?;
        let mut lines = BufReader::new(child.stderr.take().unwrap()).lines();
        let mut errors: Vec<String> = Vec::new();
        let mut last = (0, 0);
        loop {
            tokio::select! {
                line = lines.next_line() => {
                    let Ok(Some(line)) = line else { break };
                    let Ok(v) = serde_json::from_str::<Value>(&line) else { continue };
                    if let Some(s) = v.get("stats") {
                        let n = |k: &str| s.get(k).and_then(Value::as_i64).unwrap_or(0);
                        last = (n("transfers"), n("bytes"));
                        if let Some(p) = self.progress.lock().unwrap().as_mut() {
                            p.bytes = n("bytes");
                            p.total_bytes = n("totalBytes");
                            p.transfers = n("transfers");
                            p.total_transfers = n("totalTransfers");
                            p.speed = s.get("speed").and_then(Value::as_f64).unwrap_or(0.0);
                        }
                    } else if v.get("level").and_then(Value::as_str) == Some("error") {
                        if let Some(msg) = v.get("msg").and_then(Value::as_str) {
                            errors.push(msg.trim().to_owned());
                        }
                    }
                }
                _ = self.cancel.notified() => {
                    let _ = child.kill().await;
                    return Err(RunError::Cancelled);
                }
            }
        }
        let status = child.wait().await.map_err(|e| RunError::Failed(e.to_string()))?;
        if status.success() {
            Ok(last)
        } else {
            Err(RunError::Failed(
                errors.last().cloned().unwrap_or_else(|| format!("rclone beendet mit {status}")),
            ))
        }
    }

    /// Versionen älter als keep_days entfernen.
    async fn prune_versions(&self, t: &Target, env: &[(String, String)]) {
        if t.keep_days <= 0 {
            return;
        }
        let base = t.remote(VERSIONS_DIR);
        let Ok(out) = rclone_output(&["lsf", "--dirs-only", &base], env).await else { return };
        let cutoff = (Local::now() - chrono::Duration::days(t.keep_days)).format("%Y-%m-%d_%H%M%S").to_string();
        for dir in out.lines().map(|l| l.trim_end_matches('/')) {
            // Ordnernamen sind Zeitstempel und damit alphabetisch sortierbar
            if dir.len() == cutoff.len() && dir < cutoff.as_str() {
                let path = format!("{base}/{dir}");
                match rclone_output(&["purge", &path], env).await {
                    Ok(_) => tracing::info!("Alte Backup-Version {dir} von „{}“ entfernt", t.name),
                    Err(e) => tracing::warn!("Version {dir} konnte nicht entfernt werden: {e}"),
                }
            }
        }
    }

    pub fn cancel(&self) {
        self.cancel.notify_waiters();
    }
}

pub(crate) enum RunError {
    Cancelled,
    Failed(String),
}

/// Zugangsdaten für rclone. Das Passwort wird über stdin verschleiert
/// (`rclone obscure -`) und per Umgebungsvariable übergeben, damit es nicht
/// in der Prozessliste auftaucht.
pub(crate) async fn rclone_env(t: &Target) -> Result<Vec<(String, String)>, String> {
    let mut child = Command::new("rclone")
        .args(["obscure", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("rclone nicht verfügbar: {e}"))?;
    child
        .stdin
        .take()
        .unwrap()
        .write_all(t.password.as_bytes())
        .await
        .map_err(|e| e.to_string())?;
    let out = child.wait_with_output().await.map_err(|e| e.to_string())?;
    let obscured = String::from_utf8_lossy(&out.stdout).trim().to_owned();
    Ok(vec![
        ("RCLONE_SMB_HOST".into(), t.host.clone()),
        ("RCLONE_SMB_PORT".into(), t.port.to_string()),
        ("RCLONE_SMB_USER".into(), t.username.clone()),
        ("RCLONE_SMB_PASS".into(), obscured),
        ("RCLONE_CONTIMEOUT".into(), "15s".into()),
        ("RCLONE_TIMEOUT".into(), "2m".into()),
        // keine rclone.conf nötig
        ("RCLONE_CONFIG".into(), "/dev/null".into()),
    ])
}

/// Kurzer rclone-Aufruf, gibt stdout zurück oder die letzte Fehlerzeile.
pub(crate) async fn rclone_output(args: &[&str], env: &[(String, String)]) -> Result<String, String> {
    let out = tokio::time::timeout(
        Duration::from_secs(60),
        Command::new("rclone").args(args).envs(env.iter().cloned()).kill_on_drop(true).output(),
    )
    .await
    .map_err(|_| "Zeitüberschreitung: Ziel antwortet nicht".to_string())?
    .map_err(|e| format!("rclone nicht verfügbar: {e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        let err = String::from_utf8_lossy(&out.stderr);
        Err(friendly_error(err.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("Unbekannter Fehler")))
    }
}

/// Typische SMB-Fehler verständlich machen.
fn friendly_error(raw: &str) -> String {
    let lower = raw.to_lowercase();
    let hint = if lower.contains("logon failure") || lower.contains("access denied") || lower.contains("authentication") {
        "Benutzername oder Passwort falsch"
    } else if lower.contains("bad network name") || lower.contains("network name") {
        "Freigabe nicht gefunden"
    } else if lower.contains("no such host") || lower.contains("lookup") {
        "Server nicht gefunden"
    } else if lower.contains("connection refused") || lower.contains("i/o timeout") || lower.contains("no route") {
        "Server nicht erreichbar (ist SMB aktiviert?)"
    } else {
        return raw.trim().to_owned();
    };
    format!("{hint} ({})", raw.trim().chars().take(160).collect::<String>())
}

// ---------- HTTP (nur Admin) ----------

fn require_admin(u: &CurrentUser) -> Result<(), AppError> {
    if u.admin { Ok(()) } else { Err(AppError::Forbidden) }
}

#[derive(Deserialize)]
pub struct TargetBody {
    #[serde(default = "default_name")]
    pub(crate) name: String,
    host: String,
    #[serde(default = "default_port")]
    port: i64,
    share: String,
    #[serde(default)]
    folder: String,
    username: String,
    /// leer/fehlend beim Bearbeiten = bisheriges Passwort behalten
    pub(crate) password: Option<String>,
    #[serde(default = "yes")]
    include_shared: bool,
    #[serde(default = "yes")]
    include_private: bool,
    #[serde(default = "yes")]
    include_db: bool,
    #[serde(default = "manual")]
    schedule: String,
    #[serde(default = "midnight")]
    time: String,
    #[serde(default)]
    weekday: i64,
    #[serde(default)]
    keep_days: i64,
    #[serde(default = "yes")]
    enabled: bool,
}

fn default_name() -> String {
    "Backup".into()
}
fn default_port() -> i64 {
    445
}
fn manual() -> String {
    "manual".into()
}
fn midnight() -> String {
    "00:00".into()
}
fn yes() -> bool {
    true
}

impl TargetBody {
    /// Ein Ziel nur zum Verbinden (Test, Wiederherstellung auf neuem Server)
    pub(crate) fn into_target(self, password: String) -> Target {
        Target {
            id: 0,
            name: self.name,
            host: self.host,
            port: self.port,
            share: self.share,
            folder: self.folder,
            username: self.username,
            password,
            include_shared: true,
            include_private: true,
            include_db: true,
            schedule: "manual".into(),
            time: "00:00".into(),
            weekday: 0,
            keep_days: 0,
            enabled: true,
            created_at: 0,
        }
    }

    pub(crate) fn validate(&mut self) -> Result<(), AppError> {
        let bad = |m: &str| Err(AppError::BadRequest(m.into()));
        self.name = self.name.trim().to_owned();
        self.host = self.host.trim().trim_start_matches("\\\\").trim_start_matches("smb://").trim_end_matches('/').to_owned();
        self.share = self.share.trim().trim_matches(['/', '\\']).to_owned();
        self.folder = self.folder.trim().replace('\\', "/").trim_matches('/').to_owned();
        self.username = self.username.trim().to_owned();
        if self.name.is_empty() || self.name.chars().count() > 60 {
            return bad("Name: 1 bis 60 Zeichen");
        }
        if self.host.is_empty() || self.host.contains(['/', '\\', ' ', ':']) {
            return bad("Server: IP-Adresse oder Name, z. B. 192.168.1.20");
        }
        if self.share.is_empty() || self.share.contains(['/', '\\']) {
            return bad("Freigabe: nur der Name, z. B. backup");
        }
        if self.folder.split('/').any(|p| p == ".." || p == ".") {
            return bad("Ungültiger Ordner");
        }
        if self.username.is_empty() {
            return bad("Benutzername fehlt");
        }
        if !(1..=65535).contains(&self.port) {
            return bad("Ungültiger Port");
        }
        if !matches!(self.schedule.as_str(), "daily" | "weekly" | "manual") {
            return bad("Ungültiger Zeitplan");
        }
        if NaiveTime::parse_from_str(&self.time, "%H:%M").is_err() {
            return bad("Uhrzeit im Format HH:MM");
        }
        if !(0..=6).contains(&self.weekday) || !(0..=3650).contains(&self.keep_days) {
            return bad("Ungültige Werte");
        }
        if !(self.include_shared || self.include_private || self.include_db) {
            return bad("Bitte mindestens einen Bereich zum Sichern auswählen");
        }
        Ok(())
    }
}

pub async fn list(State(state): State<AppState>, Extension(me): Extension<CurrentUser>) -> Result<Json<Value>, AppError> {
    require_admin(&me)?;
    let progress = state.backups.progress();
    let targets = state
        .db
        .call(|c| {
            let targets: Vec<Target> = c
                .prepare(&format!("SELECT {TARGET_COLS} FROM backup_targets ORDER BY name"))?
                .query_map([], read_target)?
                .collect::<Result<_, _>>()?;
            let now = Local::now();
            let mut out = Vec::new();
            for t in targets {
                let last: Option<Value> = c
                    .query_row(
                        "SELECT started_at, finished_at, status, files, bytes, message FROM backup_runs
                         WHERE target_id = ? AND status != 'running' ORDER BY started_at DESC LIMIT 1",
                        [t.id],
                        |r| {
                            Ok(json!({
                                "started_at": r.get::<_, i64>(0)?,
                                "finished_at": r.get::<_, Option<i64>>(1)?,
                                "status": r.get::<_, String>(2)?,
                                "files": r.get::<_, i64>(3)?,
                                "bytes": r.get::<_, i64>(4)?,
                                "message": r.get::<_, Option<String>>(5)?,
                            }))
                        },
                    )
                    .optional()?;
                let last_ok: Option<i64> = c.query_row(
                    "SELECT max(finished_at) FROM backup_runs WHERE target_id = ? AND status = 'ok'",
                    [t.id],
                    |r| r.get(0),
                )?;
                let mut v = t.public();
                v["last_run"] = json!(last);
                v["last_success"] = json!(last_ok);
                v["next_run"] = json!(next_run(&t, now));
                out.push(v);
            }
            Ok(out)
        })
        .await?;
    let restore = state.backups.restore_result.lock().unwrap().clone();
    Ok(Json(json!({ "targets": targets, "running": progress, "restore": restore })))
}

/// Nächster geplanter Zeitpunkt (für die Anzeige).
fn next_run(t: &Target, now: chrono::DateTime<Local>) -> Option<i64> {
    if !t.enabled {
        return None;
    }
    let days = if t.schedule == "weekly" { 7 } else { 1 };
    // letzter Termin + Intervall, mit kleiner Schleife wegen Zeitumstellung
    let last = t.last_slot(now)?;
    (1..=days + 1)
        .filter_map(|d| t.last_slot(now + chrono::Duration::days(d)))
        .find(|&s| s > last)
}

pub async fn create(
    State(state): State<AppState>,
    Extension(me): Extension<CurrentUser>,
    Json(mut b): Json<TargetBody>,
) -> Result<Json<Value>, AppError> {
    require_admin(&me)?;
    b.validate()?;
    let password = b.password.clone().filter(|p| !p.is_empty()).ok_or_else(|| AppError::BadRequest("Passwort fehlt".into()))?;
    let id = state
        .db
        .call(move |c| {
            c.execute(
                "INSERT INTO backup_targets (name, host, port, share, folder, username, password, include_shared,
                     include_private, include_db, schedule, time, weekday, keep_days, enabled, created_at)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                params![
                    b.name, b.host, b.port, b.share, b.folder, b.username, password, b.include_shared, b.include_private,
                    b.include_db, b.schedule, b.time, b.weekday, b.keep_days, b.enabled, db::now()
                ],
            )?;
            Ok(c.last_insert_rowid())
        })
        .await?;
    tracing::info!("Backup-Ziel {id} angelegt von '{}'", me.name);
    Ok(Json(json!({ "id": id })))
}

pub async fn update(
    State(state): State<AppState>,
    Extension(me): Extension<CurrentUser>,
    UrlPath(id): UrlPath<i64>,
    Json(mut b): Json<TargetBody>,
) -> Result<Json<Value>, AppError> {
    require_admin(&me)?;
    b.validate()?;
    state
        .db
        .call(move |c| {
            let old = load_target(c, id)?;
            let password = b.password.filter(|p| !p.is_empty()).unwrap_or(old.password);
            c.execute(
                "UPDATE backup_targets SET name = ?, host = ?, port = ?, share = ?, folder = ?, username = ?, password = ?,
                     include_shared = ?, include_private = ?, include_db = ?, schedule = ?, time = ?, weekday = ?,
                     keep_days = ?, enabled = ?
                 WHERE id = ?",
                params![
                    b.name, b.host, b.port, b.share, b.folder, b.username, password, b.include_shared, b.include_private,
                    b.include_db, b.schedule, b.time, b.weekday, b.keep_days, b.enabled, id
                ],
            )?;
            Ok(())
        })
        .await?;
    Ok(Json(json!({ "ok": true })))
}

pub async fn delete(
    State(state): State<AppState>,
    Extension(me): Extension<CurrentUser>,
    UrlPath(id): UrlPath<i64>,
) -> Result<Json<Value>, AppError> {
    require_admin(&me)?;
    if state.backups.progress().is_some_and(|p| p.target_id == id) {
        return Err(AppError::Conflict("Backup läuft gerade, bitte erst abbrechen".into()));
    }
    // Die Dateien auf dem Ziel bleiben unberührt
    state
        .db
        .call(move |c| {
            if c.execute("DELETE FROM backup_targets WHERE id = ?", [id])? == 0 {
                return Err(AppError::NotFound);
            }
            Ok(())
        })
        .await?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
pub struct TestBody {
    #[serde(flatten)]
    target: TargetBody,
    /// beim Bearbeiten: gespeichertes Passwort verwenden
    id: Option<i64>,
}

/// Verbindung prüfen: anmelden, Zielordner anlegen, Inhalt auflisten.
pub async fn test(
    State(state): State<AppState>,
    Extension(me): Extension<CurrentUser>,
    Json(mut b): Json<TestBody>,
) -> Result<Json<Value>, AppError> {
    require_admin(&me)?;
    b.target.validate()?;
    let password = match (b.target.password.clone().filter(|p| !p.is_empty()), b.id) {
        (Some(p), _) => p,
        (None, Some(id)) => state.db.call(move |c| Ok(load_target(c, id)?.password)).await?,
        (None, None) => return Err(AppError::BadRequest("Passwort fehlt".into())),
    };
    let t = b.target.into_target(password);
    let env = rclone_env(&t).await.map_err(AppError::Internal)?;
    // beim Testen nicht mehrfach neu versuchen, damit Fehler schnell sichtbar werden
    let once = ["--retries", "1", "--low-level-retries", "1"];
    let base = t.remote("");
    let result = async {
        rclone_output(&[&["mkdir", base.as_str()][..], &once].concat(), &env).await?;
        rclone_output(&[&["lsf", "--max-depth", "1", base.as_str()][..], &once].concat(), &env).await
    }
    .await;
    Ok(Json(match result {
        Ok(listing) => {
            let existing = listing.lines().any(|l| l.trim_end_matches('/') == DB_DIR || l.trim_end_matches('/') == paths::SHARED);
            json!({
                "ok": true,
                "message": if existing {
                    format!("Verbindung zu {} klappt. Dort liegt bereits ein Backup, es wird fortgesetzt.", t.display())
                } else {
                    format!("Verbindung zu {} klappt.", t.display())
                },
            })
        }
        Err(e) => json!({ "ok": false, "message": e }),
    }))
}

pub async fn run_now(
    State(state): State<AppState>,
    Extension(me): Extension<CurrentUser>,
    UrlPath(id): UrlPath<i64>,
) -> Result<Json<Value>, AppError> {
    require_admin(&me)?;
    state.db.call(move |c| load_target(c, id).map(|_| ())).await?;
    state.backups.spawn(id)?;
    Ok(Json(json!({ "ok": true })))
}

pub async fn cancel(State(state): State<AppState>, Extension(me): Extension<CurrentUser>) -> Result<Json<Value>, AppError> {
    require_admin(&me)?;
    state.backups.cancel();
    Ok(Json(json!({ "ok": true })))
}

pub async fn runs(
    State(state): State<AppState>,
    Extension(me): Extension<CurrentUser>,
    UrlPath(id): UrlPath<i64>,
) -> Result<Json<Value>, AppError> {
    require_admin(&me)?;
    let runs = state
        .db
        .call(move |c| {
            let rows = c
                .prepare(
                    "SELECT started_at, finished_at, status, files, bytes, message FROM backup_runs
                     WHERE target_id = ? ORDER BY started_at DESC LIMIT 30",
                )?
                .query_map([id], |r| {
                    Ok(json!({
                        "started_at": r.get::<_, i64>(0)?,
                        "finished_at": r.get::<_, Option<i64>>(1)?,
                        "status": r.get::<_, String>(2)?,
                        "files": r.get::<_, i64>(3)?,
                        "bytes": r.get::<_, i64>(4)?,
                        "message": r.get::<_, Option<String>>(5)?,
                    }))
                })?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
        .await?;
    Ok(Json(json!({ "runs": runs })))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    fn target(schedule: &str, time: &str, weekday: i64) -> Target {
        Target {
            id: 1,
            name: "t".into(),
            host: "nas".into(),
            port: 445,
            share: "backup".into(),
            folder: "homevault/haus".into(),
            username: "u".into(),
            password: "p".into(),
            include_shared: true,
            include_private: true,
            include_db: true,
            schedule: schedule.into(),
            time: time.into(),
            weekday,
            keep_days: 30,
            enabled: true,
            created_at: 0,
        }
    }

    fn at(y: i32, m: u32, d: u32, h: u32, min: u32) -> chrono::DateTime<Local> {
        Local
            .from_local_datetime(&NaiveDate::from_ymd_opt(y, m, d).unwrap().and_hms_opt(h, min, 0).unwrap())
            .earliest()
            .unwrap()
    }

    #[test]
    fn pfade_auf_dem_ziel() {
        let t = target("daily", "03:00", 0);
        assert_eq!(t.remote("Gemeinsam"), ":smb:backup/homevault/haus/Gemeinsam");
        assert_eq!(t.remote(""), ":smb:backup/homevault/haus");
        assert_eq!(t.display(), "\\\\nas\\backup\\homevault\\haus");
    }

    #[test]
    fn zeitplan() {
        let daily = target("daily", "03:00", 0);
        // vor 3 Uhr: Termin von gestern
        assert_eq!(daily.last_slot(at(2026, 10, 14, 2, 0)), Some(at(2026, 10, 13, 3, 0).timestamp()));
        // nach 3 Uhr: heute
        assert_eq!(daily.last_slot(at(2026, 10, 14, 3, 1)), Some(at(2026, 10, 14, 3, 0).timestamp()));
        // wöchentlich sonntags (6); 14.10.2026 ist ein Mittwoch
        let weekly = target("weekly", "22:30", 6);
        assert_eq!(weekly.last_slot(at(2026, 10, 14, 12, 0)), Some(at(2026, 10, 11, 22, 30).timestamp()));
        assert_eq!(next_run(&weekly, at(2026, 10, 14, 12, 0)), Some(at(2026, 10, 18, 22, 30).timestamp()));
        assert_eq!(target("manual", "03:00", 0).last_slot(at(2026, 10, 14, 12, 0)), None);
    }

    #[test]
    fn eingaben_werden_bereinigt() {
        let mut b = TargetBody {
            name: " Papas NAS ".into(),
            host: "\\\\192.168.1.20".into(),
            port: 445,
            share: "/backup/".into(),
            folder: "\\homevault\\".into(),
            username: "papa".into(),
            password: Some("x".into()),
            include_shared: true,
            include_private: false,
            include_db: true,
            schedule: "weekly".into(),
            time: "03:00".into(),
            weekday: 6,
            keep_days: 30,
            enabled: true,
        };
        b.validate().unwrap();
        assert_eq!((b.name.as_str(), b.host.as_str(), b.share.as_str(), b.folder.as_str()), ("Papas NAS", "192.168.1.20", "backup", "homevault"));
        b.folder = "../x".into();
        assert!(b.validate().is_err());
    }
}
