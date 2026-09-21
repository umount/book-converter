//! Reference-translation operations.

use tauri::AppHandle;

use crate::dto::err;
use crate::jobs;
use crate::session::AppState;

use super::project_store;

pub(crate) fn use_as_base(
    app: &AppHandle,
    state: &AppState,
    project_id: &str,
) -> Result<usize, String> {
    let _slot = jobs::lease(app, state, project_id)?;
    project_store(state, project_id)?
        .restore_reference_chapters()
        .map_err(err)
}
