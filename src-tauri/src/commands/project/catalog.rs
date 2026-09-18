//! Project catalog and deletion commands.

use tauri::State;

use crate::dto::{err, ProjectSummary};
use crate::session::{project_dir, AppState, Manifest};
use crate::state::Store;

#[tauri::command]
pub async fn list_projects() -> Result<Vec<ProjectSummary>, String> {
    let root = crate::paths::app_data_dir().join("projects");
    let Ok(entries) = std::fs::read_dir(&root) else {
        return Ok(Vec::new());
    };

    let mut projects = Vec::new();
    for entry in entries.flatten() {
        if !entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false) {
            continue;
        }
        let id = entry.file_name().to_string_lossy().into_owned();
        if crate::session::validate_project_id(&id).is_err() {
            continue;
        }
        let db = entry.path().join("progress.db");
        if !db.exists() {
            continue;
        }
        let Ok(store) = Store::open(&db.to_string_lossy()) else {
            continue;
        };
        let Ok(stats) = store.stats() else {
            continue;
        };
        if stats.total == 0 {
            continue;
        }

        let manifest: Manifest = std::fs::read(entry.path().join("project.json"))
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        let name = if manifest.name.trim().is_empty() {
            store
                .get_meta("title")
                .ok()
                .flatten()
                .filter(|title| !title.trim().is_empty())
                .unwrap_or_else(|| id.clone())
        } else {
            manifest.name.clone()
        };

        projects.push(ProjectSummary {
            id,
            name,
            source_path: manifest.source_path,
            ref_path: manifest.ref_path,
            total: stats.total,
            done: stats.done,
        });
    }
    projects.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(projects)
}

#[tauri::command]
pub async fn delete_project(
    project_id: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    state.remove(&project_id);
    let directory = project_dir(&project_id).map_err(err)?;
    if directory.exists() {
        std::fs::remove_dir_all(directory).map_err(err)?;
    }
    Ok(())
}
