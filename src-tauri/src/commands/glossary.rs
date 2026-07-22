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
