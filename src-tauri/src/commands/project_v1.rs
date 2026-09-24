//! Versioned project commands delegate to UI-independent project services.
use crate::app::contracts::{AppError, ProjectDescriptor};
use crate::app::services::AppContext;

#[tauri::command]
pub fn project_inspect_manifest(
    context: tauri::State<'_, AppContext>,
    path: String,
) -> Result<ProjectDescriptor, AppError> {
    context.inspect_manifest(&path)
}
