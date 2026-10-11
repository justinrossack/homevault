//! Sichere Auflösung von Benutzerpfaden innerhalb von DATA_DIR.
//!
//! Jeder Pfad aus einem Request ist relativ zum Datenordner. Erlaubt sind nur
//! normale Pfadsegmente, also kein `..`, kein absoluter Pfad und keine
//! Symlinks, die aus dem Datenordner herauszeigen.

use std::path::{Component, Path, PathBuf};

use crate::error::AppError;

/// Wandelt einen relativen Benutzerpfad (z. B. "Urlaub/2024") in einen
/// geprüften absoluten Pfad um. `root` muss bereits kanonisch sein.
///
/// Das Ziel selbst muss nicht existieren (für Upload/Ordner anlegen), aber
/// sein Elternordner wird aufgelöst und muss innerhalb von `root` liegen.
pub fn resolve(root: &Path, user_path: &str) -> Result<PathBuf, AppError> {
    let rel = Path::new(user_path.trim_start_matches('/'));
    let mut clean = PathBuf::new();
    for comp in rel.components() {
        match comp {
            Component::Normal(seg) => {
                let s = seg.to_string_lossy();
                if s.contains('\\') || s.contains('\0') {
                    return Err(AppError::BadRequest("Ungültiger Pfad".into()));
                }
                clean.push(seg);
            }
            Component::CurDir => {}
            _ => return Err(AppError::BadRequest("Ungültiger Pfad".into())),
        }
    }

    let full = root.join(&clean);
    if clean.as_os_str().is_empty() {
        return Ok(root.to_path_buf());
    }

    // Existiert das Ziel, wird es komplett aufgelöst (Symlinks inklusive),
    // sonst nur der Elternordner.
    let check = match full.canonicalize() {
        Ok(c) => c,
        Err(_) => {
            let parent = full.parent().unwrap_or(root);
            let parent = parent
                .canonicalize()
                .map_err(|_| AppError::NotFound)?;
            parent.join(full.file_name().unwrap())
        }
    };
    if !check.starts_with(root) {
        return Err(AppError::BadRequest("Pfad außerhalb des Datenordners".into()));
    }
    Ok(check)
}

/// Bereich für gemeinsame Dateien (alle Benutzer) und Ordner der privaten
/// Bereiche (`Privat/<benutzer>`). Gleiche Namen wie die SMB-Freigaben.
pub const SHARED: &str = "Gemeinsam";
pub const PRIVATE: &str = "Privat";

/// Ein aufgelöster Pfad innerhalb eines Bereichs.
pub struct Resolved {
    pub space: &'static str,
    /// kanonischer Wurzelordner des Bereichs
    pub root: PathBuf,
    pub full: PathBuf,
}

impl Resolved {
    /// Pfad aus Sicht des Benutzers, z. B. "Gemeinsam/Urlaub/a.jpg".
    pub fn virtual_of(&self, p: &Path) -> String {
        let rel = relative(&self.root, p);
        if rel.is_empty() { self.space.to_owned() } else { format!("{}/{rel}", self.space) }
    }

    pub fn is_root(&self) -> bool {
        self.full == self.root
    }
}

/// Löst einen Benutzerpfad wie "Privat/Urlaub" auf. Der erste Teil wählt
/// den Bereich, jeder Benutzer sieht nur seinen eigenen privaten Ordner.
/// `None` bedeutet die virtuelle Übersicht (beide Bereiche).
pub fn resolve_for(data_dir: &Path, user: &str, user_path: &str) -> Result<Option<Resolved>, AppError> {
    let trimmed = user_path.trim_matches('/');
    if trimmed.is_empty() {
        return Ok(None);
    }
    let (first, rest) = trimmed.split_once('/').unwrap_or((trimmed, ""));
    let (space, root) = match first {
        SHARED => (SHARED, data_dir.join(SHARED)),
        PRIVATE => (PRIVATE, data_dir.join(PRIVATE).join(user)),
        _ => return Err(AppError::NotFound),
    };
    let root = root.canonicalize().map_err(|_| AppError::NotFound)?;
    if !root.starts_with(data_dir) {
        return Err(AppError::BadRequest("Pfad außerhalb des Datenordners".into()));
    }
    let full = resolve(&root, rest)?;
    Ok(Some(Resolved { space, root, full }))
}

/// Wie `resolve_for`, aber die Übersicht ist kein gültiges Ziel.
pub fn resolve_in_space(data_dir: &Path, user: &str, user_path: &str) -> Result<Resolved, AppError> {
    resolve_for(data_dir, user, user_path)?
        .ok_or_else(|| AppError::BadRequest("Bitte zuerst „Privat“ oder „Gemeinsam“ öffnen".into()))
}

/// Prüft einen einzelnen Dateinamen (ohne Ordner), z. B. beim Upload.
pub fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name != "."
        && name != ".."
        && !name.starts_with('.')
        && !name.contains(['/', '\\', '\0'])
        && name.len() <= 255
}

/// Relativer Pfad (mit `/`) von `full` aus Sicht von `root`.
pub fn relative(root: &Path, full: &Path) -> String {
    full.strip_prefix(root)
        .unwrap_or(full)
        .components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("data");
        std::fs::create_dir_all(root.join("fotos")).unwrap();
        std::fs::write(root.join("fotos/a.jpg"), b"x").unwrap();
        std::fs::write(dir.path().join("geheim.txt"), b"x").unwrap();
        let root = root.canonicalize().unwrap();
        (dir, root)
    }

    #[test]
    fn normale_pfade() {
        let (_d, root) = setup();
        assert_eq!(resolve(&root, "").unwrap(), root);
        assert_eq!(resolve(&root, "/").unwrap(), root);
        assert_eq!(resolve(&root, "fotos/a.jpg").unwrap(), root.join("fotos/a.jpg"));
        assert_eq!(resolve(&root, "/fotos/./a.jpg").unwrap(), root.join("fotos/a.jpg"));
        // Neue Datei in existierendem Ordner
        assert_eq!(resolve(&root, "fotos/neu.jpg").unwrap(), root.join("fotos/neu.jpg"));
    }

    #[test]
    fn traversal_wird_abgelehnt() {
        let (_d, root) = setup();
        assert!(resolve(&root, "..").is_err());
        assert!(resolve(&root, "../geheim.txt").is_err());
        assert!(resolve(&root, "fotos/../../geheim.txt").is_err());
        assert!(resolve(&root, "fotos/..\\..\\geheim.txt").is_err());
        // Neue Datei in nicht existierendem Ordner
        assert!(resolve(&root, "gibtsnicht/x.jpg").is_err());
    }

    #[cfg(unix)]
    #[test]
    fn symlink_nach_draussen_wird_abgelehnt() {
        let (d, root) = setup();
        std::os::unix::fs::symlink(d.path(), root.join("ausbruch")).unwrap();
        assert!(resolve(&root, "ausbruch/geheim.txt").is_err());
        assert!(resolve(&root, "ausbruch").is_err());
        assert!(resolve(&root, "ausbruch/neu.txt").is_err());
    }

    #[test]
    fn bereiche() {
        let d = tempfile::tempdir().unwrap();
        let data = d.path().canonicalize().unwrap();
        for p in ["Gemeinsam/Urlaub", "Privat/anna", "Privat/ben"] {
            std::fs::create_dir_all(data.join(p)).unwrap();
        }
        assert!(resolve_for(&data, "anna", "").unwrap().is_none());
        let r = resolve_for(&data, "anna", "Privat").unwrap().unwrap();
        assert_eq!(r.full, data.join("Privat/anna"));
        assert!(r.is_root());
        let r = resolve_for(&data, "anna", "/Gemeinsam/Urlaub/").unwrap().unwrap();
        assert_eq!(r.virtual_of(&r.full), "Gemeinsam/Urlaub");
        // fremder privater Ordner ist nicht erreichbar
        assert!(resolve_for(&data, "anna", "Privat/../ben").is_err());
        assert!(resolve_for(&data, "anna", "Privat/../../Privat/ben").is_err());
        assert!(resolve_for(&data, "anna", "Sonstiges").is_err());
        assert!(resolve_for(&data, "anna", "..").is_err());
        // Benutzer ohne Ordner
        assert!(resolve_for(&data, "carl", "Privat").is_err());
    }

    #[test]
    fn dateinamen() {
        assert!(valid_name("bild.jpg"));
        assert!(valid_name("Mein Urlaub (1).mp4"));
        assert!(!valid_name(""));
        assert!(!valid_name(".."));
        assert!(!valid_name(".versteckt"));
        assert!(!valid_name("a/b.jpg"));
        assert!(!valid_name("a\\b.jpg"));
    }
}
