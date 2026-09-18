//! Project assistant: DeepSeek agent loop with tool-calling over Store/jobs.

mod executor;
mod prompt;
mod runtime;
mod tools;

pub use runtime::{AssistantRuntime, ConfirmDecision};

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use anyhow::{anyhow, Result};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::config::Config;
use crate::session::AppState;
use crate::state::Store;
use crate::translator::{ChatMessage, DeepSeekClient};

use self::executor::ToolExecutor;
use self::prompt::{build_snapshot, system_prompt};
use self::tools::{assistant_tools, tool_policy, ToolPolicy};

const MAX_STEPS: usize = 8;

#[derive(Debug, Clone, Serialize)]
pub struct HistoryMessage {
    pub id: i64,
    pub role: String,
    pub content: String,
    pub tool_name: Option<String>,
    pub tool_call_id: Option<String>,
}

/// Ensure the assistant history table exists (additive migration).
pub fn ensure_schema(store: &Store) -> Result<()> {
    store.conn().execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS assistant_messages (
            id INTEGER PRIMARY KEY,
            role TEXT NOT NULL,
            content TEXT NOT NULL DEFAULT '',
            tool_name TEXT,
            tool_call_id TEXT,
            created_at TEXT NOT NULL DEFAULT (datetime('now'))
        );
        "#,
    )?;
    Ok(())
}

pub fn load_history(store: &Store) -> Result<Vec<HistoryMessage>> {
    ensure_schema(store)?;
    let mut stmt = store.conn().prepare(
        "SELECT id, role, content, tool_name, tool_call_id
         FROM assistant_messages ORDER BY id ASC",
    )?;
    let rows = stmt
        .query_map([], |r| {
            Ok(HistoryMessage {
                id: r.get(0)?,
                role: r.get(1)?,
                content: r.get(2)?,
                tool_name: r.get(3)?,
                tool_call_id: r.get(4)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

pub fn clear_history(store: &Store) -> Result<()> {
    ensure_schema(store)?;
    store.conn().execute("DELETE FROM assistant_messages", [])?;
    Ok(())
}

fn append_history(
    store: &Store,
    role: &str,
    content: &str,
    tool_name: Option<&str>,
    tool_call_id: Option<&str>,
) -> Result<()> {
    ensure_schema(store)?;
    store.conn().execute(
        "INSERT INTO assistant_messages (role, content, tool_name, tool_call_id)
         VALUES (?1, ?2, ?3, ?4)",
        rusqlite::params![role, content, tool_name, tool_call_id],
    )?;
    Ok(())
}

/// Run one user turn to completion (may wait on confirms).
pub async fn run_turn(
    app: AppHandle,
    project_id: String,
    db: String,
    user_message: String,
    open_chapter: Option<usize>,
    cancel: Arc<AtomicBool>,
    runtime: Arc<AssistantRuntime>,
) -> Result<()> {
    let config = Config::load();
    let client = DeepSeekClient::new(config.clone())?;
    let store = Store::open(&db)?;
    ensure_schema(&store)?;

    append_history(&store, "user", &user_message, None, None)?;

    let job_running = app
        .try_state::<AppState>()
        .map(|s| s.with(&project_id, |sess| sess.running))
        .unwrap_or(false);
    let snapshot = build_snapshot(&store, &config, &project_id, open_chapter, job_running)?;
    let mut messages: Vec<ChatMessage> = vec![ChatMessage::system(system_prompt(&snapshot))];

    let history = load_history(&store)?;
    let prior: Vec<_> = history
        .iter()
        .rev()
        .skip(1)
        .take(40)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    for row in prior {
        match row.role.as_str() {
            "user" => messages.push(ChatMessage::user(&row.content)),
            "assistant" if row.tool_name.is_none() => {
                messages.push(ChatMessage::assistant_text(&row.content));
            }
            _ => {}
        }
    }
    messages.push(ChatMessage::user(&user_message));

    let executor = ToolExecutor {
        app: app.clone(),
        project_id: project_id.clone(),
        db: db.clone(),
    };

    for _step in 0..MAX_STEPS {
        if cancel.load(Ordering::Relaxed) {
            return Err(anyhow!("cancelled"));
        }

        let tools = assistant_tools();
        let completion = client.chat_tools(&messages, &tools).await?;

        if !completion.tool_calls.is_empty() {
            messages.push(ChatMessage::assistant_tools(completion.tool_calls.clone()));
            if let Some(text) = &completion.content {
                let _ = app.emit(
                    "assistant_step",
                    serde_json::json!({
                        "project": project_id,
                        "role": "assistant",
                        "content": text,
                    }),
                );
            }

            for call in &completion.tool_calls {
                if cancel.load(Ordering::Relaxed) {
                    return Err(anyhow!("cancelled"));
                }
                let policy = tool_policy(&call.function.name);
                let args_pretty = pretty_args(&call.function.arguments);

                let _ = app.emit(
                    "assistant_step",
                    serde_json::json!({
                        "project": project_id,
                        "role": "tool",
                        "tool_name": call.function.name,
                        "content": format!("→ {} {}", call.function.name, args_pretty),
                    }),
                );

                let allowed = match policy {
                    ToolPolicy::Forbidden => false,
                    ToolPolicy::Auto => true,
                    ToolPolicy::Confirm | ToolPolicy::Heavy => {
                        let decision = runtime
                            .request_confirm(
                                &app,
                                &project_id,
                                &call.function.name,
                                &args_pretty,
                                matches!(policy, ToolPolicy::Heavy),
                            )
                            .await;
                        match decision {
                            ConfirmDecision::Approved => true,
                            ConfirmDecision::Denied => {
                                let result = "user_denied";
                                append_history(
                                    &store,
                                    "tool",
                                    result,
                                    Some(&call.function.name),
                                    Some(&call.id),
                                )?;
                                messages.push(ChatMessage::tool_result(&call.id, result));
                                let _ = app.emit(
                                    "assistant_step",
                                    serde_json::json!({
                                        "project": project_id,
                                        "role": "tool",
                                        "tool_name": call.function.name,
                                        "content": result,
                                    }),
                                );
                                continue;
                            }
                            ConfirmDecision::Cancelled => return Err(anyhow!("cancelled")),
                        }
                    }
                };

                if !allowed {
                    let result = format!("error: tool '{}' is not available", call.function.name);
                    messages.push(ChatMessage::tool_result(&call.id, &result));
                    continue;
                }

                let result = match executor
                    .execute(&call.function.name, &call.function.arguments)
                    .await
                {
                    Ok(s) => s,
                    Err(e) => format!("error: {e:#}"),
                };
                let clipped = clip(&result, 6000);
                append_history(
                    &store,
                    "tool",
                    &clipped,
                    Some(&call.function.name),
                    Some(&call.id),
                )?;
                messages.push(ChatMessage::tool_result(&call.id, &clipped));
                let _ = app.emit(
                    "assistant_step",
                    serde_json::json!({
                        "project": project_id,
                        "role": "tool",
                        "tool_name": call.function.name,
                        "content": clip(&clipped, 800),
                    }),
                );
            }
            continue;
        }

        let text = completion
            .content
            .unwrap_or_else(|| "(no reply)".into());
        append_history(&store, "assistant", &text, None, None)?;
        let _ = app.emit(
            "assistant_step",
            serde_json::json!({
                "project": project_id,
                "role": "assistant",
                "content": text,
            }),
        );
        return Ok(());
    }

    Err(anyhow!("assistant hit the step limit ({MAX_STEPS})"))
}

fn pretty_args(raw: &str) -> String {
    serde_json::from_str::<serde_json::Value>(raw)
        .map(|v| serde_json::to_string_pretty(&v).unwrap_or_else(|_| raw.to_string()))
        .unwrap_or_else(|_| raw.to_string())
}

fn clip(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let clipped: String = s.chars().take(max).collect();
    format!("{clipped}…")
}
