mod archive;
mod auth;
mod backup;
mod db;
mod disks;
mod docs;
mod error;
mod fs_api;
mod geo;
mod inbox;
mod index;
mod library;
mod net;
mod media;
mod paths;
mod restore;
mod share;
mod static_files;
mod storage;
mod sync;
mod trash;
mod upload;
mod users;
mod video;
mod watch;

use std::{net::SocketAddr, path::PathBuf, sync::Arc, time::Duration};

use axum::{
    Router,
    extract::DefaultBodyLimit,
    http::{HeaderMap, StatusCode, Version, header},
    middleware,
    routing::{get, patch, post},
};
use tokio::sync::Semaphore;
use tower_http::{compression::CompressionLayer, trace::TraceLayer};

#[derive(Clone)]
pub struct AppState {
    /// kanonischer Pfad des Datenordners
    pub data_dir: Arc<PathBuf>,
    pub cache_dir: Arc<PathBuf>,
    /// Datenbank und smbpasswd
    pub config_dir: Arc<PathBuf>,
    pub db: Arc<db::Db>,
    pub index: Arc<index::Index>,
    pub backups: Arc<backup::Backups>,
    pub sync: Arc<sync::Syncer>,
    pub inbox: Arc<inbox::Inbox>,
    pub transcoder: Arc<video::Transcoder>,
    /// Platten-Dienst (nur wenn DISK_AGENT_URL gesetzt ist)
    pub disks: Option<Arc<disks::DiskClient>>,
    /// Wartungsmodus: nur lesende Anfragen (Speicher wird umgezogen)
    pub maintenance: Arc<std::sync::atomic::AtomicBool>,
    pub thumb_limit: Arc<Semaphore>,
}

fn env_or(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_owned())
}

/// Nur Text, JSON, JS, CSS und SVG komprimieren, Fotos/Videos nie.
fn compressible(_: StatusCode, _: Version, headers: &HeaderMap, _: &axum::http::Extensions) -> bool {
    let ct = headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    ct.starts_with("text/")
        || ct.starts_with("application/json")
        || ct.starts_with("application/javascript")
        || ct.starts_with("image/svg+xml")
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "homevault=info,tower_http=warn".into()),
        )
        .init();

    // Eigener Startmodus für den Platten-Dienst (privilegierter Container)
    if std::env::args().nth(1).as_deref() == Some("disk-agent") {
        disks::run_agent().await;
        return;
    }

    // Erst loslegen, wenn der Speicherverbund eingehängt ist
    let disk_client = disks::DiskClient::from_env();
    let mut pool_missing = false;
    if let Some(c) = disk_client.clone() {
        pool_missing = !tokio::task::spawn_blocking(move || c.wait_until_ready()).await.unwrap_or(true);
        if pool_missing {
            tracing::error!("Speicherverbund ist nicht eingehängt – nur lesender Notbetrieb, bis die Platten wieder da sind");
        }
    }

    let data_dir = PathBuf::from(env_or("DATA_DIR", "/data"));
    let cache_dir = PathBuf::from(env_or("CACHE_DIR", "/cache"));
    std::fs::create_dir_all(&data_dir).expect("DATA_DIR kann nicht angelegt werden");
    std::fs::create_dir_all(&cache_dir).expect("CACHE_DIR kann nicht angelegt werden");
    let data_dir = data_dir.canonicalize().expect("DATA_DIR ungültig");
    if disk_client.is_some() {
        // neuer Speicher eingehängt -> neu starten und von dort lesen
        disks::watch_data_mount(data_dir.clone());
    }
    for space in [paths::SHARED, paths::PRIVATE] {
        if let Err(e) = std::fs::create_dir_all(data_dir.join(space)) {
            // häufigster Einrichtungsfehler: Ordner gehört root, App läuft als PUID
            use std::os::unix::fs::MetadataExt;
            let owner = std::fs::metadata(&data_dir).map(|m| format!("{}:{}", m.uid(), m.gid())).unwrap_or_default();
            let me = unsafe { (libc::getuid(), libc::getgid()) };
            eprintln!(
                "Fehler: Kein Schreibrecht im Datenordner {} ({e}).\n\
                 Der Ordner gehört {owner}, homevault läuft als {}:{}.\n\
                 Auf dem Server beheben mit:  sudo chown -R {}:{} <MEDIA_DIR>",
                data_dir.display(), me.0, me.1, me.0, me.1
            );
            std::process::exit(1);
        }
    }
    // Reste abgebrochener Konvertierungen entfernen
    let _ = std::fs::remove_dir_all(cache_dir.join("tmp"));

    // Konfiguration liegt standardmäßig versteckt im Datenordner, außerhalb
    // der Bereiche, also weder im Web noch per SMB erreichbar.
    let config_dir = std::env::var("CONFIG_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| data_dir.join(".homevault"));
    std::fs::create_dir_all(&config_dir).expect("CONFIG_DIR kann nicht angelegt werden");
    // Datenbank und smbpasswd enthalten Passwort-Hashes: nur für den Besitzer
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&config_dir, std::fs::Permissions::from_mode(0o700));
    }
    // nach einer Komplett-Wiederherstellung: Datenbank aus dem Backup einsetzen
    restore::apply_pending_db(&config_dir);
    let db = db::Db::open(&config_dir.join("homevault.db")).unwrap_or_else(|e| {
        eprintln!("Fehler: Datenbank: {e}");
        std::process::exit(1);
    });
    // Optional: ersten Admin ohne Einrichtungsassistent anlegen (z. B. für Skripte)
    let bootstrap = std::env::var("ADMIN_PASSWORD")
        .ok()
        .filter(|p| !p.is_empty())
        .map(|p| (env_or("ADMIN_USER", "admin").to_lowercase(), p));
    if let Err(e) = users::startup(&db, &data_dir, &config_dir, bootstrap) {
        eprintln!("Fehler: {e}");
        std::process::exit(1);
    }
    let interval: u64 = env_or("INDEX_INTERVAL_MINUTES", "10").parse().expect("INDEX_INTERVAL_MINUTES ungültig");
    let index = index::Index::start(
        db.path().to_path_buf(),
        data_dir.clone(),
        cache_dir.clone(),
        Duration::from_secs(interval.max(1) * 60),
        pool_missing,
    );
    trash::start_cleanup(db.clone(), data_dir.clone());
    let transcoder = video::Transcoder::start(db.clone(), data_dir.clone(), cache_dir.clone());

    let backups = backup::Backups::new(db.clone(), data_dir.clone(), &cache_dir);
    backups.start_scheduler();
    // fehlt der Speicherverbund, liegt unter /data nur der alte Ordner
    let maintenance = Arc::new(std::sync::atomic::AtomicBool::new(pool_missing));
    let watcher = watch::Watcher::start(data_dir.clone());
    let syncer = sync::Syncer::start(db.clone(), data_dir.clone(), maintenance.clone(), &watcher);
    let inbox = inbox::Inbox::new(db.clone(), data_dir.clone(), maintenance.clone());

    let workers = std::thread::available_parallelism().map_or(2, |n| n.get());
    let state = AppState {
        data_dir: Arc::new(data_dir.clone()),
        cache_dir: Arc::new(cache_dir),
        config_dir: Arc::new(config_dir),
        db,
        index,
        backups,
        transcoder,
        disks: disk_client.clone(),
        sync: syncer,
        inbox: inbox.clone(),
        maintenance,
        thumb_limit: Arc::new(Semaphore::new(workers)),
    };
    inbox.start(state.clone(), &watcher);

    let protected = Router::new()
        .route("/api/list", get(fs_api::list))
        .route("/api/folder", post(fs_api::mkdir))
        .route("/api/rename", patch(fs_api::rename))
        .route("/api/file", get(media::serve_file).delete(fs_api::delete))
        .route("/api/thumb", get(media::thumb))
        .route("/api/preview", get(media::preview))
        .route("/api/live", get(media::live))
        .route("/api/me/password", post(users::change_own_password))
        .route("/api/users", get(users::list).post(users::create))
        .route("/api/users/{name}", patch(users::update).delete(users::delete))
        .route("/api/settings", get(users::get_settings).patch(users::update_settings))
        .route("/api/storage", get(storage::overview))
        .route("/api/trash", get(trash::list))
        .route("/api/trash/restore", post(trash::restore))
        .route("/api/trash/delete", post(trash::delete_forever))
        .route("/api/trash/empty", post(trash::empty))
        .route("/api/trash/{id}/thumb", get(trash::thumb))
        .route("/api/video", get(video::status))
        .route("/api/doc", get(docs::view))
        .route("/api/archive", get(archive::list))
        .route("/api/archive/file", get(archive::file))
        .route("/api/capabilities", get(docs::capabilities))
        .route("/api/video/web", get(video::web))
        .route("/api/disks", get(disks::status))
        .route("/api/disks/summary", get(disks::summary))
        .route("/api/disks/{action}", post(disks::action))
        .route("/api/shares", get(share::list).post(share::create))
        .route("/api/shares/{id}", axum::routing::delete(share::delete))
        .route("/api/timeline", get(library::timeline))
        .route("/api/favorites", get(library::favorites).post(library::set_favorites))
        .route("/api/albums", get(library::albums).post(library::create_album))
        .route(
            "/api/albums/{id}",
            get(library::album).patch(library::rename_album).delete(library::delete_album),
        )
        .route("/api/albums/{id}/add", post(library::add_items))
        .route("/api/albums/{id}/remove", post(library::remove_items))
        .route("/api/index", get(library::index_status))
        .route("/api/index/rescan", post(library::rescan))
        .route("/api/network", get(net::list))
        .route("/api/inbox", get(inbox::status))
        .route("/api/inbox/{id}/undo", post(inbox::undo))
        .route("/api/sync", get(sync::list).post(sync::create))
        .route("/api/sync/{id}", patch(sync::update).delete(sync::delete))
        .route("/api/sync/{id}/run", post(sync::run_now))
        .route("/api/backups", get(backup::list).post(backup::create))
        .route("/api/backups/test", post(backup::test))
        .route("/api/backups/cancel", post(backup::cancel))
        .route("/api/backups/{id}", patch(backup::update).delete(backup::delete))
        .route("/api/backups/{id}/run", post(backup::run_now))
        .route("/api/backups/{id}/runs", get(backup::runs))
        .route("/api/backups/{id}/versions", get(restore::versions))
        .route("/api/backups/{id}/browse", get(restore::browse))
        .route("/api/backups/{id}/restore", post(restore::restore))
        .route(
            "/api/upload",
            post(upload::upload).layer(DefaultBodyLimit::disable()),
        )
        .route_layer(middleware::from_fn_with_state(state.clone(), disks::maintenance_guard))
        .route_layer(middleware::from_fn_with_state(state.clone(), auth::require_auth));

    let app = Router::new()
        .route("/api/setup", post(users::setup))
        .route("/api/setup/restore", get(restore::setup_restore_status).post(restore::setup_restore))
        // Teilen-Links: öffentlich, eigene Zugriffsprüfung
        .route("/api/s/{token}", get(share::info))
        .route("/api/s/{token}/unlock", post(share::unlock))
        .route("/api/s/{token}/{item}/{what}", get(share::item))
        .route("/api/login", post(auth::login))
        .route("/api/logout", post(auth::logout))
        .route("/api/me", get(auth::me))
        .route("/api/health", get(|| async { "ok" }))
        .merge(protected)
        .fallback(static_files::handler)
        .layer(CompressionLayer::new().compress_when(compressible))
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    let port: u16 = env_or("PORT", "8080").parse().expect("PORT ungültig");
    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    tracing::info!("homevault läuft auf http://{addr} (Daten: {})", data_dir.display());
    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            let ctrl_c = tokio::signal::ctrl_c();
            #[cfg(unix)]
            {
                let mut term =
                    tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                        .unwrap();
                tokio::select! { _ = ctrl_c => {}, _ = term.recv() => {} }
            }
            #[cfg(not(unix))]
            let _ = ctrl_c.await;
        })
        .await
        .unwrap();
}
