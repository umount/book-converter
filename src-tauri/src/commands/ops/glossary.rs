//! Glossary CRUD, harvest, bootstrap, and retarget.

use tauri::{AppHandle, Emitter};

use crate::dto::{err, RenameChange, TermDto};
use crate::glossary::{Term, TermKind};
use crate::jobs::{self, run_retarget};
use crate::session::AppState;

use super::super::util::client;
use super::project_store;

pub(crate) fn upsert_term(state: &AppState, project_id: &str, term: TermDto) -> Result<(), String> {
    let store = project_store(state, project_id)?;
    store
        .upsert_term(&Term {
            source: term.source,
            target: term.target,
            kind: TermKind::from_label(&term.kind),
            frequency: term.frequency.max(1),
            pinned: term.pinned,
        })
        .map_err(err)
}

pub(crate) fn delete_term(state: &AppState, project_id: &str, source: &str) -> Result<(), String> {
    project_store(state, project_id)?
        .delete_term(source.trim())
        .map_err(err)
}

pub(crate) fn retarget(
    app: &AppHandle,
    state: &AppState,
    project_id: &str,
    changes: Vec<RenameChange>,
) -> Result<(), String> {
    let changes: Vec<RenameChange> = changes
        .into_iter()
        .filter(|c| !c.old_target.trim().is_empty() && c.new_target.trim() != c.old_target.trim())
        .collect();
    if changes.is_empty() {
        return Err("nothing_to_update".into());
    }
    let slot = jobs::lease(app, state, project_id)?;
    let db = slot.db.clone();
    jobs::spawn(
        slot,
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

pub(crate) async fn harvest(
    app: &AppHandle,
    state: &AppState,
    project_id: &str,
    sample: usize,
    from_end: bool,
) -> Result<usize, String> {
    let _slot = jobs::lease(app, state, project_id)?;
    let store = project_store(state, project_id)?;
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
    chosen.sort_by_key(|(idx, ..)| *idx);

    let config = crate::config::Config::load();
    let client = client()?;
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

pub(crate) async fn bootstrap(
    app: &AppHandle,
    state: &AppState,
    project_id: &str,
    sample: usize,
) -> Result<usize, String> {
    let _slot = jobs::lease(app, state, project_id)?;
    let store = project_store(state, project_id)?;
    let pairs = store.reference_pairs(sample.max(1)).map_err(err)?;
    if pairs.is_empty() {
        return Err("no_reference".into());
    }
    let cl = client()?;
    let config = crate::config::Config::load();
    let mut glossary = store.load_glossary().map_err(err)?;
    let extracted = crate::reference::bootstrap_glossary(&cl, &config, &pairs)
        .await
        .map_err(err)?;
    crate::glossary::merge(&mut glossary, extracted);
    store.save_glossary(&glossary).map_err(err)?;
    Ok(glossary.len())
}
