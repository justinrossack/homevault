//! Festplatten-Verwaltung: Spiegel (Btrfs-RAID) anlegen, überwachen und
//! Platten im laufenden Betrieb hinzufügen, tauschen oder entfernen.
//!
//! Aufgeteilt in zwei Prozesse:
//! - Der **Platten-Dienst** (`homevault disk-agent`) läuft in einem eigenen
//!   Container mit Zugriff auf die Festplatten. Nur er führt mkfs, mount,
//!   btrfs und smartctl aus. Er hängt den Speicher beim Start ein, auch
//!   wenn eine Platte fehlt (Notbetrieb), und prüft monatlich alle Daten.
//! - Die **Web-App** bleibt ohne Sonderrechte und leitet Admin-Anfragen an
//!   den Dienst weiter (internes Netz, geheimes Token).
//!
//! Warum Btrfs: Prüfsummen für alle Daten. Ist eine Kopie still beschädigt,
//! wird sie beim Lesen oder bei der Prüfung aus dem Spiegel repariert.
//! Platten unterschiedlicher Größe lassen sich mischen.

use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    process::Command,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use axum::{
    Extension, Json, Router,
    extract::{Path as UrlPath, Request, State},
    http::{Method, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{AppState, auth::CurrentUser, db, error::AppError};

/// Alle 30 Tage alle Daten auf stille Fehler prüfen (Scrub)
const SCRUB_EVERY_DAYS: i64 = 30;
/// Kleinere Geräte sind keine Datenplatten (USB-Sticks, Kartenleser …)
const MIN_DISK_SIZE: u64 = 512 * 1024 * 1024;

// =====================================================================
// Platten-Dienst (läuft privilegiert im eigenen Container)
// =====================================================================

#[derive(Serialize, Deserialize, Clone, Default)]
struct PoolConfig {
    uuid: String,
    /// raid1 | raid1c3 | single
    profile: String,
    created_at: i64,
    last_scrub: Option<i64>,
}

#[derive(Serialize, Clone)]
struct Job {
    kind: String,
    message: String,
    started_at: i64,
}

struct Agent {
    data: PathBuf,
    state_dir: PathBuf,
    token: String,
    /// Test-Modus: Loop-Geräte mit Abbild unter diesem Ordner gelten als Platten
    test_loop_dir: Option<String>,
    job: Mutex<Option<Job>>,
    last_error: Mutex<Option<String>>,
    ready: AtomicBool,
}

/// Befehl ausführen; stdout bei Erfolg, sonst letzte Fehlerzeile.
fn run(cmd: &str, args: &[&str]) -> Result<String, String> {
    let out = Command::new(cmd).args(args).output().map_err(|e| format!("{cmd}: {e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        let err = String::from_utf8_lossy(&out.stderr);
        let line = err.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("unbekannter Fehler");
        Err(format!("{cmd}: {}", line.trim()))
    }
}

impl Agent {
    fn config_path(&self) -> PathBuf {
        self.state_dir.join("pool.json")
    }

    fn config(&self) -> Option<PoolConfig> {
        std::fs::read(self.config_path()).ok().and_then(|b| serde_json::from_slice(&b).ok())
    }

    fn save_config(&self, c: &PoolConfig) -> Result<(), String> {
        let tmp = self.config_path().with_extension("tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(c).unwrap()).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, self.config_path()).map_err(|e| e.to_string())
    }

    /// Ist unser Speicher auf /data eingehängt? Liefert (Gerät, Optionen).
    fn mounted(&self) -> Option<(String, String)> {
        let info = std::fs::read_to_string("/proc/self/mountinfo").ok()?;
        info.lines().rev().find_map(|l| {
            let (left, right) = l.split_once(" - ")?;
            let target = left.split_whitespace().nth(4)?;
            let mut r = right.split_whitespace();
            let (fstype, source, opts) = (r.next()?, r.next()?, r.next().unwrap_or(""));
            (target == self.data.to_str()? && fstype == "btrfs").then(|| (source.to_owned(), opts.to_owned()))
        })
    }

    /// Beim Start: Speicher einhängen, notfalls im Notbetrieb (fehlende Platte).
    fn mount_at_boot(&self) {
        let Some(cfg) = self.config() else {
            tracing::info!("Platten-Dienst: kein Speicherverbund eingerichtet");
            self.ready.store(true, Ordering::SeqCst);
            return;
        };
        if self.mounted().is_some() {
            self.ready.store(true, Ordering::SeqCst);
            return;
        }
        let spec = format!("UUID={}", cfg.uuid);
        // Platten brauchen nach dem Einschalten manchmal einen Moment. Klappt
        // es gar nicht, meldet sich der Dienst trotzdem bereit (die Web-App
        // startet dann schreibgeschützt mit Warnung) und versucht es weiter.
        for attempt in 1.. {
            let _ = run("btrfs", &["device", "scan"]);
            match run("mount", &["-o", "noatime", &spec, self.data_str()]) {
                Ok(_) => {
                    tracing::info!("Speicherverbund eingehängt");
                    *self.last_error.lock().unwrap() = None;
                    self.ready.store(true, Ordering::SeqCst);
                    return;
                }
                Err(e) if attempt >= 6 => {
                    // nach ~30 s: mit fehlender Platte im Notbetrieb starten
                    if run("mount", &["-o", "noatime,degraded", &spec, self.data_str()]).is_ok() {
                        tracing::warn!("Speicherverbund im NOTBETRIEB eingehängt: eine Platte fehlt!");
                        *self.last_error.lock().unwrap() = None;
                        self.ready.store(true, Ordering::SeqCst);
                        return;
                    }
                    if attempt == 6 || attempt % 60 == 0 {
                        tracing::error!("Speicherverbund kann nicht eingehängt werden ({e}), versuche es weiter");
                    }
                    *self.last_error.lock().unwrap() = Some(format!("Speicher kann nicht eingehängt werden: {e}"));
                    self.ready.store(true, Ordering::SeqCst);
                }
                Err(_) => {}
            }
            std::thread::sleep(Duration::from_secs(5));
        }
    }

    /// Kurzer, öffentlicher Zustand für die Gesundheitsprüfung, die Web-App
    /// und Samba (ohne Token).
    fn health(&self) -> Value {
        let job = self.job.lock().unwrap().as_ref().map(|j| j.kind.clone());
        json!({
            "ready": self.ready.load(Ordering::SeqCst),
            "pool": self.config().is_some(),
            "mounted": self.mounted().is_some(),
            "job": job,
        })
    }

    /// Geräte, die auf dem Server selbst eingehängt oder Swap sind (Systemplatte!).
    fn host_busy_devices() -> HashSet<String> {
        let mut busy = HashSet::new();
        // /proc/1 ist mit `pid: host` der Init-Prozess des Servers
        for file in ["/proc/1/mountinfo", "/proc/self/mountinfo"] {
            if let Ok(info) = std::fs::read_to_string(file) {
                for l in info.lines() {
                    if let Some((_, right)) = l.split_once(" - ") {
                        if let Some(src) = right.split_whitespace().nth(1) {
                            if src.starts_with("/dev/") {
                                busy.insert(std::fs::canonicalize(src).map_or(src.to_owned(), |p| p.to_string_lossy().into_owned()));
                            }
                        }
                    }
                }
            }
        }
        if let Ok(swaps) = std::fs::read_to_string("/proc/swaps") {
            for l in swaps.lines().skip(1) {
                if let Some(dev) = l.split_whitespace().next() {
                    busy.insert(dev.to_owned());
                }
            }
        }
        busy
    }

    fn smart(path: &str) -> Value {
        let Ok(out) = Command::new("smartctl").args(["-j", "-H", "-A", "-i", path]).output() else { return Value::Null };
        let Ok(v) = serde_json::from_slice::<Value>(&out.stdout) else { return Value::Null };
        if v["smart_status"].is_null() {
            return Value::Null;
        }
        let attr = |id: i64| {
            v["ata_smart_attributes"]["table"]
                .as_array()
                .and_then(|t| t.iter().find(|a| a["id"].as_i64() == Some(id)))
                .and_then(|a| a["raw"]["value"].as_i64())
        };
        json!({
            "passed": v["smart_status"]["passed"].as_bool(),
            "temperature": v["temperature"]["current"].as_i64(),
            "power_on_hours": v["power_on_time"]["hours"].as_i64(),
            // 5 = neu zugewiesene Sektoren, 197 = wartende Sektoren: steigen sie, stirbt die Platte bald
            "reallocated": attr(5),
            "pending": attr(197),
        })
    }

    /// Alle Platten mit Hinweis, ob sie für den Speicher verwendet werden können.
    /// Geräte, die gerade zum Speicherverbund gehören (laut btrfs).
    fn pool_paths(&self) -> HashSet<String> {
        let Some(cfg) = self.config() else { return HashSet::new() };
        run("btrfs", &["filesystem", "show", "--raw", &cfg.uuid])
            .unwrap_or_default()
            .lines()
            .filter(|l| l.trim_start().starts_with("devid") && !l.to_lowercase().contains("missing"))
            .filter_map(|l| l.split_whitespace().skip_while(|p| *p != "path").nth(1).map(str::to_owned))
            .collect()
    }

    fn disks(&self, pool_uuid: Option<&str>) -> Vec<Value> {
        let members = self.pool_paths();
        let Ok(out) = run(
            "lsblk",
            &["-J", "-b", "-o", "NAME,PATH,TYPE,SIZE,MODEL,SERIAL,ROTA,TRAN,FSTYPE,UUID,HOTPLUG,LABEL"],
        ) else {
            return Vec::new();
        };
        let Ok(tree) = serde_json::from_str::<Value>(&out) else { return Vec::new() };
        let busy = Self::host_busy_devices();
        let loop_files: HashMap<String, String> = run("losetup", &["-n", "-O", "NAME,BACK-FILE"])
            .unwrap_or_default()
            .lines()
            .filter_map(|l| l.split_once(char::is_whitespace).map(|(a, b)| (a.to_owned(), b.trim().to_owned())))
            .collect();

        let mut disks = Vec::new();
        for d in tree["blockdevices"].as_array().into_iter().flatten() {
            let path = d["path"].as_str().unwrap_or_default().to_owned();
            let kind = d["type"].as_str().unwrap_or_default();
            let size = d["size"].as_u64().unwrap_or(0);
            let is_test_loop = kind == "loop"
                && self.test_loop_dir.as_deref().is_some_and(|dir| loop_files.get(&path).is_some_and(|f| f.starts_with(dir)));
            if !(kind == "disk" || is_test_loop) || size < MIN_DISK_SIZE {
                continue;
            }
            // Platte selbst und alle Partitionen/Unterbaugeräte einsammeln
            let mut all = vec![d.clone()];
            let mut i = 0;
            while i < all.len() {
                let kids = all[i]["children"].as_array().cloned().unwrap_or_default();
                all.extend(kids);
                i += 1;
            }
            // Mitglied nur, wenn btrfs das Gerät wirklich verwendet. Eine früher
            // entfernte Platte trägt noch dieselbe Kennung, gehört aber nicht mehr dazu.
            let in_pool = all.iter().any(|n| n["path"].as_str().is_some_and(|p| members.contains(p)));
            let old_member = !in_pool && pool_uuid.is_some_and(|u| all.iter().any(|n| n["uuid"].as_str() == Some(u)));
            let fstypes: Vec<String> = all.iter().filter_map(|n| n["fstype"].as_str().map(str::to_owned)).collect();
            let reason = if in_pool {
                None
            } else if all.iter().any(|n| n["path"].as_str().is_some_and(|p| busy.contains(p))) {
                Some("Wird vom System verwendet (z. B. Systemplatte) und ist gesperrt")
            } else if all.iter().skip(1).any(|n| matches!(n["type"].as_str(), Some("lvm" | "crypt" | "md" | "raid0" | "raid1" | "raid5" | "raid6" | "raid10"))) {
                Some("Gehört zu einem anderen Speicherverbund (LVM/RAID/verschlüsselt)")
            } else {
                None
            };
            disks.push(json!({
                "path": path,
                "name": d["name"],
                "size": size,
                "model": d["model"].as_str().map(str::trim).filter(|s| !s.is_empty()).unwrap_or(if is_test_loop { "Test-Platte" } else { "Unbekannt" }),
                "serial": d["serial"],
                "ssd": d["rota"].as_bool() == Some(false) || d["rota"].as_str() == Some("0"),
                "transport": d["tran"],
                "hotplug": d["hotplug"],
                "in_pool": in_pool,
                "usable": !in_pool && reason.is_none(),
                "reason": reason,
                // vorhandene Dateisysteme: werden beim Einrichten gelöscht
                "existing": if in_pool { Vec::new() } else { fstypes },
                "old_member": old_member,
                "smart": if is_test_loop { Value::Null } else { Self::smart(&path) },
            }));
        }
        disks
    }

    fn pool_status(&self, cfg: &PoolConfig) -> Value {
        let mounted = self.mounted();
        // Geräte des Verbunds, auch fehlende
        let show = run("btrfs", &["filesystem", "show", "--raw", &cfg.uuid]).unwrap_or_default();
        let mut devices = Vec::new();
        for l in show.lines().map(str::trim).filter(|l| l.starts_with("devid")) {
            let parts: Vec<&str> = l.split_whitespace().collect();
            let get = |key: &str| parts.iter().position(|p| *p == key).and_then(|i| parts.get(i + 1)).copied();
            let missing = l.to_lowercase().contains("missing");
            devices.push(json!({
                "devid": get("devid").and_then(|v| v.parse::<i64>().ok()),
                "size": get("size").and_then(|v| v.parse::<u64>().ok()),
                "used": get("used").and_then(|v| v.parse::<u64>().ok()),
                "path": if missing { Value::Null } else { get("path").map(Value::from).unwrap_or(Value::Null) },
                "missing": missing,
            }));
        }
        let some_missing = show.contains("Some devices missing") || devices.iter().any(|d| d["missing"] == true);

        let mut out = json!({
            "uuid": cfg.uuid,
            "profile": cfg.profile,
            "mounted": mounted.is_some(),
            // fehlt eine Platte? (nach einem Tausch ist alles wieder gespiegelt,
            // auch wenn noch mit der Option "degraded" eingehängt)
            "degraded": some_missing,
            "devices": devices,
            "last_scrub": cfg.last_scrub,
        });
        if mounted.is_none() {
            return out;
        }
        let data = self.data.to_str().unwrap();

        // Platz: geschätzt frei unter Berücksichtigung der Spiegelung
        let usage = run("btrfs", &["filesystem", "usage", "-b", data]).unwrap_or_default();
        let num = |key: &str| {
            usage.lines().find(|l| l.trim_start().starts_with(key)).and_then(|l| l.split(':').nth(1)?.split_whitespace().next()?.parse::<u64>().ok())
        };
        let used = num("Used:").unwrap_or(0);
        let free = num("Free (estimated):").unwrap_or(0);
        out["raw_size"] = json!(num("Device size:"));
        out["used"] = json!(used);
        out["free"] = json!(free);
        out["size"] = json!(used + free);

        // tatsächliches Profil der Daten (kann nach Hinzufügen noch "single" sein)
        let df = run("btrfs", &["filesystem", "df", "-b", data]).unwrap_or_default();
        out["data_profile"] = json!(df.lines().find(|l| l.starts_with("Data,")).and_then(|l| l.split(':').next()).map(|p| p.trim_start_matches("Data,").trim().to_lowercase()));

        // Lese-/Schreib-/Prüfsummenfehler je Platte
        let stats = run("btrfs", &["device", "stats", data]).unwrap_or_default();
        let mut errors: HashMap<String, u64> = HashMap::new();
        for l in stats.lines() {
            if let Some((dev, rest)) = l.trim_start_matches('[').split_once("].") {
                let n = rest.split_whitespace().last().and_then(|v| v.parse::<u64>().ok()).unwrap_or(0);
                *errors.entry(dev.to_owned()).or_default() += n;
            }
        }
        if let Some(devs) = out["devices"].as_array_mut() {
            for d in devs {
                let p = d["path"].as_str().map(str::to_owned);
                d["errors"] = json!(p.and_then(|p| errors.get(&p).copied()).unwrap_or(0));
            }
        }

        // laufende Prüfung / Tausch / Umverteilung
        let scrub = run("btrfs", &["scrub", "status", data]).unwrap_or_default();
        let field = |text: &str, key: &str| {
            text.lines().find(|l| l.trim_start().to_lowercase().starts_with(key)).and_then(|l| l.split_once(':')).map(|(_, v)| v.trim().to_owned())
        };
        out["scrub"] = json!({
            "running": field(&scrub, "status").is_some_and(|s| s.contains("running")),
            "status": field(&scrub, "status"),
            "errors": field(&scrub, "error summary"),
        });
        let replace = run("btrfs", &["replace", "status", "-1", data]).unwrap_or_default();
        out["replace"] = json!({
            "running": replace.contains("% done") && !replace.contains("finished"),
            "progress": replace.split('%').next().and_then(|s| s.split_whitespace().last()).and_then(|v| v.parse::<f64>().ok()),
        });
        let balance = run("btrfs", &["balance", "status", data]).unwrap_or_default();
        out["balance"] = json!({
            "running": balance.contains("is running"),
            "left": balance.split('%').next().and_then(|s| s.split_whitespace().last()).and_then(|v| v.parse::<f64>().ok()),
        });
        out
    }

    fn status(&self) -> Value {
        let cfg = self.config();
        let pool = cfg.as_ref().map(|c| self.pool_status(c));
        json!({
            "ready": self.ready.load(Ordering::SeqCst),
            "pool": pool,
            "disks": self.disks(cfg.as_ref().map(|c| c.uuid.as_str())),
            "job": *self.job.lock().unwrap(),
            "last_error": *self.last_error.lock().unwrap(),
        })
    }

    /// Prüft, ob die Geräte frei verwendbar sind und der Name bestätigt wurde.
    fn check_devices(&self, devices: &[String], confirm: &[String]) -> Result<(), String> {
        let disks = self.disks(self.config().as_ref().map(|c| c.uuid.as_str()));
        for dev in devices {
            let d = disks.iter().find(|d| d["path"].as_str() == Some(dev)).ok_or(format!("{dev} wurde nicht gefunden"))?;
            if d["usable"] != true {
                return Err(format!("{dev} kann nicht verwendet werden: {}", d["reason"].as_str().unwrap_or("bereits im Speicher")));
            }
            let name = d["name"].as_str().unwrap_or_default();
            if !confirm.iter().any(|c| c == name) {
                return Err(format!("Bitte den Namen „{name}“ zur Bestätigung eintippen"));
            }
        }
        Ok(())
    }

    /// Lange Vorgänge im Hintergrund, immer nur einer gleichzeitig.
    fn start_job(self: &Arc<Self>, kind: &str, message: &str, work: impl FnOnce(&Agent) -> Result<(), String> + Send + 'static) -> Result<(), String> {
        {
            let mut job = self.job.lock().unwrap();
            if let Some(j) = &*job {
                return Err(format!("Es läuft bereits: {}", j.message));
            }
            *job = Some(Job { kind: kind.into(), message: message.into(), started_at: db::now() });
        }
        *self.last_error.lock().unwrap() = None;
        let me = self.clone();
        let kind = kind.to_owned();
        std::thread::spawn(move || {
            let result = work(&me);
            if let Err(e) = &result {
                tracing::error!("Festplatten ({kind}): {e}");
                *me.last_error.lock().unwrap() = Some(e.clone());
            } else {
                tracing::info!("Festplatten ({kind}) erledigt");
            }
            *me.job.lock().unwrap() = None;
        });
        Ok(())
    }

    fn set_job_message(&self, msg: &str) {
        if let Some(j) = self.job.lock().unwrap().as_mut() {
            j.message = msg.to_owned();
        }
    }

    fn data_str(&self) -> &str {
        self.data.to_str().unwrap()
    }

    // ---------- Vorgänge ----------

    fn create(&self, devices: Vec<String>, profile: String) -> Result<(), String> {
        let (data_p, meta_p) = match (profile.as_str(), devices.len()) {
            ("single", 1) | (_, 1) => ("single", "dup"),
            ("raid1c3", n) if n >= 3 => ("raid1c3", "raid1c3"),
            (_, _) => ("raid1", "raid1"),
        };
        self.set_job_message("Platten werden vorbereitet …");
        for d in &devices {
            run("wipefs", &["-a", d])?;
        }
        let mut args = vec!["-f", "-L", "homevault", "-d", data_p, "-m", meta_p];
        args.extend(devices.iter().map(String::as_str));
        run("mkfs.btrfs", &args)?;
        let uuid = run("blkid", &["-s", "UUID", "-o", "value", &devices[0]])?.trim().to_owned();
        let _ = run("btrfs", &["device", "scan"]);

        // vorhandene Daten (Benutzer, Fotos …) auf den neuen Speicher kopieren
        self.set_job_message("Vorhandene Daten werden auf den neuen Speicher kopiert …");
        let tmp = PathBuf::from("/mnt/newpool");
        std::fs::create_dir_all(&tmp).map_err(|e| e.to_string())?;
        run("mount", &["-o", "noatime", &format!("UUID={uuid}"), tmp.to_str().unwrap()])?;
        // neuer Speicher gehört demselben Benutzer wie der bisherige Ordner,
        // sonst darf die Web-App (läuft nicht als root) dort nicht schreiben
        {
            use std::os::unix::fs::MetadataExt;
            let m = std::fs::metadata(&self.data).map_err(|e| e.to_string())?;
            let _ = run("chown", &[&format!("{}:{}", m.uid(), m.gid()), tmp.to_str().unwrap()]);
            let _ = run("chmod", &[&format!("{:o}", m.mode() & 0o7777), tmp.to_str().unwrap()]);
        }
        let copied = run("cp", &["-a", &format!("{}/.", self.data_str()), tmp.to_str().unwrap()]);
        let _ = run("sync", &[]);
        let _ = run("umount", &[tmp.to_str().unwrap()]);
        copied?;

        self.set_job_message("Speicher wird eingehängt …");
        run("mount", &["-o", "noatime", &format!("UUID={uuid}"), self.data_str()])?;
        self.save_config(&PoolConfig { uuid, profile: data_p.into(), created_at: db::now(), last_scrub: None })
    }

    fn add(&self, device: String) -> Result<(), String> {
        let mut cfg = self.config().ok_or("Kein Speicherverbund eingerichtet")?;
        run("wipefs", &["-a", &device])?;
        run("btrfs", &["device", "add", "-f", &device, self.data_str()])?;
        // Mit der zweiten Platte wird aus „ohne Schutz“ ein Spiegel
        if cfg.profile == "single" {
            self.set_job_message("Daten werden auf beide Platten gespiegelt …");
            run("btrfs", &["balance", "start", "-dconvert=raid1", "-mconvert=raid1", self.data_str()])?;
            cfg.profile = "raid1".into();
            self.save_config(&cfg)?;
        }
        Ok(())
    }

    fn replace(&self, devid: i64, device: String) -> Result<(), String> {
        run("wipefs", &["-a", &device])?;
        run("btrfs", &["replace", "start", "-f", &devid.to_string(), &device, self.data_str()])?;
        // warten, bis die Daten auf die neue Platte kopiert sind
        loop {
            std::thread::sleep(Duration::from_secs(3));
            let st = run("btrfs", &["replace", "status", "-1", self.data_str()]).unwrap_or_default();
            if let Some(p) = st.split('%').next().and_then(|s| s.split_whitespace().last()) {
                self.set_job_message(&format!("Daten werden auf die neue Platte kopiert … {p} %"));
            }
            if st.contains("finished") || st.contains("Never started") {
                break;
            }
            if st.contains("canceled") || st.contains("failed") {
                return Err(format!("Tausch abgebrochen: {}", st.trim()));
            }
        }
        // Was im Notbetrieb nur einfach geschrieben wurde, wieder spiegeln
        self.set_job_message("Spiegel wird wiederhergestellt …");
        let profile = self.config().map(|c| c.profile).unwrap_or_else(|| "raid1".into());
        if profile != "single" {
            run("btrfs", &["balance", "start", &format!("-dconvert={profile},soft"), &format!("-mconvert={profile},soft"), self.data_str()])?;
        }
        Ok(())
    }

    fn remove(&self, devid: Option<i64>) -> Result<(), String> {
        let target = devid.map_or("missing".to_owned(), |d| d.to_string());
        run("btrfs", &["device", "remove", &target, self.data_str()]).map(|_| ())
    }

    fn scrub(&self) -> Result<(), String> {
        run("btrfs", &["scrub", "start", self.data_str()])?;
        if let Some(mut cfg) = self.config() {
            cfg.last_scrub = Some(db::now());
            self.save_config(&cfg)?;
        }
        Ok(())
    }
}

// ---------- HTTP des Dienstes ----------

async fn agent_auth(State(agent): State<Arc<Agent>>, req: Request, next: Next) -> Response {
    let ok = req.headers().get("x-agent-token").and_then(|v| v.to_str().ok()) == Some(agent.token.as_str());
    if !ok && req.uri().path() != "/health" {
        return (StatusCode::UNAUTHORIZED, "Token fehlt").into_response();
    }
    next.run(req).await
}

fn blocking_json<F: FnOnce() -> Value + Send + 'static>(f: F) -> impl Future<Output = Json<Value>> {
    async move { Json(tokio::task::spawn_blocking(f).await.unwrap_or(Value::Null)) }
}

#[derive(Deserialize)]
struct CreateBody {
    devices: Vec<String>,
    #[serde(default)]
    profile: String,
    #[serde(default)]
    confirm: Vec<String>,
}

#[derive(Deserialize)]
struct DeviceBody {
    device: Option<String>,
    devid: Option<i64>,
    #[serde(default)]
    confirm: Vec<String>,
}

fn job_result(r: Result<(), String>) -> Response {
    match r {
        Ok(()) => Json(json!({ "ok": true })).into_response(),
        Err(e) => (StatusCode::BAD_REQUEST, Json(json!({ "error": e }))).into_response(),
    }
}

async fn agent_action(State(agent): State<Arc<Agent>>, UrlPath(action): UrlPath<String>, body: Option<Json<Value>>) -> Response {
    let body = body.map(|b| b.0).unwrap_or(Value::Null);
    let a = agent.clone();
    let result = tokio::task::spawn_blocking(move || -> Result<(), String> {
        match action.as_str() {
            "create" => {
                if a.config().is_some() {
                    return Err("Es ist bereits ein Speicherverbund eingerichtet".into());
                }
                let b: CreateBody = serde_json::from_value(body).map_err(|e| e.to_string())?;
                if b.devices.is_empty() {
                    return Err("Bitte mindestens eine Platte auswählen".into());
                }
                a.check_devices(&b.devices, &b.confirm)?;
                let n = b.devices.len();
                a.start_job("create", &format!("Speicher aus {n} Platte(n) wird eingerichtet …"), move |me| me.create(b.devices, b.profile))
            }
            "add" => {
                let b: DeviceBody = serde_json::from_value(body).map_err(|e| e.to_string())?;
                let dev = b.device.ok_or("Gerät fehlt")?;
                a.check_devices(std::slice::from_ref(&dev), &b.confirm)?;
                a.start_job("add", "Platte wird hinzugefügt …", move |me| me.add(dev))
            }
            "replace" => {
                let b: DeviceBody = serde_json::from_value(body).map_err(|e| e.to_string())?;
                let (dev, devid) = (b.device.ok_or("Neue Platte fehlt")?, b.devid.ok_or("Alte Platte fehlt")?);
                a.check_devices(std::slice::from_ref(&dev), &b.confirm)?;
                a.start_job("replace", "Platte wird getauscht …", move |me| me.replace(devid, dev))
            }
            "remove" => {
                let b: DeviceBody = serde_json::from_value(body).map_err(|e| e.to_string())?;
                let cfg = a.config().ok_or("Kein Speicherverbund eingerichtet")?;
                let st = a.pool_status(&cfg);
                let present = st["devices"].as_array().map_or(0, |d| d.iter().filter(|d| d["missing"] != true).count());
                let min = match cfg.profile.as_str() {
                    "raid1c3" => 3,
                    "raid1" => 2,
                    _ => 1,
                };
                let removing_present = b.devid.is_some_and(|id| st["devices"].as_array().is_some_and(|d| d.iter().any(|d| d["devid"] == id && d["missing"] != true)));
                if removing_present && present <= min {
                    return Err(format!("Der Spiegel braucht mindestens {min} Platten. Bitte zuerst eine neue Platte hinzufügen oder „Tauschen“ verwenden."));
                }
                a.start_job("remove", "Platte wird entfernt, Daten werden umverteilt …", move |me| me.remove(b.devid))
            }
            "scrub" => {
                a.config().ok_or("Kein Speicherverbund eingerichtet")?;
                a.scrub()
            }
            _ => Err("Unbekannte Aktion".into()),
        }
    })
    .await
    .unwrap_or_else(|e| Err(e.to_string()));
    job_result(result)
}

/// Startet den Platten-Dienst (eigener Container, braucht Zugriff auf /dev).
pub async fn run_agent() {
    let token = std::env::var("DISK_AGENT_TOKEN").unwrap_or_default();
    if token.len() < 16 || token.starts_with("bitte") {
        eprintln!("Fehler: DISK_AGENT_TOKEN muss ein eigenes Token sein, mindestens 16 Zeichen (z. B. openssl rand -hex 24)");
        std::process::exit(1);
    }
    let agent = Arc::new(Agent {
        data: PathBuf::from(std::env::var("DATA_DIR").unwrap_or_else(|_| "/data".into())),
        state_dir: PathBuf::from(std::env::var("STATE_DIR").unwrap_or_else(|_| "/state".into())),
        token,
        test_loop_dir: std::env::var("DISK_TEST_LOOP_DIR").ok().filter(|s| !s.is_empty()),
        job: Mutex::new(None),
        last_error: Mutex::new(None),
        ready: AtomicBool::new(false),
    });
    std::fs::create_dir_all(&agent.state_dir).expect("STATE_DIR");
    let a = agent.clone();
    tokio::task::spawn_blocking(move || a.mount_at_boot());

    // monatliche Datenprüfung
    let a = agent.clone();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(3600)).await;
            let due = a.config().is_some_and(|c| {
                a.mounted().is_some() && c.last_scrub.unwrap_or(c.created_at) < db::now() - SCRUB_EVERY_DAYS * 86400
            });
            if due && a.job.lock().unwrap().is_none() {
                let a2 = a.clone();
                let _ = tokio::task::spawn_blocking(move || {
                    tracing::info!("Monatliche Datenprüfung startet");
                    a2.scrub()
                })
                .await;
            }
        }
    });

    let app = Router::new()
        .route("/health", get(|State(a): State<Arc<Agent>>| async move {
            let h = tokio::task::spawn_blocking(move || a.health()).await.unwrap_or(Value::Null);
            let code = if h["ready"] == true { StatusCode::OK } else { StatusCode::SERVICE_UNAVAILABLE };
            (code, Json(h))
        }))
        .route("/status", get(|State(a): State<Arc<Agent>>| blocking_json(move || a.status())))
        .route("/{action}", post(agent_action))
        .layer(middleware::from_fn_with_state(agent.clone(), agent_auth))
        .with_state(agent);
    let port: u16 = std::env::var("PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(8091);
    let listener = tokio::net::TcpListener::bind(("0.0.0.0", port)).await.expect("Port");
    tracing::info!("Platten-Dienst läuft auf Port {port}");
    axum::serve(listener, app).await.unwrap();
}

// =====================================================================
// Web-App: Weiterleitung an den Platten-Dienst
// =====================================================================

pub struct DiskClient {
    url: String,
    token: String,
    agent: ureq::Agent,
}

impl DiskClient {
    pub fn from_env() -> Option<Arc<Self>> {
        let url = std::env::var("DISK_AGENT_URL").ok().filter(|u| !u.is_empty())?;
        let token = std::env::var("DISK_AGENT_TOKEN").unwrap_or_default();
        let agent = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .timeout_global(Some(Duration::from_secs(30)))
            .build()
            .into();
        Some(Arc::new(Self { url: url.trim_end_matches('/').to_owned(), token, agent }))
    }

    fn call(&self, method: Method, path: &str, body: Option<Value>) -> Result<(u16, Value), String> {
        let url = format!("{}{path}", self.url);
        let res = if method == Method::POST {
            self.agent.post(&url).header("x-agent-token", &self.token).send_json(body.unwrap_or(json!({})))
        } else {
            self.agent.get(&url).header("x-agent-token", &self.token).call()
        };
        let mut res = res.map_err(|e| format!("Platten-Dienst nicht erreichbar: {e}"))?;
        let status = res.status().as_u16();
        let value = res.body_mut().read_json::<Value>().unwrap_or(Value::Null);
        Ok((status, value))
    }

    /// Beim Start der Web-App warten, bis der Speicher eingehängt ist.
    /// Sonst würde sie nach einem Neustart des Servers kurz auf der leeren
    /// Platte darunter loslegen. `false`: Speicher eingerichtet, aber nicht
    /// eingehängt – dann nur lesend starten.
    pub fn wait_until_ready(&self) -> bool {
        for i in 0.. {
            match self.call(Method::GET, "/health", None) {
                Ok((200, h)) => return h["pool"] != true || h["mounted"] == true,
                _ if i % 6 == 0 => tracing::info!("Warte auf den Platten-Dienst (Speicher wird eingehängt) …"),
                _ => {}
            }
            std::thread::sleep(Duration::from_secs(5));
        }
        unreachable!()
    }
}

fn require_admin(u: &CurrentUser) -> Result<(), AppError> {
    if u.admin { Ok(()) } else { Err(AppError::Forbidden) }
}

pub async fn status(State(state): State<AppState>, Extension(me): Extension<CurrentUser>) -> Result<Json<Value>, AppError> {
    require_admin(&me)?;
    let Some(client) = state.disks.clone() else {
        return Ok(Json(json!({ "available": false })));
    };
    let r = tokio::task::spawn_blocking(move || client.call(Method::GET, "/status", None))
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;
    Ok(Json(match r {
        Ok((_, mut v)) => {
            v["available"] = json!(true);
            v["maintenance"] = json!(state.maintenance.load(Ordering::SeqCst));
            v
        }
        Err(e) => json!({ "available": true, "reachable": false, "error": e }),
    }))
}

/// Kurzer Zustand für Warnungen in der Seitenleiste (alle Admins).
pub async fn summary(State(state): State<AppState>, Extension(me): Extension<CurrentUser>) -> Result<Json<Value>, AppError> {
    require_admin(&me)?;
    let Some(client) = state.disks.clone() else { return Ok(Json(json!({ "state": "none" }))) };
    let r = tokio::task::spawn_blocking(move || client.call(Method::GET, "/status", None)).await.ok().and_then(Result::ok);
    let state = match r {
        None => "unreachable",
        Some((_, v)) => {
            let pool = &v["pool"];
            if pool.is_null() {
                "none"
            } else if pool["degraded"] == true || !pool["mounted"].as_bool().unwrap_or(false) {
                "degraded"
            } else if pool["devices"].as_array().is_some_and(|d| d.iter().any(|d| d["errors"].as_u64().unwrap_or(0) > 0))
                || v["disks"].as_array().is_some_and(|d| d.iter().any(|d| d["in_pool"] == true && d["smart"]["passed"] == false))
            {
                "warning"
            } else {
                "ok"
            }
        }
    };
    Ok(Json(json!({ "state": state })))
}

pub async fn action(
    State(state): State<AppState>,
    Extension(me): Extension<CurrentUser>,
    UrlPath(action): UrlPath<String>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, AppError> {
    require_admin(&me)?;
    let client = state.disks.clone().ok_or_else(|| AppError::Unsupported("Festplatten-Verwaltung ist nicht eingerichtet".into()))?;
    if !["create", "add", "replace", "remove", "scrub"].contains(&action.as_str()) {
        return Err(AppError::NotFound);
    }
    if action == "create" {
        // Während des Umzugs auf den neuen Speicher nichts mehr schreiben
        state.maintenance.store(true, Ordering::SeqCst);
        state.index.pause(true);
        let _ = state
            .db
            .call(|c| Ok(c.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")?))
            .await;
    }
    let path = format!("/{action}");
    let c = client.clone();
    let r = tokio::task::spawn_blocking(move || c.call(Method::POST, &path, Some(body)))
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;
    let result = match r {
        Ok((200, v)) => Ok(Json(v)),
        Ok((_, v)) => Err(AppError::BadRequest(v["error"].as_str().unwrap_or("Fehler im Platten-Dienst").to_owned())),
        Err(e) => Err(AppError::Internal(e)),
    };
    if action == "create" {
        if result.is_err() {
            end_maintenance(&state);
        } else {
            // Bei Erfolg startet die Web-App neu (watch_data_mount). Schlägt
            // das Einrichten fehl, wieder normal weiterarbeiten.
            let (state, client) = (state.clone(), client.clone());
            tokio::spawn(async move {
                loop {
                    tokio::time::sleep(Duration::from_secs(3)).await;
                    let c = client.clone();
                    let r = tokio::task::spawn_blocking(move || c.call(Method::GET, "/status", None)).await;
                    let Ok(Ok((200, v))) = r else { continue };
                    if v["job"].is_null() {
                        if v["pool"]["mounted"] != true {
                            tracing::warn!("Festplatten einrichten fehlgeschlagen: {}", v["last_error"]);
                            end_maintenance(&state);
                        }
                        return;
                    }
                }
            });
        }
    }
    if result.is_ok() {
        tracing::info!("Festplatten: '{action}' gestartet von '{}'", me.name);
    }
    result
}

fn end_maintenance(state: &AppState) {
    state.maintenance.store(false, Ordering::SeqCst);
    state.index.pause(false);
}

/// Wartungsmodus: Während der Speicher umgezogen wird, sind nur lesende
/// Anfragen erlaubt.
pub async fn maintenance_guard(State(state): State<AppState>, req: Request, next: Next) -> Response {
    let read_only = matches!(*req.method(), Method::GET | Method::HEAD);
    if !read_only && state.maintenance.load(Ordering::SeqCst) {
        return AppError::Unavailable("Die Festplatten werden gerade eingerichtet. Bitte kurz warten.".into()).into_response();
    }
    next.run(req).await
}

/// Wird der Datenordner neu eingehängt (neuer Speicher), startet die
/// Web-App neu, damit sie Datenbank und Dateien vom neuen Speicher liest.
pub fn watch_data_mount(data_dir: PathBuf) {
    use std::os::unix::fs::MetadataExt;
    let Ok(initial) = std::fs::metadata(&data_dir).map(|m| m.dev()) else { return };
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(Duration::from_secs(3));
            if let Ok(dev) = std::fs::metadata(&data_dir).map(|m| m.dev()) {
                if dev != initial {
                    tracing::warn!("Datenordner wurde neu eingehängt, starte neu …");
                    std::process::exit(0);
                }
            }
        }
    });
}
