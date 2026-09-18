//! Translation job control commands.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use tauri::{AppHandle, Emitter, Manager, State};

use crate::dto::{err, Progress};
use crate::jobs::run_job;
use crate::session::AppState;
use crate::state::Store;

/// Start translating pending chapters (up to `limit`) on a background thread.
#[tauri::command]
pub fn start_translation(
    project_id: String,
    limit: Option<usize>,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let res: Result<_, String> = state.with(&project_id, |s| {
        if s.running {
            return Err("translation_running".to_string());
        }
        let db = s.db_path.clone().ok_or("no_source")?;
        let cancel = Arc::new(AtomicBool::new(false));
        s.cancel = Some(cancel.clone());
        s.running = true;
        Ok((db, cancel))
    });
    let (db, cancel) = res?;

    let app2 = app.clone();
    let pid = project_id.clone();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("current-thread runtime");
        let result = rt.block_on(run_job(&pid, &db, limit, None, &cancel, &app2));

        if let Some(st) = app2.try_state::<AppState>() {
            st.with(&pid, |s| s.running = false);
        }
        match result {
            Ok(()) => {
                let _ = app2.emit("done", serde_json::json!({ "project": pid }));
            }
            Err(e) => {
                let _ = app2.emit(
                    "job_error",
                    serde_json::json!({ "project": pid, "message": e.to_string() }),
                );
            }
        }
    });

    Ok(())
}

/// Request a pause: the run stops after the current chapter.
#[tauri::command]
pub fn pause_translation(project_id: String, state: State<'_, AppState>) -> Result<(), String> {
    state.with(&project_id, |s| {
        if let Some(c) = &s.cancel {
            c.store(true, Ordering::Relaxed);
        }
    });
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
    let res: Result<_, String> = state.with(&project_id, |s| {
        if s.running {
            return Err("translation_running".to_string());
        }
        let db = s.db_path.clone().ok_or("no_source")?;
        let cancel = Arc::new(AtomicBool::new(false));
        s.cancel = Some(cancel.clone());
        s.running = true;
        Ok((db, cancel))
    });
    let (db, cancel) = res?;

    let app2 = app.clone();
    let pid = project_id.clone();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("current-thread runtime");
        let result = rt.block_on(run_job(
            &pid,
            &db,
            None,
            Some(index),
            &cancel,
            &app2,
        ));

        if let Some(st) = app2.try_state::<AppState>() {
            st.with(&pid, |s| s.running = false);
        }
        match result {
            Ok(()) => {
                let _ = app2.emit("done", serde_json::json!({ "project": pid }));
            }
            Err(e) => {
                let _ = app2.emit(
                    "job_error",
                    serde_json::json!({ "project": pid, "message": e.to_string() }),
                );
            }
        }
    });

    Ok(())
}

/// Manually save an edited chapter translation (origin = `manual`).
/// Allowed while a batch job runs, as long as this chapter is not the one
/// currently being translated (`in_progress`).
#[tauri::command]
pub async fn update_chapter_translation(
    project_id: String,
    index: usize,
    translated_title: String,
    translated: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
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
    store
        .save_manual_translation(index, translated_title.trim(), translated.trim())
        .map_err(err)?;
    Ok(())
}
