//! Videos für alle Browser: iPhones nehmen meist in HEVC auf, was Chrome,
//! Firefox und viele Windows-Rechner nicht abspielen. Ein Hintergrund-Job
//! erstellt deshalb eine H.264-Fassung (max. 1080p) im Cache. Das Original
//! bleibt unverändert. Was gerade jemand ansehen will, kommt zuerst dran.
//! HDR-Videos werden dabei auf normale Helligkeit umgerechnet (Tone-Mapping).

use std::{
    collections::{HashSet, VecDeque},
    path::{Path, PathBuf},
    process::Stdio,
    sync::{Arc, Mutex},
    time::Duration,
};

use axum::{
    Extension, Json,
    extract::{Query, Request, State},
    response::Response,
};
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::Command,
    sync::Notify,
};

use crate::{
    AppState,
    auth::CurrentUser,
    db::{self, Db},
    error::AppError,
    index::{self, Meta},
    media, paths,
    trash::TRASH_DIR,
};

/// Längste Kante der Web-Fassung
const WEB_SIZE: u32 = 1920;

/// Spielt jeder Browser ab? (H.264 in MP4/MOV)
pub fn plays_everywhere(path: &Path, codec: Option<&str>) -> bool {
    let ext = path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    codec == Some("h264") && matches!(ext.as_str(), "mp4" | "m4v" | "mov")
}

fn web_path(cache_dir: &Path, src: &Path) -> Option<PathBuf> {
    let meta = std::fs::metadata(src).ok()?;
    Some(media::cache_path(cache_dir, "web", src, &meta, "mp4"))
}

#[derive(Serialize, Clone)]
struct Current {
    #[serde(skip)]
    path: PathBuf,
    name: String,
    progress: f64,
}

pub struct Transcoder {
    db: Arc<Db>,
    data_dir: PathBuf,
    cache_dir: PathBuf,
    /// Vom Benutzer angefragt, wird bevorzugt
    priority: Mutex<VecDeque<PathBuf>>,
    current: Mutex<Option<Current>>,
    failed: Mutex<HashSet<PathBuf>>,
    wake: Notify,
}

impl Transcoder {
    pub fn start(db: Arc<Db>, data_dir: PathBuf, cache_dir: PathBuf) -> Arc<Self> {
        let me = Arc::new(Self {
            db,
            data_dir,
            cache_dir,
            priority: Mutex::new(VecDeque::new()),
            current: Mutex::new(None),
            failed: Mutex::new(HashSet::new()),
            wake: Notify::new(),
        });
        let worker = me.clone();
        tokio::spawn(async move { worker.run().await });
        me
    }

    /// Video vorziehen (jemand möchte es gerade ansehen).
    pub fn prioritize(&self, src: PathBuf) {
        let mut q = self.priority.lock().unwrap();
        if !q.contains(&src) && self.current.lock().unwrap().as_ref().is_none_or(|c| c.path != src) {
            q.push_front(src);
        }
        drop(q);
        self.wake.notify_one();
    }

    fn progress_of(&self, src: &Path) -> Option<f64> {
        self.current.lock().unwrap().as_ref().filter(|c| c.path == src).map(|c| c.progress)
    }

    pub fn status(&self) -> Value {
        json!({
            "current": *self.current.lock().unwrap(),
            "queued": self.priority.lock().unwrap().len(),
        })
    }

    async fn run(self: Arc<Self>) {
        // dem Index beim Start etwas Vorsprung lassen
        tokio::time::sleep(Duration::from_secs(10)).await;
        loop {
            let queued = self.priority.lock().unwrap().pop_front();
            let next = match queued {
                Some(p) => Some(p),
                None => self.find_candidate().await,
            };
            let Some(src) = next else {
                // nichts zu tun: auf Anfrage oder nächsten Durchlauf warten
                let _ = tokio::time::timeout(Duration::from_secs(120), self.wake.notified()).await;
                continue;
            };
            if let Err(e) = self.convert(&src).await {
                tracing::warn!("Video-Umwandlung fehlgeschlagen ({}): {e}", src.display());
                self.failed.lock().unwrap().insert(src);
            }
            *self.current.lock().unwrap() = None;
        }
    }

    /// Nächstes Video ohne Web-Fassung (neueste zuerst), falls automatische
    /// Umwandlung eingeschaltet ist.
    async fn find_candidate(&self) -> Option<PathBuf> {
        let rows: Vec<(String, Option<String>)> = self
            .db
            .call(|c| {
                if !may_run_now(&mode(c), chrono::Local::now()) {
                    return Ok(Vec::new());
                }
                Ok(c.prepare(&format!(
                    "SELECT path, codec FROM media WHERE kind = 'video' AND path NOT LIKE '{TRASH_DIR}/%'
                     ORDER BY taken_at DESC"
                ))?
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect::<Result<_, _>>()?)
            })
            .await
            .ok()?;
        let failed = self.failed.lock().unwrap().clone();
        rows.into_iter().map(|(p, codec)| (self.data_dir.join(p), codec)).find(|(abs, codec)| {
            codec.is_some()
                && !plays_everywhere(abs, codec.as_deref())
                && !failed.contains(abs)
                && web_path(&self.cache_dir, abs).is_some_and(|w| !w.exists())
        })
        .map(|(abs, _)| abs)
    }

    async fn convert(&self, src: &Path) -> Result<(), String> {
        let Some(out) = web_path(&self.cache_dir, src) else { return Ok(()) };
        if out.exists() {
            return Ok(());
        }
        let probe = src.to_path_buf();
        let meta = tokio::task::spawn_blocking(move || index::probe_video(&probe)).await.unwrap_or_default();
        if plays_everywhere(src, meta.codec.as_deref()) {
            return Ok(());
        }
        let name = src.file_name().unwrap_or_default().to_string_lossy().into_owned();
        *self.current.lock().unwrap() = Some(Current { path: src.to_path_buf(), name: name.clone(), progress: 0.0 });
        tracing::info!("Video wird für alle Browser umgewandelt: {name}");

        tokio::fs::create_dir_all(out.parent().unwrap()).await.map_err(|e| e.to_string())?;
        let tmp = media::tmp_for(&out);
        let mut vf = format!("scale={WEB_SIZE}:{WEB_SIZE}:force_original_aspect_ratio=decrease:force_divisible_by=2");
        if meta.hdr {
            // HLG/PQ auf normale Helligkeit (BT.709) umrechnen, sonst wirkt es blass
            vf.push_str(
                ",zscale=t=linear:npl=100,format=gbrpf32le,zscale=p=bt709,tonemap=tonemap=hable:desat=0,zscale=t=bt709:m=bt709:r=tv",
            );
        }
        // Intel Quick Sync (VAAPI) kodiert in Hardware; klappt das nicht, Software
        let hw = tokio::task::spawn_blocking(vaapi_device).await.ok().flatten();
        let result = match hw {
            Some(dev) => match self.ffmpeg(src, &tmp, &vf, Some(dev), meta.duration).await {
                Ok(()) => Ok(()),
                Err(e) => {
                    tracing::warn!("Hardware-Umwandlung fehlgeschlagen ({e}), versuche Software");
                    self.ffmpeg(src, &tmp, &vf, None, meta.duration).await
                }
            },
            None => self.ffmpeg(src, &tmp, &vf, None, meta.duration).await,
        };
        if let Err(e) = result {
            let _ = tokio::fs::remove_file(&tmp).await;
            return Err(e);
        }
        tokio::fs::rename(&tmp, &out).await.map_err(|e| e.to_string())?;
        tracing::info!("Video umgewandelt{}: {name}", if hw.is_some() { " (Quick Sync)" } else { "" });
        Ok(())
    }

    /// Ein ffmpeg-Lauf mit niedrigster Priorität und Fortschrittsanzeige.
    async fn ffmpeg(&self, src: &Path, tmp: &Path, vf: &str, hw: Option<&str>, duration: Option<f64>) -> Result<(), String> {
        // nur die Hälfte der Kerne, damit der Server nutzbar (und leise) bleibt
        let threads = (std::thread::available_parallelism().map_or(2, |n| n.get()) / 2).max(1).to_string();
        let mut cmd = Command::new("ffmpeg");
        cmd.args(["-v", "error", "-y", "-nostats", "-progress", "pipe:1"]);
        if let Some(dev) = hw {
            cmd.args(["-vaapi_device", dev]);
        }
        // auch das Auspacken (HEVC) nur mit begrenzt vielen Threads
        cmd.args(["-threads", &threads, "-i"]).arg(src);
        cmd.args(["-map", "0:v:0", "-map", "0:a:0?"]);
        match hw {
            Some(_) => cmd.args(["-vf", &format!("{vf},format=nv12,hwupload"), "-c:v", "h264_vaapi", "-qp", "23", "-profile:v", "high"]),
            None => cmd.args([
                "-vf", &format!("{vf},format=yuv420p"),
                "-c:v", "libx264", "-preset", "veryfast", "-crf", "23", "-profile:v", "high", "-threads", &threads,
            ]),
        };
        cmd.args(["-c:a", "aac", "-b:a", "128k", "-movflags", "+faststart"]).arg(tmp);
        // niedrigste Priorität: Uploads, Videos ansehen und Netzlaufwerk gehen vor
        // SAFETY: nice() ist zwischen fork und exec erlaubt
        unsafe {
            cmd.pre_exec(|| {
                libc::nice(19);
                Ok(())
            });
        }
        let mut child = cmd
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| format!("ffmpeg nicht verfügbar: {e}"))?;

        let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
        let duration = duration.unwrap_or(0.0);
        while let Ok(Some(line)) = lines.next_line().await {
            if let Some(us) = line.strip_prefix("out_time_us=").and_then(|v| v.parse::<f64>().ok()) {
                if duration > 0.0 {
                    if let Some(c) = self.current.lock().unwrap().as_mut() {
                        c.progress = (us / 1e6 / duration).clamp(0.0, 0.99);
                    }
                }
            }
        }
        let output = child.wait_with_output().await.map_err(|e| e.to_string())?;
        if !output.status.success() {
            let err = String::from_utf8_lossy(&output.stderr);
            return Err(err.lines().last().unwrap_or("ffmpeg-Fehler").to_owned());
        }
        Ok(())
    }
}

/// Wann wird automatisch umgewandelt? "now" (sofort), "night" (1–7 Uhr),
/// "demand" (nur wenn jemand das Video ansehen will)
pub fn mode(c: &rusqlite::Connection) -> String {
    match db::get_setting(c, "transcode_mode").ok().flatten() {
        Some(m) if matches!(m.as_str(), "now" | "night" | "demand") => m,
        // ältere Einstellung „automatisch umwandeln“ aus/an
        _ if db::get_setting(c, "auto_transcode").ok().flatten().as_deref() == Some("0") => "demand".into(),
        _ => "now".into(),
    }
}

fn may_run_now(mode: &str, now: chrono::DateTime<chrono::Local>) -> bool {
    use chrono::Timelike;
    match mode {
        "now" => true,
        "night" => (1..7).contains(&now.hour()),
        _ => false,
    }
}

/// Gerät für Intel Quick Sync, falls vorhanden und nutzbar (einmal geprüft).
fn vaapi_device() -> Option<&'static str> {
    static DEV: std::sync::OnceLock<Option<&'static str>> = std::sync::OnceLock::new();
    *DEV.get_or_init(|| {
        let dev = "/dev/dri/renderD128";
        std::fs::OpenOptions::new().read(true).write(true).open(dev).ok()?;
        // Probe: ein Bild in Hardware kodieren
        let ok = std::process::Command::new("ffmpeg")
            .args(["-v", "error", "-vaapi_device", dev, "-f", "lavfi", "-i", "testsrc=size=320x240:rate=1", "-frames:v", "1"])
            .args(["-vf", "format=nv12,hwupload", "-c:v", "h264_vaapi", "-f", "null", "-"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success());
        if ok {
            tracing::info!("Videos werden mit Intel Quick Sync umgewandelt");
        }
        ok.then_some(dev)
    })
}

/// Codec eines Videos: erst aus dem Index, sonst direkt per ffprobe.
async fn codec_of(state: &AppState, src: &Path) -> Option<String> {
    let rel = paths::relative(&state.data_dir, src);
    let from_db = state
        .db
        .call(move |c| Ok(c.query_row("SELECT codec FROM media WHERE path = ?", [rel], |r| r.get::<_, Option<String>>(0)).optional()?))
        .await
        .ok()
        .flatten()
        .flatten();
    if from_db.is_some() {
        return from_db;
    }
    let p = src.to_path_buf();
    tokio::task::spawn_blocking(move || index::probe_video(&p)).await.ok().and_then(|m: Meta| m.codec)
}

/// Wie soll der Browser dieses Video abspielen?
/// original = direkt, web = umgewandelte Fassung, pending = wird gerade erstellt
pub async fn status_of(state: &AppState, src: PathBuf) -> Result<Value, AppError> {
    if !tokio::fs::metadata(&src).await?.is_file() {
        return Err(AppError::NotFound);
    }
    let codec = codec_of(state, &src).await;
    if plays_everywhere(&src, codec.as_deref()) {
        return Ok(json!({ "mode": "original", "codec": codec }));
    }
    if web_path(&state.cache_dir, &src).is_some_and(|w| w.exists()) {
        return Ok(json!({ "mode": "web", "codec": codec }));
    }
    let progress = state.transcoder.progress_of(&src);
    if state.transcoder.failed.lock().unwrap().contains(&src) {
        return Ok(json!({ "mode": "failed", "codec": codec }));
    }
    state.transcoder.prioritize(src);
    Ok(json!({ "mode": "pending", "codec": codec, "progress": progress }))
}

pub async fn web_of(state: &AppState, src: &Path, req: Request) -> Result<Response, AppError> {
    let out = web_path(&state.cache_dir, src).filter(|w| w.exists()).ok_or(AppError::NotFound)?;
    media::serve_path(&out, req, media::IMMUTABLE).await
}

#[derive(Deserialize)]
pub struct PathQuery {
    path: String,
}

pub async fn status(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Query(q): Query<PathQuery>,
) -> Result<Json<Value>, AppError> {
    let src = paths::resolve_in_space(&state.data_dir, &user.name, &q.path)?.full;
    Ok(Json(status_of(&state, src).await?))
}

pub async fn web(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Query(q): Query<PathQuery>,
    req: Request,
) -> Result<Response, AppError> {
    let src = paths::resolve_in_space(&state.data_dir, &user.name, &q.path)?.full;
    web_of(&state, &src, req).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn night_mode_only_between_one_and_seven() {
        use chrono::TimeZone;
        let at = |h| chrono::Local.with_ymd_and_hms(2026, 10, 12, h, 30, 0).earliest().unwrap();
        assert!(may_run_now("now", at(14)));
        assert!(may_run_now("night", at(3)) && !may_run_now("night", at(7)) && !may_run_now("night", at(0)));
        assert!(!may_run_now("demand", at(3)));
    }

    #[test]
    fn welche_videos_umgewandelt_werden() {
        assert!(plays_everywhere(Path::new("a.mp4"), Some("h264")));
        assert!(plays_everywhere(Path::new("IMG_1.MOV"), Some("h264")));
        assert!(!plays_everywhere(Path::new("IMG_1.MOV"), Some("hevc")));
        assert!(!plays_everywhere(Path::new("a.mkv"), Some("h264")));
        assert!(!plays_everywhere(Path::new("a.mp4"), None));
    }
}
