//! Glossary CRUD and retarget commands.

use tauri::{AppHandle, State};

use crate::dto::{err, term_to_dto, GlossaryPage, RenameChange, TermDto};
use crate::session::AppState;

use super::ops;

/// One page of the glossary, filtered and ordered by the database.
#[tauri::command]
pub async fn get_glossary_page(
    project_id: String,
    query: Option<String>,
    kind: Option<String>,
    offset: usize,
    limit: usize,
    state: State<'_, AppState>,
) -> Result<GlossaryPage, String> {
    let store = ops::project_store(&state, &project_id)?;
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
#[tauri::command]
pub async fn chapter_terms(
    project_id: String,
    index: usize,
    state: State<'_, AppState>,
) -> Result<Vec<TermDto>, String> {
    let store = ops::project_store(&state, &project_id)?;
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
    ops::glossary::upsert_term(&state, &project_id, term)
}

/// Remove a term from the glossary by its source form.
#[tauri::command]
pub async fn delete_term(
    project_id: String,
    source: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    ops::glossary::delete_term(&state, &project_id, &source)
}

/// Extract glossary terms from already-translated chapters and merge into the
/// existing glossary.
#[tauri::command]
pub async fn harvest_glossary(
    project_id: String,
    sample: usize,
    from_end: bool,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<usize, String> {
    ops::glossary::harvest(&app, &state, &project_id, sample, from_end).await
}

/// Propagate one or more renames into the already-translated text.
#[tauri::command]
pub fn retarget_terms(
    project_id: String,
    changes: Vec<RenameChange>,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    ops::glossary::retarget(&app, &state, &project_id, changes)
}
