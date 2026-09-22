//! Reader chapter commands.

mod metadata;
mod search;

pub use metadata::*;
pub use search::*;

use tauri::State;

use crate::dto::{err, ChapterBlock, ChapterRow, ChapterView};
use crate::session::AppState;

use super::ops;

#[tauri::command]
pub async fn list_chapters(
    project_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<ChapterRow>, String> {
    let store = ops::project_store(&state, &project_id)?;
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
            kind: row.kind,
        })
        .collect())
}

#[tauri::command]
pub async fn get_chapter(
    project_id: String,
    index: usize,
    state: State<'_, AppState>,
) -> Result<ChapterView, String> {
    let store = ops::project_store(&state, &project_id)?;
    let row = store
        .chapter_full(index)
        .map_err(err)?
        .ok_or("chapter not found")?;
    let (rolling_summary, prev_tail) = store.context_before(index).map_err(err)?;
    // Only a chapter with pictures has stored blocks; prose is its own text.
    let blocks = store
        .chapter_blocks(index)
        .map_err(err)?
        .into_iter()
        .map(|block| ChapterBlock {
            ord: block.ord,
            kind: block.kind,
            text: block.text,
            translated: block.translated,
            asset: block
                .rel_path
                .as_deref()
                .and_then(asset_file)
                .map(|file| format!("{project_id}/{file}")),
            width: block.width,
            height: block.height,
        })
        .collect();
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
        kind: row.kind,
        blocks,
    })
}

/// File name part of a stored `assets/<file>` path.
fn asset_file(rel_path: &str) -> Option<&str> {
    rel_path.rsplit('/').next().filter(|f| !f.is_empty())
}

#[tauri::command]
pub async fn set_chapter_prompt(
    project_id: String,
    index: usize,
    prompt: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    ops::translation::set_prompt(&state, &project_id, index, &prompt)
}

#[tauri::command]
pub async fn set_chapter_context(
    project_id: String,
    index: usize,
    summary: String,
    prev_tail: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    ops::translation::set_context(&state, &project_id, index, &summary, &prev_tail)
}
