//! SQLite-Datenbank: Benutzer, Sessions, Einstellungen, Medien-Index,
//! Favoriten und Alben.
//!
//! Die Fotos selbst bleiben normale Dateien. Der Medien-Index ist nur ein
//! Verzeichnis darüber und kann jederzeit neu aufgebaut werden.

use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use rusqlite::{Connection, OptionalExtension};

use crate::error::AppError;

/// Schema-Versionen. Neue Änderungen immer hinten anhängen, nie alte ändern.
const MIGRATIONS: &[&str] = &[
    // 1: Grundschema
    r#"
    CREATE TABLE settings (
        key   TEXT PRIMARY KEY,
        value TEXT NOT NULL
    );

    CREATE TABLE users (
        id            INTEGER PRIMARY KEY,
        name          TEXT NOT NULL UNIQUE,
        admin         INTEGER NOT NULL DEFAULT 0,
        uid           INTEGER NOT NULL UNIQUE,   -- Linux-UID im Samba-Container
        password_hash TEXT NOT NULL,             -- Argon2
        nt_hash       TEXT NOT NULL,             -- für Samba
        created_at    INTEGER NOT NULL
    );

    CREATE TABLE sessions (
        token_hash TEXT PRIMARY KEY,             -- blake3 des Cookies
        user_id    INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
        expires_at INTEGER NOT NULL
    );
    CREATE INDEX sessions_user ON sessions(user_id);

    CREATE TABLE media (
        id         INTEGER PRIMARY KEY,
        path       TEXT NOT NULL UNIQUE,          -- relativ zu DATA_DIR, z. B. Gemeinsam/Urlaub/a.jpg
        kind       TEXT NOT NULL,                 -- image | video
        size       INTEGER NOT NULL,
        mtime      INTEGER NOT NULL,
        taken_at   INTEGER NOT NULL,              -- Aufnahmezeit (Ortszeit als Unix-Sekunden), sonst mtime
        live_path  TEXT,                          -- Video eines Live Photos
        indexed_at INTEGER NOT NULL
    );
    CREATE INDEX media_taken ON media(taken_at DESC, id DESC);

    CREATE TABLE favorites (
        user_id    INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
        media_id   INTEGER NOT NULL REFERENCES media(id) ON DELETE CASCADE,
        created_at INTEGER NOT NULL,
        PRIMARY KEY (user_id, media_id)
    );

    CREATE TABLE albums (
        id         INTEGER PRIMARY KEY,
        owner_id   INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
        name       TEXT NOT NULL,
        created_at INTEGER NOT NULL
    );
    CREATE INDEX albums_owner ON albums(owner_id);

    CREATE TABLE album_items (
        album_id INTEGER NOT NULL REFERENCES albums(id) ON DELETE CASCADE,
        media_id INTEGER NOT NULL REFERENCES media(id) ON DELETE CASCADE,
        added_at INTEGER NOT NULL,
        PRIMARY KEY (album_id, media_id)
    );
    CREATE INDEX album_items_media ON album_items(media_id);
    "#,
    // 2: Backup-Ziele (andere NAS per SMB) und Verlauf
    r#"
    CREATE TABLE backup_targets (
        id              INTEGER PRIMARY KEY,
        name            TEXT NOT NULL,
        host            TEXT NOT NULL,
        port            INTEGER NOT NULL DEFAULT 445,
        share           TEXT NOT NULL,
        folder          TEXT NOT NULL DEFAULT '',   -- Unterordner auf der Freigabe
        username        TEXT NOT NULL,
        password        TEXT NOT NULL,              -- wird nie an das Frontend gegeben
        include_shared  INTEGER NOT NULL DEFAULT 1,
        include_private INTEGER NOT NULL DEFAULT 1,
        include_db      INTEGER NOT NULL DEFAULT 1,
        schedule        TEXT NOT NULL DEFAULT 'daily', -- daily | weekly | manual
        time            TEXT NOT NULL DEFAULT '03:00', -- Ortszeit HH:MM
        weekday         INTEGER NOT NULL DEFAULT 6,    -- 0 = Montag (nur weekly)
        keep_days       INTEGER NOT NULL DEFAULT 30,   -- alte Versionen, 0 = für immer
        enabled         INTEGER NOT NULL DEFAULT 1,
        created_at      INTEGER NOT NULL
    );

    CREATE TABLE backup_runs (
        id          INTEGER PRIMARY KEY,
        target_id   INTEGER NOT NULL REFERENCES backup_targets(id) ON DELETE CASCADE,
        started_at  INTEGER NOT NULL,
        finished_at INTEGER,
        status      TEXT NOT NULL,                  -- running | ok | error | cancelled
        files       INTEGER NOT NULL DEFAULT 0,
        bytes       INTEGER NOT NULL DEFAULT 0,
        message     TEXT
    );
    CREATE INDEX backup_runs_target ON backup_runs(target_id, started_at DESC);
    "#,
    // 3: Papierkorb, Speicherlimits, Video-Infos, Teilen-Links
    r#"
    CREATE TABLE trash (
        id            INTEGER PRIMARY KEY,
        space         TEXT NOT NULL,              -- Gemeinsam | Privat
        owner_id      INTEGER REFERENCES users(id) ON DELETE CASCADE, -- bei Privat
        deleted_by    TEXT NOT NULL,
        name          TEXT NOT NULL,
        kind          TEXT NOT NULL,              -- dir | image | video | other
        size          INTEGER NOT NULL,
        original_path TEXT NOT NULL,              -- relativ zu DATA_DIR
        trash_path    TEXT NOT NULL,              -- relativ zu DATA_DIR (.papierkorb/<id>/<name>)
        live_original TEXT,                       -- Live-Video, falls mitgelöscht
        live_trash    TEXT,
        deleted_at    INTEGER NOT NULL
    );
    CREATE INDEX trash_space ON trash(space, owner_id, deleted_at DESC);

    ALTER TABLE users ADD COLUMN quota_bytes INTEGER;   -- NULL = unbegrenzt (gilt für Privat)

    ALTER TABLE media ADD COLUMN codec TEXT;            -- Video-Codec, z. B. h264, hevc
    ALTER TABLE media ADD COLUMN duration REAL;         -- Sekunden
    ALTER TABLE media ADD COLUMN hdr INTEGER NOT NULL DEFAULT 0;

    CREATE TABLE shares (
        id             INTEGER PRIMARY KEY,
        token          TEXT NOT NULL UNIQUE,
        owner_id       INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
        album_id       INTEGER REFERENCES albums(id) ON DELETE CASCADE,
        media_id       INTEGER REFERENCES media(id) ON DELETE CASCADE,
        password_hash  TEXT,
        allow_download INTEGER NOT NULL DEFAULT 1,
        expires_at     INTEGER,
        views          INTEGER NOT NULL DEFAULT 0,
        created_at     INTEGER NOT NULL
    );
    CREATE INDEX shares_owner ON shares(owner_id);
    "#,
    // 4: Live-Sync zu anderen NAS, Eingangs-Ordner (automatisch einsortieren)
    r#"
    -- Wer darf seinen Privat-Ordner selbst auf dieses NAS synchronisieren?
    -- 0 = niemand, 1 = alle Benutzer, 2 = nur die in sync_target_users
    ALTER TABLE backup_targets ADD COLUMN user_sync INTEGER NOT NULL DEFAULT 0;
    CREATE TABLE sync_target_users (
        target_id INTEGER NOT NULL REFERENCES backup_targets(id) ON DELETE CASCADE,
        user_id   INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
        PRIMARY KEY (target_id, user_id)
    );

    CREATE TABLE sync_rules (
        id             INTEGER PRIMARY KEY,
        target_id      INTEGER NOT NULL REFERENCES backup_targets(id) ON DELETE CASCADE,
        source_space   TEXT NOT NULL,             -- Gemeinsam | Privat
        source_user_id INTEGER REFERENCES users(id) ON DELETE CASCADE, -- bei Privat
        source_sub     TEXT NOT NULL DEFAULT '',  -- Unterordner, leer = alles
        dest           TEXT NOT NULL,             -- Ordner in der Freigabe des NAS
        created_by     INTEGER REFERENCES users(id) ON DELETE SET NULL,
        by_admin       INTEGER NOT NULL,          -- vom Admin angelegt (Benutzer kann sie nicht ändern)
        keep_days      INTEGER NOT NULL DEFAULT 30,
        enabled        INTEGER NOT NULL DEFAULT 1,
        last_run_at    INTEGER,
        last_ok_at     INTEGER,
        last_error     TEXT,
        files          INTEGER NOT NULL DEFAULT 0, -- beim letzten Lauf übertragen
        bytes          INTEGER NOT NULL DEFAULT 0,
        created_at     INTEGER NOT NULL
    );

    CREATE TABLE inbox_log (
        id            INTEGER PRIMARY KEY,
        area          TEXT NOT NULL,              -- Gemeinsam | Privat/<name>
        original_name TEXT NOT NULL,
        new_path      TEXT,                       -- relativ zu DATA_DIR
        live_path     TEXT,                       -- mitverschobenes Live-Video
        kind          TEXT NOT NULL,              -- photo | document
        status        TEXT NOT NULL,              -- done | error | undone
        message       TEXT,
        created_at    INTEGER NOT NULL
    );
    CREATE INDEX inbox_log_area ON inbox_log(area, created_at DESC);
    "#,
];

pub fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

impl From<rusqlite::Error> for AppError {
    fn from(e: rusqlite::Error) -> Self {
        if let rusqlite::Error::SqliteFailure(f, _) = &e {
            if f.code == rusqlite::ErrorCode::ConstraintViolation {
                return AppError::Conflict("Existiert bereits".into());
            }
        }
        AppError::Internal(format!("Datenbank: {e}"))
    }
}

pub struct Db {
    path: PathBuf,
    conn: Mutex<Connection>,
}

impl Db {
    pub fn open(path: &Path) -> Result<Arc<Self>, String> {
        let conn = Self::connect(path).map_err(|e| format!("{}: {e}", path.display()))?;
        migrate(&conn).map_err(|e| format!("Migration fehlgeschlagen: {e}"))?;
        Ok(Arc::new(Self { path: path.to_path_buf(), conn: Mutex::new(conn) }))
    }

    /// Neue Verbindung (z. B. für den Index-Thread, damit lange Scans die
    /// Web-Anfragen nicht blockieren).
    pub fn connect(path: &Path) -> rusqlite::Result<Connection> {
        let conn = Connection::open(path)?;
        conn.busy_timeout(Duration::from_secs(10))?;
        // WAL erlaubt Lesen während der Index schreibt. Falls das Dateisystem
        // das nicht kann, läuft SQLite im normalen Modus weiter.
        let mode: String = conn
            .pragma_update_and_check(None, "journal_mode", "WAL", |r| r.get(0))
            .unwrap_or_else(|_| "delete".into());
        if !mode.eq_ignore_ascii_case("wal") {
            tracing::warn!("SQLite läuft ohne WAL (Modus: {mode})");
        }
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        Ok(conn)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Führt `f` auf einem Blocking-Thread mit der gemeinsamen Verbindung aus.
    pub async fn call<T, F>(self: &Arc<Self>, f: F) -> Result<T, AppError>
    where
        T: Send + 'static,
        F: FnOnce(&mut Connection) -> Result<T, AppError> + Send + 'static,
    {
        let db = self.clone();
        tokio::task::spawn_blocking(move || {
            let mut conn = db.conn.lock().unwrap();
            f(&mut conn)
        })
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?
    }

    /// Synchrone Variante für den Programmstart.
    pub fn blocking<T>(&self, f: impl FnOnce(&mut Connection) -> T) -> T {
        f(&mut self.conn.lock().unwrap())
    }
}

fn migrate(conn: &Connection) -> rusqlite::Result<()> {
    let version: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
    for (i, sql) in MIGRATIONS.iter().enumerate().skip(version as usize) {
        let tx = conn.unchecked_transaction()?;
        tx.execute_batch(sql)?;
        tx.pragma_update(None, "user_version", i as i64 + 1)?;
        tx.commit()?;
        tracing::info!("Datenbank auf Version {} aktualisiert", i + 1);
    }
    Ok(())
}

pub fn get_setting(conn: &Connection, key: &str) -> rusqlite::Result<Option<String>> {
    conn.query_row("SELECT value FROM settings WHERE key = ?", [key], |r| r.get(0))
        .optional()
}

pub fn set_setting(conn: &Connection, key: &str, value: &str) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO settings (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        [key, value],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migration_ist_wiederholbar() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("t.db");
        let db = Db::open(&p).unwrap();
        db.blocking(|c| set_setting(c, "a", "1")).unwrap();
        drop(db);
        let db = Db::open(&p).unwrap();
        assert_eq!(db.blocking(|c| get_setting(c, "a")).unwrap().as_deref(), Some("1"));
        let v: i64 = db.blocking(|c| c.pragma_query_value(None, "user_version", |r| r.get(0))).unwrap();
        assert_eq!(v as usize, MIGRATIONS.len());
    }
}
