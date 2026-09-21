//! Project assistant: DeepSeek agent loop with tool-calling over Store/jobs.

mod args;
mod executor;
mod history;
mod prompt;
mod runtime;
mod tools;
mod turn;

pub use runtime::AssistantRuntime;
pub(crate) use runtime::TurnGuard;

use serde::Serialize;

use crate::state::Store;

#[derive(Debug, Clone, Serialize)]
pub struct HistoryMessage {
    pub id: i64,
    pub role: String,
    pub content: String,
    pub tool_name: Option<String>,
    pub tool_call_id: Option<String>,
}

pub fn load_history(store: &Store, limit: usize) -> anyhow::Result<Vec<HistoryMessage>> {
    Ok(history::to_dto(store.assistant_history(limit)?))
}

pub async fn run_turn(
    app: tauri::AppHandle,
    project_id: String,
    db: String,
    user_message: String,
    open_chapter: Option<usize>,
    handle: runtime::TurnHandle,
    runtime: std::sync::Arc<AssistantRuntime>,
) -> anyhow::Result<()> {
    turn::run(
        app,
        project_id,
        db,
        user_message,
        open_chapter,
        handle,
        runtime,
    )
    .await
}
