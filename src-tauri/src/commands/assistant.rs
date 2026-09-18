//! Project assistant IPC commands.

use std::sync::Arc;

use tauri::{AppHandle, Emitter, State};

use crate::assistant::{self, AssistantRuntime};
use crate::dto::err;
use crate::session::AppState;
use crate::state::Store;

#[tauri::command]
pub async fn assistant_history(
    project_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<assistant::HistoryMessage>, String> {
    let db = state
        .with(&project_id, |s| s.db_path.clone())
        .ok_or("no_source")?;
    let store = Store::open(&db).map_err(err)?;
    assistant::load_history(&store).map_err(err)
}

#[tauri::command]
pub async fn assistant_clear(
    project_id: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let db = state
        .with(&project_id, |s| s.db_path.clone())
        .ok_or("no_source")?;
    let store = Store::open(&db).map_err(err)?;
    assistant::clear_history(&store).map_err(err)
}

/// Start an assistant turn on a background thread (may wait on confirms).
#[tauri::command]
pub fn assistant_send(
    project_id: String,
    message: String,
    open_chapter: Option<usize>,
    app: AppHandle,
    state: State<'_, AppState>,
    runtime: State<'_, Arc<AssistantRuntime>>,
) -> Result<(), String> {
    let text = message.trim().to_string();
    if text.is_empty() {
        return Err("empty_message".into());
    }
    let db = state
        .with(&project_id, |s| s.db_path.clone())
        .ok_or("no_source")?;
    let cancel = runtime.begin_turn(&project_id)?;
    let runtime = runtime.inner().clone();
    let app2 = app.clone();
    let project = project_id.clone();

    std::thread::spawn(move || {
        let result = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(anyhow::Error::from)
            .and_then(|rt| {
                rt.block_on(assistant::run_turn(
                    app2.clone(),
                    project.clone(),
                    db,
                    text,
                    open_chapter,
                    cancel,
                    runtime.clone(),
                ))
            });
        runtime.finish_turn(&project);
        match result {
            Ok(()) => {
                let _ = app2.emit(
                    "assistant_done",
                    serde_json::json!({ "project": project }),
                );
            }
            Err(e) => {
                let _ = app2.emit(
                    "assistant_error",
                    serde_json::json!({
                        "project": project,
                        "message": e.to_string(),
                    }),
                );
            }
        }
    });
    Ok(())
}

#[tauri::command]
pub fn assistant_approve(
    project_id: String,
    confirm_id: String,
    approved: bool,
    runtime: State<'_, Arc<AssistantRuntime>>,
) -> Result<(), String> {
    runtime.resolve_confirm(&project_id, &confirm_id, approved)
}

#[tauri::command]
pub fn assistant_cancel(
    project_id: String,
    runtime: State<'_, Arc<AssistantRuntime>>,
) -> Result<(), String> {
    runtime.cancel(&project_id);
    Ok(())
}
