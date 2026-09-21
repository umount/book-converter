//! Translation job control commands.

use tauri::{AppHandle, State};

use crate::dto::{err, Progress};
use crate::session::AppState;
use crate::state::Store;

use super::ops;

/// Start translating pending chapters (up to `limit`) on a background thread.
#[tauri::command]
pub fn start_translation(
    project_id: String,
    limit: Option<usize>,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    ops::translation::start(&app, &state, &project_id, limit)
}

/// Request a pause: the run stops after the current chapter.
#[tauri::command]
pub fn pause_translation(project_id: String, state: State<'_, AppState>) -> Result<(), String> {
    ops::translation::pause(&state, &project_id);
    Ok(())
}

/// Current progress.
#[tauri::command]
pub async fn get_progress(
    project_id: String,
    state: State<'_, AppState>,
) -> Result<Progress, String> {
    let (db, running) = state.with(&project_id, |s| (s.db_path.clone(), s.running));
    let db = db.ok_or("no_source")?;
    let store = Store::open(&db).map_err(err)?;
    let st = store.stats().map_err(err)?;
    let next_number = store
        .next_pending()
        .map_err(err)?
        .map(|(idx, n)| n.unwrap_or(idx));
    let max_number = store.max_chapter_number().map_err(err)?;
    Ok(Progress {
        project: project_id,
        done: st.done,
        total: st.total,
        failed: st.failed,
        pending: st.pending,
        running,
        job_done: 0,
        job_total: 0,
        current_idx: None,
        current_number: None,
        current_title: None,
        next_number,
        max_number,
        phase: "status".into(),
        last_ms: None,
        eta_secs: None,
    })
}

/// Reset translated chapters back to `pending` for a fresh run with the current
/// glossary. `from_number` is the book chapter number from the title (e.g. 523 for
/// `第523章`); `None` resets the whole book and also clears the rolling context
/// summary. Returns how many chapters were reset. The caller then calls
/// `start_translation` to re-run them.
#[tauri::command]
pub async fn reset_translation(
    project_id: String,
    from_number: Option<usize>,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<usize, String> {
    ops::translation::reset(&app, &state, &project_id, from_number)
}

/// Translate a single chapter by index (reader action). Uses the previous
/// chapter's saved rolling context. Runs on a background thread like a normal job.
#[tauri::command]
pub fn translate_chapter(
    project_id: String,
    index: usize,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    ops::translation::translate_chapter(&app, &state, &project_id, index)
}

/// Manually save an edited chapter translation (origin = `manual`).
/// Allowed while a batch job runs, as long as this chapter is not the one
/// currently being translated (`in_progress`).
///
/// Returns leftover foreign-script words still in the saved text (same check as
/// the translation repair pass), or `None` when the warning can be cleared.
#[tauri::command]
pub async fn update_chapter_translation(
    project_id: String,
    index: usize,
    translated_title: String,
    translated: String,
    state: State<'_, AppState>,
) -> Result<Option<String>, String> {
    ops::translation::save_manual(&state, &project_id, index, &translated_title, &translated)
}
