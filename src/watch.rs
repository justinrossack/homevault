//! Bemerkt Änderungen in Gemeinsam/ und Privat/ – egal ob über die
//! Weboberfläche, das Netzlaufwerk oder einen Scanner. Live-Sync und
//! Eingangs-Ordner hängen sich hier an.
//!
//! Klappt das Beobachten nicht (z. B. zu viele Ordner für inotify),
//! läuft alles weiter: Sync und Eingang prüfen zusätzlich regelmäßig.

use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use notify::{Event, EventKind, RecursiveMode, Watcher as _};

use crate::paths;

type Listener = Box<dyn Fn(&str) + Send + Sync>;

pub struct Watcher {
    listeners: Mutex<Vec<Listener>>,
    /// hält den Beobachter am Leben
    inner: Mutex<Option<notify::RecommendedWatcher>>,
}

impl Watcher {
    pub fn start(data_dir: PathBuf) -> Arc<Self> {
        let me = Arc::new(Self { listeners: Mutex::new(Vec::new()), inner: Mutex::new(None) });
        let weak = Arc::downgrade(&me);
        let base = data_dir.clone();
        let handler = move |res: notify::Result<Event>| {
            let (Ok(event), Some(me)) = (res, weak.upgrade()) else { return };
            if matches!(event.kind, EventKind::Access(_) | EventKind::Other) {
                return;
            }
            for p in &event.paths {
                if let Some(rel) = relevant(&base, p) {
                    for l in me.listeners.lock().unwrap().iter() {
                        l(&rel);
                    }
                }
            }
        };
        match notify::recommended_watcher(handler) {
            Ok(mut w) => {
                let mut ok = true;
                for space in [paths::SHARED, paths::PRIVATE] {
                    if let Err(e) = w.watch(&data_dir.join(space), RecursiveMode::Recursive) {
                        tracing::warn!("Änderungen in {space} werden nicht live erkannt: {e} (Sync/Eingang prüfen regelmäßig)");
                        ok = false;
                    }
                }
                if ok {
                    tracing::info!("Dateiänderungen werden live erkannt");
                }
                *me.inner.lock().unwrap() = Some(w);
            }
            Err(e) => tracing::warn!("Dateiänderungen werden nicht live erkannt: {e}"),
        }
        me
    }

    /// `f` bekommt den geänderten Pfad relativ zum Datenordner,
    /// z. B. "Privat/anna/Eingang/scan.pdf".
    pub fn subscribe(&self, f: impl Fn(&str) + Send + Sync + 'static) {
        self.listeners.lock().unwrap().push(Box::new(f));
    }
}

/// Relativer Pfad, oder None für Dinge, die niemanden interessieren
/// (laufende Uploads, versteckte Dateien wie .DS_Store).
fn relevant(base: &Path, p: &Path) -> Option<String> {
    let rel = p.strip_prefix(base).ok()?;
    if rel.components().any(|c| c.as_os_str().to_string_lossy().starts_with('.')) {
        return None;
    }
    let rel = rel.to_string_lossy().into_owned();
    (!rel.is_empty()).then_some(rel)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ignores_hidden_and_temp_files() {
        let base = Path::new("/data");
        assert_eq!(relevant(base, Path::new("/data/Gemeinsam/a.jpg")).as_deref(), Some("Gemeinsam/a.jpg"));
        assert_eq!(relevant(base, Path::new("/data/Gemeinsam/.upload-1.part")), None);
        assert_eq!(relevant(base, Path::new("/data/Privat/anna/.DS_Store")), None);
        assert_eq!(relevant(base, Path::new("/data/.papierkorb/1/a.jpg")), None);
        assert_eq!(relevant(base, Path::new("/anderswo/x")), None);
    }
}
