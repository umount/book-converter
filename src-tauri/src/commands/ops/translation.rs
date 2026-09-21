//! Translation job and chapter-edit operations.

use tauri::{AppHandle, Emitter};

use crate::config::Config;
use crate::dto::err;
use crate::jobs::{self, run_translation};
use crate::session::AppState;
use crate::textutil;

use super::project_store;

pub(crate) fn start(
    app: &AppHandle,
    state: &AppState,
    project_id: &str,
    limit: Option<usize>,
) -> Result<(), String> {
    let slot = jobs::lease(app, state, project_id)?;
    let db = slot.db.clone();
    jobs::spawn(
        slot,
        move |app, project_id, cancel| async move {
            run_translation(&project_id, &db, limit, None, &cancel, &app).await
        },
        |app, project_id, ()| {
            let _ = app.emit("done", serde_json::json!({ "project": project_id }));
        },
    );
    Ok(())
}

pub(crate) fn pause(state: &AppState, project_id: &str) {
    state.request_cancel(project_id);
}

pub(crate) fn translate_chapter(
    app: &AppHandle,
    state: &AppState,
    project_id: &str,
    index: usize,
) -> Result<(), String> {
    let slot = jobs::lease(app, state, project_id)?;
    let db = slot.db.clone();
    jobs::spawn(
        slot,
        move |app, project_id, cancel| async move {
            run_translation(&project_id, &db, None, Some(index), &cancel, &app).await
        },
        |app, project_id, ()| {
            let _ = app.emit("done", serde_json::json!({ "project": project_id }));
        },
    );
    Ok(())
}

pub(crate) fn reset(
    app: &AppHandle,
    state: &AppState,
    project_id: &str,
    from_number: Option<usize>,
) -> Result<usize, String> {
    let _slot = jobs::lease(app, state, project_id)?;
    let store = project_store(state, project_id)?;
    let n = store.reset_from_number(from_number).map_err(err)?;
    if from_number.is_none() {
        let _ = store.set_meta("running_summary", "");
    }
    Ok(n)
}

/// Hand edits stay allowed while a batch job runs: the guard is the chapter's
/// own `in_progress` status, so this one deliberately takes no job lease.
pub(crate) fn save_manual(
    state: &AppState,
    project_id: &str,
    index: usize,
    title: &str,
    body: &str,
) -> Result<Option<String>, String> {
    let store = project_store(state, project_id)?;
    if let Some(chapter) = store.chapter_full(index).map_err(err)? {
        if chapter.status == "in_progress" {
            return Err("chapter_busy".into());
        }
    }
    if body.trim().is_empty() && store.has_translation(index).map_err(err)? {
        tracing::warn!(chapter = index, "refused an empty overwrite of a translation");
        return Err("refuse_empty_overwrite".into());
    }
    let title = title.trim();
    let body = body.trim();
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

pub(crate) fn set_prompt(
    state: &AppState,
    project_id: &str,
    index: usize,
    prompt: &str,
) -> Result<(), String> {
    project_store(state, project_id)?
        .set_chapter_user_prompt(index, prompt)
        .map_err(err)
}

pub(crate) fn set_context(
    state: &AppState,
    project_id: &str,
    index: usize,
    summary: &str,
    prev_tail: &str,
) -> Result<(), String> {
    project_store(state, project_id)?
        .set_context_before(index, summary, prev_tail)
        .map_err(err)
}
