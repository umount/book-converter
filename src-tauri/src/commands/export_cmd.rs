//! Export translated book command.

use tauri::State;

use crate::session::AppState;

use super::ops;

/// Export the translated chapters to `out_path` (format inferred from extension).
#[tauri::command]
pub async fn export_book(
    project_id: String,
    out_path: String,
    state: State<'_, AppState>,
) -> Result<String, String> {
    ops::export::export(&state, &project_id, &out_path)
}
