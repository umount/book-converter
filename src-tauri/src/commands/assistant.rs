//! Project assistant IPC commands.

use std::sync::Arc;

use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

use crate::assistant::{self, AssistantRuntime, TurnGuard};
use crate::dto::err;
use crate::session::{validate_project_id, AppState};
use crate::state::Store;

#[derive(Serialize)]
pub struct PendingConfirmDto {
    pub id: String,
    pub tool: String,
    pub args: String,
    pub heavy: bool,
}

#[derive(Serialize)]
pub struct AssistantStateDto {
    pub running: bool,
    pub pending: Option<PendingConfirmDto>,
}

#[tauri::command]
pub async fn assistant_history(
    project_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<assistant::HistoryMessage>, String> {
    validate_project_id(&project_id).map_err(|e| e.to_string())?;
    let db = state
        .with(&project_id, |s| s.db_path.clone())
        .ok_or("no_source")?;
    let store = Store::open(&db).map_err(err)?;
    assistant::load_history(&store, 2000).map_err(err)
}

#[tauri::command]
pub async fn assistant_clear(
    project_id: String,
    state: State<'_, AppState>,
    runtime: State<'_, Arc<AssistantRuntime>>,
) -> Result<(), String> {
    validate_project_id(&project_id).map_err(|e| e.to_string())?;
    runtime.cancel(&project_id);
    let db = state
        .with(&project_id, |s| s.db_path.clone())
        .ok_or("no_source")?;
    let store = Store::open(&db).map_err(err)?;
    store.assistant_clear().map_err(err)
}

#[tauri::command]
pub async fn assistant_state(
    project_id: String,
    runtime: State<'_, Arc<AssistantRuntime>>,
) -> Result<AssistantStateDto, String> {
    validate_project_id(&project_id).map_err(|e| e.to_string())?;
    Ok(AssistantStateDto {
        running: runtime.is_running(&project_id),
        pending: runtime.pending(&project_id).map(|p| PendingConfirmDto {
            id: p.id,
            tool: p.tool,
            args: p.args,
            heavy: p.heavy,
        }),
    })
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
    validate_project_id(&project_id).map_err(|e| e.to_string())?;
    let text = message.trim().to_string();
    if text.is_empty() {
        return Err("empty_message".into());
    }
    let db = state
        .with(&project_id, |s| s.db_path.clone())
        .ok_or("no_source")?;
    let handle = runtime.begin_turn(&project_id)?;
    let runtime = runtime.inner().clone();
    let app2 = app.clone();
    let project = project_id.clone();

    std::thread::spawn(move || {
        let _guard = TurnGuard::new(runtime.clone(), project.clone());
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
                    handle,
                    runtime.clone(),
                ))
            });
        match result {
            Ok(()) => {
                let _ = app2.emit("assistant_done", serde_json::json!({ "project": project }));
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
    validate_project_id(&project_id).map_err(|e| e.to_string())?;
    runtime.resolve_confirm(&project_id, &confirm_id, approved)
}

#[tauri::command]
pub fn assistant_cancel(
    project_id: String,
    runtime: State<'_, Arc<AssistantRuntime>>,
) -> Result<(), String> {
    validate_project_id(&project_id).map_err(|e| e.to_string())?;
    runtime.cancel(&project_id);
    Ok(())
}
