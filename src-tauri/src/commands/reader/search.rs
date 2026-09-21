//! Book-wide search and deterministic replacement commands.

use tauri::{AppHandle, State};

use crate::dto::{err, SearchChapter};
use crate::session::AppState;
use crate::state::Store;

use super::ops;

#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub async fn replace_in_book(
    project_id: String,
    find: String,
    replace: String,
    match_case: bool,
    whole_word: bool,
    regex: bool,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<usize, String> {
    ops::text::replace(
        &app,
        &state,
        &project_id,
        &find,
        &replace,
        match_case,
        whole_word,
        regex,
    )
}

#[tauri::command]
pub async fn search_book(
    project_id: String,
    query: String,
    match_case: bool,
    whole_word: bool,
    regex: bool,
    in_source: bool,
    state: State<'_, AppState>,
) -> Result<Vec<SearchChapter>, String> {
    if query.trim().is_empty() {
        return Ok(Vec::new());
    }
    let db = ops::project_db(&state, &project_id)?;
    let regex = ops::text::build_regex(&query, match_case, whole_word, regex)?;
    blocking(move || {
        let store = Store::open(&db)?;
        ops::text::search(&store, &regex, in_source, ops::text::SearchLimits::UI)
    })
    .await
}

async fn blocking<T, F>(work: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce() -> anyhow::Result<T> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|error| err(anyhow::anyhow!("background task failed: {error}")))?
        .map_err(err)
}
