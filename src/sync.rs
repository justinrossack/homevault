//! Live-Sync: Ordner aus homevault laufend auf andere NAS spiegeln.
//!
//! Eine Regel heißt „Quelle → Ziel“, z. B. „Privat von anna → Keller-NAS
//! /Anna“. Ändert sich etwas in der Quelle, wird etwa eine Minute später
//! übertragen (bei Dauer-Uploads spätestens nach fünf Minuten). Nur der
//! geänderte Unterordner wird abgeglichen, nicht jedes Mal alles.
//!
//! Richtung: nur homevault → NAS. Auf dem NAS Gelöschtes oder
//! Überschriebenes landet unter `<ziel>/.versionen/<Zeitpunkt>/` und wird
//! nach `keep_days` Tagen aufgeräumt.
//!
//! Sicherheitsregeln:
//! - zwei Regeln (oder Regel und Backup) schreiben nie in denselben Ordner
//! - fehlt die Quelle oder ist sie plötzlich leer, wird nichts gelöscht
//! - während der Festplatten-Einrichtung wird nicht synchronisiert

use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    process::Stdio,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use axum::{
    Extension, Json,
    extract::{Path as UrlPath, State},
};
use chrono::Local;
use rusqlite::{Connection, OptionalExtension, params};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::Command,
    sync::Semaphore,
};

use crate::{
    AppState,
    auth::CurrentUser,
    backup::{self, Target, VERSIONS_DIR},
    db::{self, Db},
    error::AppError,
    paths,
};

/// so lange Ruhe abwarten, bevor übertragen wird
const QUIET: Duration = Duration::from_secs(60);
/// spätestens dann übertragen, auch wenn noch Änderungen kommen
const MAX_WAIT: Duration = Duration::from_secs(300);
/// nach einem Fehler (z. B. NAS aus) erneut versuchen
const RETRY: Duration = Duration::from_secs(300);
/// zusätzlich regelmäßig komplett abgleichen (falls etwas verpasst wurde)
const FULL_EVERY: Duration = Duration::from_secs(6 * 3600);
/// SQL: darf Benutzer ?1 sein Privat auf Ziel `t` synchronisieren? (alle oder ausgewählt)
const ALLOWED: &str =
    "(t.user_sync = 1 OR (t.user_sync = 2 AND EXISTS (SELECT 1 FROM sync_target_users s WHERE s.target_id = t.id AND s.user_id = ?1)))";
/// Ordner für Regeln, die Benutzer selbst anlegen: <backup-ordner>/Benutzer/<name>
const USER_DIR: &str = "Benutzer";

// ---------- Regeln ----------

struct Rule {
    id: i64,
    target_id: i64,
    source_space: String,
    source_user_id: Option<i64>,
    source_user: Option<String>,
    source_sub: String,
    dest: String,
    by_admin: bool,
    keep_days: i64,
    enabled: bool,
    last_run_at: Option<i64>,
    last_ok_at: Option<i64>,
    last_error: Option<String>,
    files: i64,
    bytes: i64,
}

const RULE_SQL: &str = "SELECT r.id, r.target_id, r.source_space, r.source_user_id, u.name, r.source_sub, r.dest, r.by_admin,
        r.keep_days, r.enabled, r.last_run_at, r.last_ok_at, r.last_error, r.files, r.bytes
    FROM sync_rules r LEFT JOIN users u ON u.id = r.source_user_id";

fn read_rule(r: &rusqlite::Row) -> rusqlite::Result<Rule> {
    Ok(Rule {
        id: r.get(0)?,
        target_id: r.get(1)?,
        source_space: r.get(2)?,
        source_user_id: r.get(3)?,
        source_user: r.get(4)?,
        source_sub: r.get(5)?,
        dest: r.get(6)?,
        by_admin: r.get(7)?,
        keep_days: r.get(8)?,
        enabled: r.get(9)?,
        last_run_at: r.get(10)?,
        last_ok_at: r.get(11)?,
        last_error: r.get(12)?,
        files: r.get(13)?,
        bytes: r.get(14)?,
    })
}

fn load_rule(c: &Connection, id: i64) -> Result<Rule, AppError> {
    c.query_row(&format!("{RULE_SQL} WHERE r.id = ?"), [id], read_rule).optional()?.ok_or(AppError::NotFound)
}

impl Rule {
    /// Quelle relativ zum Datenordner, z. B. "Privat/anna/Fotos"
    fn source(&self) -> String {
        let mut p = self.source_space.clone();
        if self.source_space == paths::PRIVATE {
            p.push('/');
            p.push_str(self.source_user.as_deref().unwrap_or("?"));
        }
        if !self.source_sub.is_empty() {
            p.push('/');
            p.push_str(&self.source_sub);
        }
        p
    }

    /// Für Menschen: "Gemeinsam/Fotos" oder "Privat von anna/Fotos"
    fn source_label(&self) -> String {
        let base = match &self.source_user {
            Some(u) if self.source_space == paths::PRIVATE => format!("Privat von {u}"),
            _ => self.source_space.clone(),
        };
        if self.source_sub.is_empty() { base } else { format!("{base}/{}", self.source_sub) }
    }
}

// ---------- Dienst ----------

struct Dirty {
    since: Instant,
    last: Instant,
    /// geänderter Unterordner der Quelle, "" = alles
    sub: String,
    now: bool,
}

#[derive(Default)]
struct SyncState {
    /// (Regel, Quelle relativ zum Datenordner) aller aktiven Regeln
    sources: Vec<(i64, String)>,
    dirty: HashMap<i64, Dirty>,
    running: HashSet<i64>,
    retry_at: HashMap<i64, Instant>,
    pruned: HashMap<i64, Instant>,
}

pub struct Syncer {
    db: Arc<Db>,
    data_dir: PathBuf,
    maintenance: Arc<AtomicBool>,
    state: Mutex<SyncState>,
    /// höchstens zwei Übertragungen gleichzeitig
    limit: Arc<Semaphore>,
}

impl Syncer {
    pub fn start(db: Arc<Db>, data_dir: PathBuf, maintenance: Arc<AtomicBool>, watcher: &crate::watch::Watcher) -> Arc<Self> {
        let me = Arc::new(Self { db, data_dir, maintenance, state: Mutex::default(), limit: Arc::new(Semaphore::new(2)) });
        let weak = Arc::downgrade(&me);
        watcher.subscribe(move |rel| {
            if let Some(me) = weak.upgrade() {
                me.changed(rel);
            }
        });
        let m = me.clone();
        tokio::spawn(async move {
            m.reload().await;
            // nach dem Start einmal alles abgleichen (Änderungen während der Server aus war)
            tokio::time::sleep(Duration::from_secs(60)).await;
            let mut last_full = Instant::now();
            m.mark_all();
            loop {
                if last_full.elapsed() >= FULL_EVERY {
                    m.mark_all();
                    last_full = Instant::now();
                }
                m.start_due();
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
        });
        me
    }

    /// Regel-Liste neu laden (nach Anlegen/Ändern/Löschen)
    async fn reload(&self) {
        let rules = self
            .db
            .call(|c| {
                Ok(c.prepare(&format!("{RULE_SQL} WHERE r.enabled = 1"))?
                    .query_map([], read_rule)?
                    .collect::<Result<Vec<_>, _>>()?)
            })
            .await;
        match rules {
            Ok(rules) => self.state.lock().unwrap().sources = rules.iter().map(|r| (r.id, r.source())).collect(),
            Err(e) => tracing::error!("Sync-Regeln laden: {e}"),
        }
    }

    fn changed(&self, rel: &str) {
        let mut st = self.state.lock().unwrap();
        for (id, sub) in affected(&st.sources, rel) {
            mark(&mut st, id, sub, false);
        }
    }

    fn mark_all(&self) {
        let mut st = self.state.lock().unwrap();
        let ids: Vec<i64> = st.sources.iter().map(|(id, _)| *id).collect();
        for id in ids {
            mark(&mut st, id, String::new(), false);
        }
    }

    /// Sofort komplett abgleichen (neue Regel, „Jetzt synchronisieren“)
    fn mark_now(&self, id: i64) {
        let mut st = self.state.lock().unwrap();
        st.retry_at.remove(&id);
        mark(&mut st, id, String::new(), true);
    }

    fn start_due(self: &Arc<Self>) {
        if self.maintenance.load(Ordering::SeqCst) {
            return;
        }
        let due: Vec<(i64, String)> = {
            let mut st = self.state.lock().unwrap();
            let ready: Vec<i64> = st
                .dirty
                .iter()
                .filter(|(id, d)| {
                    !st.running.contains(id)
                        && st.retry_at.get(id).is_none_or(|t| Instant::now() >= *t)
                        && (d.now || d.last.elapsed() >= QUIET || d.since.elapsed() >= MAX_WAIT)
                })
                .map(|(id, _)| *id)
                .collect();
            ready
                .into_iter()
                .filter_map(|id| {
                    let d = st.dirty.remove(&id)?;
                    st.running.insert(id);
                    Some((id, d.sub))
                })
                .collect()
        };
        for (id, sub) in due {
            let me = self.clone();
            tokio::spawn(async move {
                let _permit = me.limit.clone().acquire_owned().await;
                let result = me.run(id, &sub).await;
                let mut st = me.state.lock().unwrap();
                st.running.remove(&id);
                if let Err(e) = result {
                    tracing::warn!("Sync-Regel {id}: {e}");
                    // später erneut, dann sicherheitshalber alles
                    st.retry_at.insert(id, Instant::now() + RETRY);
                    mark(&mut st, id, String::new(), false);
                } else {
                    st.retry_at.remove(&id);
                }
            });
        }
    }

    /// Ein Abgleich. `sub`: geänderter Unterordner der Quelle ("" = alles).
    async fn run(&self, id: i64, sub: &str) -> Result<(), String> {
        let loaded = self
            .db
            .call(move |c| {
                let rule = load_rule(c, id)?;
                let target = backup::load_target(c, rule.target_id)?;
                Ok((rule, target))
            })
            .await;
        let Ok((rule, target)) = loaded else { return Ok(()) }; // inzwischen gelöscht
        if !rule.enabled {
            return Ok(());
        }
        let started = db::now();
        let result = self.transfer(&rule, &target, sub).await;
        let (files, bytes) = *result.as_ref().unwrap_or(&(0, 0));
        let error = result.as_ref().err().cloned();
        let _ = self
            .db
            .call(move |c| {
                match &error {
                    None => c.execute(
                        "UPDATE sync_rules SET last_run_at = ?1, last_ok_at = ?2, last_error = NULL, files = ?3, bytes = ?4 WHERE id = ?5",
                        params![started, db::now(), files, bytes, id],
                    )?,
                    Some(e) => c.execute(
                        "UPDATE sync_rules SET last_run_at = ?1, last_error = ?2 WHERE id = ?3",
                        params![started, e, id],
                    )?,
                };
                Ok(())
            })
            .await;
        if result.is_ok() {
            if files > 0 {
                tracing::info!("Sync „{}“ → {}: {files} Dateien übertragen", rule.source_label(), target.name);
            }
            self.prune_daily(&rule, &target).await;
        }
        result.map(|_| ())
    }

    async fn transfer(&self, rule: &Rule, target: &Target, sub: &str) -> Result<(i64, i64), String> {
        let root = self.data_dir.join(rule.source());
        if !root.is_dir() {
            return Err(format!("Quellordner „{}“ gibt es nicht (mehr)", rule.source_label()));
        }
        // gelöschte Unterordner: den nächsten noch vorhandenen darüber abgleichen
        let mut sub = sub.to_owned();
        while !sub.is_empty() && !root.join(&sub).is_dir() {
            sub = sub.rsplit_once('/').map_or(String::new(), |(p, _)| p.to_owned());
        }
        let env = backup::rclone_env(target).await?;
        if sub.is_empty() && is_empty_dir(&root) && !remote_empty(target, &rule.dest, &env).await? {
            return Err(format!(
                "„{}“ ist leer, auf dem NAS liegen aber Dateien. Zum Schutz wurde nichts gelöscht.",
                rule.source_label()
            ));
        }
        let join = |a: &str, b: &str| if b.is_empty() { a.to_owned() } else { format!("{a}/{b}") };
        let stamp = Local::now().format("%Y-%m-%d_%H%M%S").to_string();
        let dest = target.remote_in_share(&join(&rule.dest, &sub));
        let versions = target.remote_in_share(&join(&format!("{}/{VERSIONS_DIR}/{stamp}", rule.dest), &sub));
        let mut args: Vec<String> = vec!["sync".into(), root.join(&sub).to_string_lossy().into_owned(), dest];
        args.extend(
            [
                "--backup-dir", &versions,
                "--exclude", ".upload-*.part",
                "--exclude", ".DS_Store",
                "--exclude", "._*",
                "--create-empty-src-dirs",
                "--transfers", "4",
                "--use-json-log",
                "--stats", "30s",
                "--stats-log-level", "NOTICE",
            ]
            .map(String::from),
        );
        if sub.is_empty() {
            // Versionen liegen im Zielordner und dürfen nicht mitgespiegelt werden
            args.extend(["--exclude", "/.versionen/**"].map(String::from));
        }
        run_rclone(&args, &env).await
    }

    /// Alte Versionen höchstens einmal am Tag aufräumen.
    async fn prune_daily(&self, rule: &Rule, target: &Target) {
        if rule.keep_days <= 0 {
            return;
        }
        {
            let mut st = self.state.lock().unwrap();
            if st.pruned.get(&rule.id).is_some_and(|t| t.elapsed() < Duration::from_secs(86400)) {
                return;
            }
            st.pruned.insert(rule.id, Instant::now());
        }
        let Ok(env) = backup::rclone_env(target).await else { return };
        let base = target.remote_in_share(&format!("{}/{VERSIONS_DIR}", rule.dest));
        let Ok(out) = backup::rclone_output(&["lsf", "--dirs-only", &base], &env).await else { return };
        let cutoff = (Local::now() - chrono::Duration::days(rule.keep_days)).format("%Y-%m-%d_%H%M%S").to_string();
        for dir in out.lines().map(|l| l.trim_end_matches('/')) {
            if dir.len() == cutoff.len() && dir < cutoff.as_str() {
                let _ = backup::rclone_output(&["purge", &format!("{base}/{dir}")], &env).await;
            }
        }
    }

    fn status_of(&self, id: i64) -> (&'static str, Option<i64>) {
        let st = self.state.lock().unwrap();
        if st.running.contains(&id) {
            ("running", None)
        } else if let Some(d) = st.dirty.get(&id) {
            let waited = d.since.elapsed().as_secs() as i64;
            ("pending", Some(db::now() - waited))
        } else {
            ("idle", None)
        }
    }
}

/// Welche Regeln betrifft eine Änderung, und welcher Unterordner ihrer Quelle?
fn affected(sources: &[(i64, String)], rel: &str) -> Vec<(i64, String)> {
    sources
        .iter()
        .filter_map(|(id, src)| {
            if let Some(inner) = rel.strip_prefix(src.as_str()).and_then(|r| r.strip_prefix('/')) {
                // geänderte Datei -> ihr Ordner
                Some((*id, inner.rsplit_once('/').map_or("", |(dir, _)| dir).to_owned()))
            } else if src.starts_with(&format!("{rel}/")) || src == rel {
                // Quellordner selbst (oder ein Ordner darüber) umbenannt/gelöscht
                Some((*id, String::new()))
            } else {
                None
            }
        })
        .collect()
}

fn mark(st: &mut SyncState, id: i64, sub: String, now: bool) {
    let t = Instant::now();
    match st.dirty.get_mut(&id) {
        Some(d) => {
            d.sub = common_dir(&d.sub, &sub);
            d.last = t;
            d.now |= now;
        }
        None => {
            st.dirty.insert(id, Dirty { since: t, last: t, sub, now });
        }
    }
}

/// Gemeinsamer Oberordner zweier relativer Ordner ("" = ganz oben)
fn common_dir(a: &str, b: &str) -> String {
    if a.is_empty() || b.is_empty() {
        return String::new();
    }
    a.split('/').zip(b.split('/')).take_while(|(x, y)| x == y).map(|(x, _)| x).collect::<Vec<_>>().join("/")
}

fn is_empty_dir(p: &Path) -> bool {
    std::fs::read_dir(p).map(|mut d| d.next().is_none()).unwrap_or(false)
}

/// Liegt im Zielordner (außer alten Versionen) schon etwas?
async fn remote_empty(target: &Target, dest: &str, env: &[(String, String)]) -> Result<bool, String> {
    let remote = target.remote_in_share(dest);
    // nur ein Versuch: beim Anlegen soll ein ausgeschaltetes NAS schnell auffallen
    let args = ["lsf", "--max-depth", "1", "--exclude", "/.versionen/**", "--retries", "1", "--low-level-retries", "1", &remote];
    match backup::rclone_output(&args, env).await {
        Ok(out) => Ok(out.lines().all(|l| l.trim().is_empty() || l.starts_with(VERSIONS_DIR))),
        // Ordner gibt es noch nicht
        Err(e) if e.contains("not found") || e.contains("directory not found") => Ok(true),
        Err(e) => Err(e),
    }
}

/// rclone ausführen; (übertragene Dateien, Bytes) oder die letzte Fehlermeldung.
async fn run_rclone(args: &[String], env: &[(String, String)]) -> Result<(i64, i64), String> {
    let mut child = Command::new("rclone")
        .args(args)
        .envs(env.iter().cloned())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| format!("rclone nicht verfügbar: {e}"))?;
    let mut lines = BufReader::new(child.stderr.take().unwrap()).lines();
    let (mut last, mut error) = ((0, 0), None);
    while let Ok(Some(line)) = lines.next_line().await {
        let Ok(v) = serde_json::from_str::<Value>(&line) else { continue };
        if let Some(s) = v.get("stats") {
            let n = |k: &str| s.get(k).and_then(Value::as_i64).unwrap_or(0);
            last = (n("transfers"), n("bytes"));
        } else if v.get("level").and_then(Value::as_str) == Some("error") {
            error = v.get("msg").and_then(Value::as_str).map(|m| m.trim().to_owned());
        }
    }
    let status = child.wait().await.map_err(|e| e.to_string())?;
    if status.success() {
        Ok(last)
    } else {
        Err(backup::friendly_error(&error.unwrap_or_else(|| format!("rclone beendet mit {status}"))))
    }
}

// ---------- Prüfungen ----------

/// Unterordner prüfen und vereinheitlichen ("a\b/" -> "a/b")
fn clean_sub(s: &str) -> Result<String, AppError> {
    let s = s.trim().replace('\\', "/");
    let parts: Vec<&str> = s.split('/').filter(|p| !p.is_empty()).collect();
    if parts.iter().any(|p| !paths::valid_name(p)) {
        return Err(AppError::BadRequest(format!("Ungültiger Ordner: {s}")));
    }
    Ok(parts.join("/"))
}

/// Belegt ein anderer Sync oder ein Backup diesen Ordner auf demselben NAS?
fn check_overlap(c: &Connection, target: &Target, dest: &str, except_rule: Option<i64>) -> Result<(), AppError> {
    let norm = |s: &str| s.trim_matches('/').to_lowercase();
    let overlaps = |a: &str, b: &str| a == b || a.starts_with(&format!("{b}/")) || b.starts_with(&format!("{a}/"));
    let d = norm(dest);
    let same_share = "SELECT id FROM backup_targets WHERE lower(host) = lower(?1) AND lower(share) = lower(?2)";
    let ids: Vec<i64> = c.prepare(same_share)?.query_map(params![target.host, target.share], |r| r.get(0))?.collect::<Result<_, _>>()?;
    for tid in ids {
        let t = backup::load_target(c, tid)?;
        // Ordner, in die das Backup dieses Ziels schreibt
        for area in [paths::SHARED, paths::PRIVATE, backup::DB_DIR, VERSIONS_DIR] {
            let used = norm(&if t.folder.is_empty() { area.to_owned() } else { format!("{}/{area}", t.folder) });
            if overlaps(&d, &used) {
                return Err(AppError::Conflict(format!(
                    "„{dest}“ überschneidet sich mit dem Backup „{}“ ({used}). Bitte einen anderen Zielordner wählen.",
                    t.name
                )));
            }
        }
        let rules: Vec<(i64, String)> = c
            .prepare("SELECT id, dest FROM sync_rules WHERE target_id = ?")?
            .query_map([tid], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<Result<_, _>>()?;
        for (rid, other) in rules {
            if Some(rid) != except_rule && overlaps(&d, &norm(&other)) {
                return Err(AppError::Conflict(format!(
                    "In „{other}“ auf diesem NAS synchronisiert schon eine andere Regel. Zwei Regeln im selben Ordner würden sich gegenseitig Dateien löschen."
                )));
            }
        }
    }
    Ok(())
}

// ---------- HTTP ----------

fn rule_json(r: &Rule, target: &Target, me: &CurrentUser, syncer: &Syncer) -> Value {
    let (running, pending_since) = syncer.status_of(r.id);
    let status = if !r.enabled {
        "paused"
    } else if running != "idle" {
        running
    } else if r.last_error.is_some() {
        "error"
    } else if r.last_ok_at.is_some() {
        "ok"
    } else {
        "new"
    };
    json!({
        "id": r.id,
        "target_id": r.target_id,
        "target_name": target.name,
        "location": format!("\\\\{}\\{}\\{}", target.host, target.share, r.dest.replace('/', "\\")),
        "source_space": r.source_space,
        "source_user": r.source_user,
        "source_sub": r.source_sub,
        "source_label": r.source_label(),
        "dest": r.dest,
        "by_admin": r.by_admin,
        "can_edit": me.admin || (!r.by_admin && r.source_user_id == Some(me.id)),
        "keep_days": r.keep_days,
        "enabled": r.enabled,
        "status": status,
        "pending_since": pending_since,
        "last_run_at": r.last_run_at,
        "last_ok_at": r.last_ok_at,
        "last_error": r.last_error,
        "files": r.files,
        "bytes": r.bytes,
    })
}

pub async fn list(State(state): State<AppState>, Extension(me): Extension<CurrentUser>) -> Result<Json<Value>, AppError> {
    let me2 = me.clone();
    let (rules, targets, users) = state
        .db
        .call(move |c| {
            // Benutzer sehen nur Regeln für ihren eigenen Privat-Ordner
            let filter = if me2.admin { String::new() } else { format!(" WHERE r.source_space = 'Privat' AND r.source_user_id = {}", me2.id) };
            let rules: Vec<Rule> = c
                .prepare(&format!("{RULE_SQL}{filter} ORDER BY r.source_space, u.name, r.source_sub"))?
                .query_map([], read_rule)?
                .collect::<Result<_, _>>()?;
            let mut targets = HashMap::new();
            for r in &rules {
                if let std::collections::hash_map::Entry::Vacant(e) = targets.entry(r.target_id) {
                    e.insert(backup::load_target(c, r.target_id)?);
                }
            }
            // NAS, die man als Ziel wählen darf (Admin: alle)
            // Admin: alle (die Bedingung nutzt ?1 trotzdem, damit die Parameter passen)
            let filter = if me2.admin { "?1 IS NOT NULL" } else { ALLOWED };
            let selectable: Vec<Value> = c
                .prepare(&format!("SELECT t.id, t.name, t.host, t.share, t.folder FROM backup_targets t WHERE {filter} ORDER BY t.name"))?
                .query_map([me2.id], |r| {
                    Ok(json!({
                        "id": r.get::<_, i64>(0)?,
                        "name": r.get::<_, String>(1)?,
                        "location": format!("\\\\{}\\{}", r.get::<_, String>(2)?, r.get::<_, String>(3)?),
                        "folder": r.get::<_, String>(4)?,
                    }))
                })?
                .collect::<Result<Vec<_>, _>>()?;
            let users: Vec<String> = if me2.admin {
                c.prepare("SELECT name FROM users ORDER BY name")?.query_map([], |r| r.get(0))?.collect::<Result<_, _>>()?
            } else {
                Vec::new()
            };
            Ok((rules, (targets, selectable), users))
        })
        .await?;
    let (targets, selectable) = targets;
    let rules: Vec<Value> = rules.iter().map(|r| rule_json(r, &targets[&r.target_id], &me, &state.sync)).collect();
    Ok(Json(json!({ "rules": rules, "targets": selectable, "users": users })))
}

#[derive(Deserialize)]
pub struct CreateBody {
    target_id: i64,
    /// "shared" oder "private"
    source: String,
    /// Benutzer, dessen Privat-Ordner (nur Admin; sonst immer man selbst)
    user: Option<String>,
    #[serde(default)]
    sub: String,
    /// Ordner in der Freigabe (nur Admin)
    dest: Option<String>,
    #[serde(default = "thirty")]
    keep_days: i64,
    /// Zielordner ist nicht leer – trotzdem anlegen
    #[serde(default)]
    confirm_nonempty: bool,
}

fn thirty() -> i64 {
    30
}

pub async fn create(
    State(state): State<AppState>,
    Extension(me): Extension<CurrentUser>,
    Json(b): Json<CreateBody>,
) -> Result<Json<Value>, AppError> {
    let sub = clean_sub(&b.sub)?;
    if !(0..=3650).contains(&b.keep_days) {
        return Err(AppError::BadRequest("Ungültige Anzahl Tage".into()));
    }
    let (space, user) = match b.source.as_str() {
        "shared" if me.admin => (paths::SHARED, None),
        "private" => {
            let u = if me.admin { b.user.clone().unwrap_or_else(|| me.name.clone()) } else { me.name.clone() };
            (paths::PRIVATE, Some(u))
        }
        _ => return Err(AppError::Forbidden),
    };
    let (me2, sub2, user2) = (me.clone(), sub.clone(), user.clone());
    let (target, user_id, dest) = state
        .db
        .call(move |c| {
            let target = backup::load_target(c, b.target_id)?;
            let allowed: bool = me2.admin
                || c.query_row(&format!("SELECT count(*) > 0 FROM backup_targets t WHERE t.id = ?2 AND {ALLOWED}"), params![me2.id, b.target_id], |r| r.get(0))?;
            if !allowed {
                return Err(AppError::Forbidden);
            }
            let user_id: Option<i64> = match &user2 {
                Some(u) => Some(c.query_row("SELECT id FROM users WHERE name = ?", [u], |r| r.get(0)).optional()?.ok_or(AppError::NotFound)?),
                None => None,
            };
            let dest = if me2.admin {
                clean_sub(b.dest.as_deref().unwrap_or(""))?
            } else {
                // Benutzer: fester Ordner pro Name, damit niemand fremde Daten überschreibt
                let base = if target.folder.is_empty() { USER_DIR.to_owned() } else { format!("{}/{USER_DIR}", target.folder) };
                let mut d = format!("{base}/{}", me2.name);
                if !sub2.is_empty() {
                    d = format!("{d}/{sub2}");
                }
                d
            };
            if dest.is_empty() {
                return Err(AppError::BadRequest("Bitte einen Zielordner auf dem NAS angeben (nicht die ganze Freigabe)".into()));
            }
            check_overlap(c, &target, &dest, None)?;
            Ok((target, user_id, dest))
        })
        .await?;

    let source_dir = {
        let mut p = state.data_dir.join(space);
        if let Some(u) = &user {
            p = p.join(u);
        }
        if !sub.is_empty() {
            p = p.join(&sub);
        }
        p
    };
    if !source_dir.is_dir() {
        return Err(AppError::BadRequest(format!("Den Ordner „{sub}“ gibt es nicht")));
    }
    // Verbindung prüfen und vor fremden Dateien im Zielordner warnen
    let env = backup::rclone_env(&target).await.map_err(AppError::Internal)?;
    let empty = remote_empty(&target, &dest, &env).await.map_err(|e| AppError::BadRequest(format!("NAS „{}“: {e}", target.name)))?;
    if !empty && !b.confirm_nonempty {
        return Err(AppError::Conflict(format!(
            "DEST_NOT_EMPTY: Im Ordner „{dest}“ auf „{}“ liegen schon Dateien. Was davon nicht in homevault ist, wird nach „{dest}/.versionen“ verschoben.",
            target.name
        )));
    }

    let by_admin = me.admin;
    let (uid, d2, tid, keep) = (me.id, dest.clone(), b.target_id, b.keep_days);
    let id = state
        .db
        .call(move |c| {
            c.execute(
                "INSERT INTO sync_rules (target_id, source_space, source_user_id, source_sub, dest, created_by, by_admin, keep_days, created_at)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
                params![tid, space, user_id, sub, d2, uid, by_admin, keep, db::now()],
            )?;
            Ok(c.last_insert_rowid())
        })
        .await?;
    tracing::info!("Sync-Regel {id} angelegt von '{}' → {} {dest}", me.name, target.name);
    state.sync.reload().await;
    state.sync.mark_now(id);
    Ok(Json(json!({ "id": id })))
}

/// Darf `me` diese Regel ändern?
async fn editable(state: &AppState, me: &CurrentUser, id: i64) -> Result<(), AppError> {
    let (by_admin, owner): (bool, Option<i64>) = state
        .db
        .call(move |c| {
            Ok(c.query_row("SELECT by_admin, source_user_id FROM sync_rules WHERE id = ?", [id], |r| Ok((r.get(0)?, r.get(1)?)))
                .optional()?
                .ok_or(AppError::NotFound)?)
        })
        .await?;
    if me.admin || (!by_admin && owner == Some(me.id)) {
        Ok(())
    } else if owner == Some(me.id) {
        Err(AppError::BadRequest("Diese Regel hat der Admin angelegt".into()))
    } else {
        Err(AppError::NotFound)
    }
}

#[derive(Deserialize)]
pub struct UpdateBody {
    enabled: Option<bool>,
    keep_days: Option<i64>,
}

pub async fn update(
    State(state): State<AppState>,
    Extension(me): Extension<CurrentUser>,
    UrlPath(id): UrlPath<i64>,
    Json(b): Json<UpdateBody>,
) -> Result<Json<Value>, AppError> {
    editable(&state, &me, id).await?;
    if b.keep_days.is_some_and(|d| !(0..=3650).contains(&d)) {
        return Err(AppError::BadRequest("Ungültige Anzahl Tage".into()));
    }
    state
        .db
        .call(move |c| {
            if let Some(e) = b.enabled {
                c.execute("UPDATE sync_rules SET enabled = ? WHERE id = ?", params![e, id])?;
            }
            if let Some(k) = b.keep_days {
                c.execute("UPDATE sync_rules SET keep_days = ? WHERE id = ?", params![k, id])?;
            }
            Ok(())
        })
        .await?;
    state.sync.reload().await;
    if b.enabled == Some(true) {
        state.sync.mark_now(id);
    }
    Ok(Json(json!({ "ok": true })))
}

pub async fn delete(
    State(state): State<AppState>,
    Extension(me): Extension<CurrentUser>,
    UrlPath(id): UrlPath<i64>,
) -> Result<Json<Value>, AppError> {
    editable(&state, &me, id).await?;
    // Die Dateien auf dem NAS bleiben liegen
    state.db.call(move |c| Ok(c.execute("DELETE FROM sync_rules WHERE id = ?", [id])?)).await?;
    state.sync.reload().await;
    state.sync.state.lock().unwrap().dirty.remove(&id);
    Ok(Json(json!({ "ok": true })))
}

pub async fn run_now(
    State(state): State<AppState>,
    Extension(me): Extension<CurrentUser>,
    UrlPath(id): UrlPath<i64>,
) -> Result<Json<Value>, AppError> {
    // ansehen reicht: auch vom Admin angelegte Regeln darf der Benutzer anstoßen
    let owner: Option<i64> = state
        .db
        .call(move |c| Ok(c.query_row("SELECT source_user_id FROM sync_rules WHERE id = ?", [id], |r| r.get(0)).optional()?.ok_or(AppError::NotFound)?))
        .await?;
    if !me.admin && owner != Some(me.id) {
        return Err(AppError::NotFound);
    }
    state.sync.mark_now(id);
    Ok(Json(json!({ "ok": true })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn common_parent_folder() {
        assert_eq!(common_dir("a/b/c", "a/b/d"), "a/b");
        assert_eq!(common_dir("a/b", "a/b/c"), "a/b");
        assert_eq!(common_dir("a", "b"), "");
        assert_eq!(common_dir("", "a/b"), "");
    }

    #[test]
    fn sub_folder_is_cleaned() {
        assert_eq!(clean_sub(" Fotos\\2024/ ").unwrap(), "Fotos/2024");
        assert_eq!(clean_sub("").unwrap(), "");
        assert!(clean_sub("../etc").is_err());
        assert!(clean_sub("a/.versionen").is_err());
    }

    #[test]
    fn changes_mark_the_right_rule_and_folder() {
        let mut st = SyncState { sources: vec![(1, "Gemeinsam/Fotos".into()), (2, "Privat/anna".into())], ..Default::default() };
        for rel in ["Gemeinsam/Fotos/2024/Juli/a.jpg", "Gemeinsam/Fotos/2024/Mai/b.jpg", "Gemeinsam/Fotosammlung/x.jpg"] {
            for (id, sub) in affected(&st.sources.clone(), rel) {
                mark(&mut st, id, sub, false);
            }
        }
        assert_eq!(st.dirty[&1].sub, "2024");
        assert!(!st.dirty.contains_key(&2));
        assert_eq!(affected(&st.sources, "Privat/anna/brief.pdf"), vec![(2, String::new())]);
        // Quellordner selbst gelöscht -> alles prüfen
        assert_eq!(affected(&st.sources, "Gemeinsam"), vec![(1, String::new())]);
    }
}
