//! Glossary CRUD and retarget commands.

use tauri::{AppHandle, Emitter, State};

use crate::dto::{err, term_to_dto, GlossaryPage, RenameChange, TermDto};
use crate::glossary::{Term, TermKind};
use crate::jobs::{run_retarget, spawn_project_job};
use crate::session::AppState;
use crate::state::Store;

/// One page of the glossary, filtered and ordered by the database.
///
/// The glossary of a long book runs to tens of thousands of terms. Shipping all
/// of them to the UI and filtering there froze the app, so the window, the
/// filter and the ordering are all the database's job; the UI asks for what it
/// is about to draw.
#[tauri::command]
pub async fn get_glossary_page(
    project_id: String,
    query: Option<String>,
    kind: Option<String>,
    offset: usize,
    limit: usize,
    state: State<'_, AppState>,
) -> Result<GlossaryPage, String> {
    let db = state
        .with(&project_id, |s| s.db_path.clone())
        .ok_or("no_source")?;
    let store = Store::open(&db).map_err(err)?;
    let (total, terms) = store
        .glossary_page(
            query.as_deref().unwrap_or_default(),
            kind.as_deref(),
            offset,
            limit.clamp(1, 1000),
        )
        .map_err(err)?;
    Ok(GlossaryPage {
        total,
        terms: terms.into_iter().map(term_to_dto).collect(),
    })
}

/// The glossary terms that occur in one chapter's original text.
///
/// This is what the reader needs to underline terms and map a source term to
/// the rendering used in the translation. It used to receive the entire
/// glossary and scan it against the chapter on every render, which is the same
/// work the translation prompt already does through `glossary::relevant_terms`,
/// so that function is reused here.
#[tauri::command]
pub async fn chapter_terms(
    project_id: String,
    index: usize,
    state: State<'_, AppState>,
) -> Result<Vec<TermDto>, String> {
    let db = state
        .with(&project_id, |s| s.db_path.clone())
        .ok_or("no_source")?;
    let store = Store::open(&db).map_err(err)?;
    let Some((_, source)) = store.chapter(index).map_err(err)? else {
        return Ok(Vec::new());
    };
    let glossary = store.load_glossary().map_err(err)?;
    Ok(crate::glossary::relevant_terms(&glossary, &source)
        .into_iter()
        .cloned()
        .map(term_to_dto)
        .collect())
}

/// Manually edit / pin a term.
#[tauri::command]
pub async fn update_term(
    project_id: String,
    term: TermDto,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let db = state
        .with(&project_id, |s| s.db_path.clone())
        .ok_or("no_source")?;
    let store = Store::open(&db).map_err(err)?;
    // One row, not the whole glossary: this runs on every edit in the table.
    store
        .upsert_term(&Term {
            source: term.source,
            target: term.target,
            kind: TermKind::from_label(&term.kind),
            frequency: term.frequency.max(1),
            pinned: true,
        })
        .map_err(err)?;
    Ok(())
}

/// Remove a term from the glossary by its source form.
#[tauri::command]
pub async fn delete_term(
    project_id: String,
    source: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let db = state
        .with(&project_id, |s| s.db_path.clone())
        .ok_or("no_source")?;
    let store = Store::open(&db).map_err(err)?;
    store.delete_term(source.trim()).map_err(err)?;
    Ok(())
}

/// Extract glossary terms from already-translated chapters and merge into the
/// existing glossary. `from_end = false` takes the first `sample` done chapters
/// (book opening); `from_end = true` takes the last `sample` (most recently
/// translated — keeps current names from going cold). Returns the glossary size
/// after the merge.
#[tauri::command]
pub async fn harvest_glossary(
    project_id: String,
    sample: usize,
    from_end: bool,
    state: State<'_, AppState>,
) -> Result<usize, String> {
    let (db, running) = state.with(&project_id, |s| (s.db_path.clone(), s.running));
    if running {
        return Err("job_running".into());
    }
    let db = db.ok_or("no_source")?;
    let store = Store::open(&db).map_err(err)?;
    let pairs = store.done_chapter_pairs().map_err(err)?;
    if pairs.is_empty() {
        return Err("nothing_translated".into());
    }

    let n = sample.max(1);
    let mut chosen: Vec<(usize, String, String)> = if from_end {
        pairs.into_iter().rev().take(n).collect()
    } else {
        pairs.into_iter().take(n).collect()
    };
    // Process in reading order for stable logs / extraction context.
    chosen.sort_by_key(|(idx, ..)| *idx);

    let config = crate::config::Config::load();
    let client = super::util::client().map_err(err)?;
    let mut glossary = store.load_glossary().map_err(err)?;
    let before = glossary.len();

    for (idx, source, translated) in &chosen {
        match crate::translator::extract_terms(&client, &config, source, translated, 2).await {
            Ok(terms) => {
                crate::glossary::merge(&mut glossary, terms);
                tracing::info!(
                    chapter = idx,
                    terms = glossary.len(),
                    from_end,
                    "harvested glossary from translated chapter"
                );
            }
            Err(e) => {
                tracing::warn!(chapter = idx, "harvest extraction failed: {e:#}");
            }
        }
    }

    store.save_glossary(&glossary).map_err(err)?;
    tracing::info!(
        before,
        after = glossary.len(),
        chapters = chosen.len(),
        from_end,
        "glossary harvest finished"
    );
    Ok(glossary.len())
}

/// Propagate one or more renames into the already-translated text: rewrite (via
/// the model) only the paragraphs that mention an old rendering, replacing every
/// inflected form with the new one and fixing gender/case agreement. Runs on a
/// background thread; emits `retarget_progress` and finally `retarget_done`
/// (chapters changed).
#[tauri::command]
pub fn retarget_terms(
    project_id: String,
    changes: Vec<RenameChange>,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let changes: Vec<RenameChange> = changes
        .into_iter()
        .filter(|c| !c.old_target.trim().is_empty() && c.new_target.trim() != c.old_target.trim())
        .collect();
    if changes.is_empty() {
        return Err("nothing_to_update".into());
    }
    let (db, cancel) = state.begin_job(&project_id, "job_running")?;
    spawn_project_job(
        app,
        project_id,
        cancel,
        move |app, project_id, cancel| async move {
            run_retarget(&project_id, &db, &changes, &cancel, &app).await
        },
        |app, project_id, changed| {
            let _ = app.emit(
                "retarget_done",
                serde_json::json!({ "project": project_id, "changed": changed }),
            );
        },
    );
    Ok(())
}
