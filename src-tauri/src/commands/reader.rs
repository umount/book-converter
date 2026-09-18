//! Reader chapter commands.

mod metadata;
mod search;

pub use metadata::*;
pub use search::*;

use tauri::State;

use crate::dto::{err, ChapterRow, ChapterView};
use crate::session::AppState;
use crate::state::Store;

#[tauri::command]
pub async fn list_chapters(
    project_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<ChapterRow>, String> {
    let store = project_store(&project_id, &state)?;
    Ok(store
        .list_chapters()
        .map_err(err)?
        .into_iter()
        .map(|row| ChapterRow {
            idx: row.idx,
            number: row.number,
            title: row.title,
            translated_title: row.translated_title,
            status: row.status,
            origin: row.origin,
            lang_issues: row.lang_issues,
        })
        .collect())
}

#[tauri::command]
pub async fn get_chapter(
    project_id: String,
    index: usize,
    state: State<'_, AppState>,
) -> Result<ChapterView, String> {
    let store = project_store(&project_id, &state)?;
    let row = store
        .chapter_full(index)
        .map_err(err)?
        .ok_or("chapter not found")?;
    let (rolling_summary, prev_tail) = store.context_before(index).map_err(err)?;
    Ok(ChapterView {
        idx: index,
        number: row.number,
        source_title: row.source_title,
        source: row.source,
        translated_title: row.translated_title,
        translated: row.translated,
        status: row.status,
        origin: row.origin,
        user_prompt: row.user_prompt,
        rolling_summary: (!rolling_summary.trim().is_empty()).then_some(rolling_summary),
        prev_tail,
    })
}

#[tauri::command]
pub async fn set_chapter_prompt(
    project_id: String,
    index: usize,
    prompt: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    project_store(&project_id, &state)?
        .set_chapter_user_prompt(index, &prompt)
        .map_err(err)
}

#[tauri::command]
pub async fn set_chapter_context(
    project_id: String,
    index: usize,
    summary: String,
    prev_tail: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    project_store(&project_id, &state)?
        .set_context_before(index, &summary, &prev_tail)
        .map_err(err)
}

fn project_store(project_id: &str, state: &State<'_, AppState>) -> Result<Store, String> {
    let db = state
        .with(project_id, |session| session.db_path.clone())
        .ok_or("no_source")?;
    Store::open(&db).map_err(err)
}
