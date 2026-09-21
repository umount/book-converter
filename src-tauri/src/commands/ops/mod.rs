//! Shared project operations used by both IPC commands and the assistant.

pub(crate) mod export;
pub(crate) mod glossary;
pub(crate) mod reference;
pub(crate) mod text;
pub(crate) mod translation;

use crate::dto::err;
use crate::session::AppState;
use crate::state::Store;

pub(crate) fn project_db(state: &AppState, project_id: &str) -> Result<String, String> {
    state
        .with(project_id, |session| session.db_path.clone())
        .ok_or_else(|| "no_source".into())
}

pub(crate) fn project_store(state: &AppState, project_id: &str) -> Result<Store, String> {
    Store::open(&project_db(state, project_id)?).map_err(err)
}
