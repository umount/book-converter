//! Versioned project commands delegate to UI-independent project services.
use crate::app::contracts::{AppError, ProjectDescriptor};

#[tauri::command]
pub fn project_inspect_manifest(path: String) -> Result<ProjectDescriptor, AppError> {
    crate::project::inspect_manifest(std::path::Path::new(&path))
}
