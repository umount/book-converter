//! Glossary CRUD and retarget commands.

use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use tauri::{AppHandle, Emitter, Manager, State};

use crate::dto::{err, term_to_dto, RenameChange, TermDto};
use crate::glossary::{Term, TermKind};
use crate::jobs::run_retarget;
use crate::session::AppState;
use crate::state::Store;

/// The whole glossary (most frequent first).
#[tauri::command]
pub async fn get_glossary(
    project_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<TermDto>, String> {
    let db = state
        .with(&project_id, |s| s.db_path.clone())
        .ok_or("no_source")?;
    let store = Store::open(&db).map_err(err)?;
    let mut terms = store.load_glossary().map_err(err)?;
    terms.sort_by(|a, b| b.frequency.cmp(&a.frequency));
    Ok(terms.into_iter().map(term_to_dto).collect())
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
    let mut glossary = store.load_glossary().map_err(err)?;
    let updated = Term {
        source: term.source.clone(),
        target: term.target,
        kind: TermKind::from_label(&term.kind),
        frequency: term.frequency.max(1),
        pinned: true,
    };
    match glossary.iter_mut().find(|t| t.source == term.source) {
        Some(existing) => *existing = updated,
        None => glossary.push(updated),
    }
    store.save_glossary(&glossary).map_err(err)?;
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

    let client = super::util::client().map_err(err)?;
    let mut glossary = store.load_glossary().map_err(err)?;
    let before = glossary.len();

    for (idx, source, translated) in &chosen {
        match crate::translator::extract_terms(&client, source, translated, 2).await {
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
    let res: Result<_, String> = state.with(&project_id, |s| {
        if s.running {
            return Err("job_running".to_string());
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
        let result = rt.block_on(run_retarget(&pid, &db, &changes, &cancel, &app2));

        if let Some(st) = app2.try_state::<AppState>() {
            st.with(&pid, |s| s.running = false);
        }
        match result {
            Ok(n) => {
                let _ = app2.emit(
                    "retarget_done",
                    serde_json::json!({ "project": pid, "changed": n }),
                );
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
