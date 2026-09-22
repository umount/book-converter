//! Translation job and chapter-edit operations.

use tauri::{AppHandle, Emitter};

use crate::config::Config;
use crate::dto::err;
use crate::jobs::{self, run_translation};
use crate::session::AppState;
use crate::textutil;
use crate::translator::DeepSeekClient;

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
    // A page of pictures has no words to send to a text model, and the
    // assistant can ask for any chapter by number.
    if !project_store(state, project_id)?
        .chapter_kind(index)
        .map_err(err)?
        .is_translatable()
    {
        return Err("chapter_no_text".into());
    }
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
        tracing::warn!(
            chapter = index,
            "refused an empty overwrite of a translation"
        );
        return Err("refuse_empty_overwrite".into());
    }
    let title = title.trim();
    let body = body.trim();
    let source = store
        .chapter(index)
        .map_err(err)?
        .map(|(_, source)| source)
        .unwrap_or_default();
    let issues =
        textutil::leftover_foreign(&Config::load_for(&store).target_lang, title, body, &source);
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

pub(crate) fn set_book_prompt(
    state: &AppState,
    project_id: &str,
    prompt: &str,
) -> Result<(), String> {
    project_store(state, project_id)?
        .set_book_prompt(prompt)
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

/// Translate only a chapter's title. Does not take the job lease: a running
/// batch may continue, as long as this chapter is not the one in flight.
pub(crate) async fn translate_chapter_title(
    state: &AppState,
    project_id: &str,
    index: usize,
) -> Result<crate::dto::TitleTranslation, String> {
    let store = project_store(state, project_id)?;
    let chapter = store
        .chapter_full(index)
        .map_err(err)?
        .ok_or_else(|| "chapter not found".to_string())?;
    if chapter.status == "in_progress" {
        return Err("chapter_busy".into());
    }
    let source_title = chapter.source_title.trim().to_string();
    if source_title.is_empty() {
        return Err("no_title".into());
    }

    let config = Config::load_for(&store);
    let meta = store.project_metadata().map_err(err)?;
    let glossary = store.load_glossary().map_err(err)?;
    let (system, user) = {
        let relevant = crate::glossary::relevant_terms(&glossary, &source_title);
        let ctx = crate::translator::prompt::PromptContext {
            terms: &relevant,
            book_note: meta.book_prompt.as_deref(),
            user_note: chapter.user_prompt.as_deref(),
            book: Some(crate::translator::prompt::BookRef {
                title: meta.title.as_deref(),
                author: meta.author.as_deref(),
                title_translated: meta.title_translated.as_deref(),
            }),
            ..Default::default()
        };
        (
            crate::translator::prompt::system_prompt(&config),
            crate::translator::prompt::title_user_prompt(&ctx, &source_title),
        )
    };
    let raw = DeepSeekClient::new(config.clone())
        .map_err(err)?
        .translate(&system, &user)
        .await
        .map_err(err)?;
    let title = raw.trim().trim_matches('"').trim().to_string();
    if title.is_empty() {
        return Err("empty_title".into());
    }
    let body = chapter.translated.as_deref().unwrap_or("");
    let issues = textutil::leftover_foreign(&config.target_lang, &title, body, &chapter.source);
    store
        .set_translated_title(index, &title, &issues)
        .map_err(err)?;
    Ok(crate::dto::TitleTranslation {
        title,
        lang_issues: (!issues.is_empty()).then(|| issues.join(", ")),
    })
}
