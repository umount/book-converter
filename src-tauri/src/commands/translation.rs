//! Translation job control commands.

use tauri::{AppHandle, Emitter, State};

use crate::config::Config;
use crate::dto::{err, Progress};
use crate::jobs::{run_translation, spawn_project_job};
use crate::session::AppState;
use crate::state::Store;
use crate::textutil;

/// Start translating pending chapters (up to `limit`) on a background thread.
#[tauri::command]
pub fn start_translation(
    project_id: String,
    limit: Option<usize>,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let (db, cancel) = state.begin_job(&project_id, "translation_running")?;
    spawn_project_job(
        app,
        project_id,
        cancel,
        move |app, project_id, cancel| async move {
            run_translation(&project_id, &db, limit, None, &cancel, &app).await
        },
        |app, project_id, ()| {
            let _ = app.emit("done", serde_json::json!({ "project": project_id }));
        },
    );

    Ok(())
}

/// Request a pause: the run stops after the current chapter.
#[tauri::command]
pub fn pause_translation(project_id: String, state: State<'_, AppState>) -> Result<(), String> {
    state.request_cancel(&project_id);
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
    state: State<'_, AppState>,
) -> Result<usize, String> {
    let (db, running) = state.with(&project_id, |s| (s.db_path.clone(), s.running));
    if running {
        return Err("job_running".into());
    }
    let db = db.ok_or("no_source")?;
    let store = Store::open(&db).map_err(err)?;
    let n = store.reset_from_number(from_number).map_err(err)?;
    // A full reset rebuilds context from scratch, so drop the rolling summary.
    if from_number.is_none() {
        let _ = store.set_meta("running_summary", "");
    }
    Ok(n)
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
    let (db, cancel) = state.begin_job(&project_id, "translation_running")?;
    spawn_project_job(
        app,
        project_id,
        cancel,
        move |app, project_id, cancel| async move {
            run_translation(
                &project_id,
                &db,
                None,
                Some(index),
                &cancel,
                &app,
            )
            .await
        },
        |app, project_id, ()| {
            let _ = app.emit("done", serde_json::json!({ "project": project_id }));
        },
    );

    Ok(())
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
    let db = state
        .with(&project_id, |s| s.db_path.clone())
        .ok_or("no_source")?;
    let store = Store::open(&db).map_err(err)?;
    if let Some(chapter) = store.chapter_full(index).map_err(err)? {
        if chapter.status == "in_progress" {
            return Err("chapter_busy".into());
        }
    }
    // Refuse to replace a translation with nothing.
    //
    // This is the editor's autosave path, so it fires on its own, and a UI race
    // that pairs a new chapter with a stale empty draft would silently destroy a
    // finished chapter while marking it done. That happened. Clearing a chapter
    // deliberately is what resetting it is for, so nothing legitimate is lost by
    // rejecting this here, and no future editor bug can do it either.
    if translated.trim().is_empty() && store.has_translation(index).map_err(err)? {
        tracing::warn!(chapter = index, "refused an empty overwrite of a translation");
        return Err("refuse_empty_overwrite".into());
    }
    let title = translated_title.trim();
    let body = translated.trim();
    let source = store
        .chapter(index)
        .map_err(err)?
        .map(|(_, source)| source)
        .unwrap_or_default();
    let issues = textutil::leftover_foreign(&Config::load().target_lang, title, body, &source);
    store
        .save_manual_translation(index, title, body, &issues)
        .map_err(err)?;
    Ok((!issues.is_empty()).then(|| issues.join(", ")))
}
