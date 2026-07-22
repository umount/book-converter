//! Per-project session state and project filesystem layout.

use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

use crate::export::fb2::Cover;
use crate::paths::app_data_dir;
use crate::reference::Reference;

/// Per-project session state.
#[derive(Default)]
pub struct Session {
    pub(crate) db_path: Option<String>,
    pub(crate) source_path: Option<String>,
    pub(crate) title: Option<String>,
    pub(crate) author: Option<String>,
    pub(crate) reference: Option<Reference>,
    pub(crate) style: Option<String>,
    pub(crate) cancel: Option<Arc<AtomicBool>>,
    pub(crate) running: bool,
    /// Source or reference was a `.zip` → default to a zipped output.
    pub(crate) zipped_input: bool,
    /// Translated book title shown in the UI and written to output.
    pub(crate) title_translated: Option<String>,
    /// Translated / transliterated author, so a Chinese name is not rendered as
    /// boxes in the Latin/Cyrillic-only PDF font.
    pub(crate) author_translated: Option<String>,
    /// Annotation / summary (auto from a source FB2, or edited by the user).
    pub(crate) summary: Option<String>,
    /// Cover image (auto from a source FB2, or replaced by the user).
    pub(crate) cover: Option<Cover>,
}

/// Managed app state: one `Session` per open project, keyed by project id, so
/// projects are isolated and can translate in parallel.
pub struct AppState(pub Mutex<HashMap<String, Session>>);

impl AppState {
    pub fn new() -> Self {
        AppState(Mutex::new(HashMap::new()))
    }

    /// Run `f` with the session for `id`, creating an empty one if absent.
    pub(crate) fn with<R>(&self, id: &str, f: impl FnOnce(&mut Session) -> R) -> R {
        let mut map = self.0.lock().unwrap();
        f(map.entry(id.to_string()).or_default())
    }
}

/// A project's own data directory (`<app_data>/projects/<id>/`).
pub(crate) fn project_dir(id: &str) -> std::path::PathBuf {
    app_data_dir().join("projects").join(id)
}

/// The resumable progress DB for a project. Kept in the app data directory (not
/// next to the book) so the source can live on a read-only mount without breaking.
pub(crate) fn db_path_for_project(id: &str) -> String {
    let dir = project_dir(id);
    let _ = std::fs::create_dir_all(&dir);
    dir.join("progress.db").to_string_lossy().into_owned()
}

/// Remove legacy flat `*.progress.db` files from before the per-project layout.
pub fn cleanup_legacy_data() {
    if let Ok(entries) = std::fs::read_dir(app_data_dir()) {
        for e in entries.flatten() {
            if e.file_name().to_string_lossy().ends_with(".progress.db") {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
}

/// Project manifest, stored as `project.json` and bundled into an archive so a
/// project is self-describing.
#[derive(Serialize, Deserialize, Default)]
pub(crate) struct Manifest {
    pub(crate) name: String,
    pub(crate) source_path: String,
    pub(crate) ref_path: Option<String>,
}

pub(crate) fn write_manifest(id: &str, source_path: &str, ref_path: Option<&str>) {
    let _ = std::fs::create_dir_all(project_dir(id));
    let name = Path::new(source_path)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("book")
        .to_string();
    let m = Manifest {
        name,
        source_path: source_path.to_string(),
        ref_path: ref_path.map(|s| s.to_string()),
    };
    if let Ok(bytes) = serde_json::to_vec_pretty(&m) {
        let _ = std::fs::write(project_dir(id).join("project.json"), bytes);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_dir_nests_under_app_data() {
        let p = project_dir("abc-123");
        assert_eq!(p.file_name().and_then(|s| s.to_str()), Some("abc-123"));
        assert!(p.to_string_lossy().contains("projects"));
        assert!(p.to_string_lossy().contains("book-converter"));
    }

    #[test]
    fn db_path_for_project_ends_with_progress_db() {
        let id = format!("test-{}", std::process::id());
        let db = db_path_for_project(&id);
        assert!(db.ends_with("progress.db"));
        assert!(db.contains(&id));
        // Clean up the empty dir created by the helper.
        let _ = std::fs::remove_dir_all(project_dir(&id));
    }
}

