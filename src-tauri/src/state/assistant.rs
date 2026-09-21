//! Assistant chat history: the project's own transcript of the agent loop.

use anyhow::Result;
use rusqlite::params;

use super::Store;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AssistantRole {
    User,
    Assistant,
    Tool,
    SystemNote,
}

impl AssistantRole {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            AssistantRole::User => "user",
            AssistantRole::Assistant => "assistant",
            AssistantRole::Tool => "tool",
            AssistantRole::SystemNote => "system_note",
        }
    }

    fn from_str(s: &str) -> AssistantRole {
        match s {
            "assistant" => AssistantRole::Assistant,
            "tool" => AssistantRole::Tool,
            "system_note" => AssistantRole::SystemNote,
            _ => AssistantRole::User,
        }
    }
}

/// One protocol message, mirroring `translator::ChatMessage`.
#[derive(Debug, Clone)]
pub(crate) struct AssistantMessage {
    pub(crate) id: i64,
    #[allow(dead_code)] // selected in SQL; tests assert the window is turn-based
    pub(crate) turn: i64,
    pub(crate) role: AssistantRole,
    pub(crate) content: String,
    pub(crate) tool_name: Option<String>,
    pub(crate) tool_call_id: Option<String>,
    /// JSON `Vec<ToolCall>` on assistant rows that requested tools.
    pub(crate) tool_calls: Option<String>,
}

pub(crate) struct NewAssistantMessage<'a> {
    pub(crate) turn: i64,
    pub(crate) role: AssistantRole,
    pub(crate) content: &'a str,
    pub(crate) tool_name: Option<&'a str>,
    pub(crate) tool_call_id: Option<&'a str>,
    pub(crate) tool_calls: Option<&'a str>,
}

fn map_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<AssistantMessage> {
    Ok(AssistantMessage {
        id: r.get(0)?,
        turn: r.get(1)?,
        role: AssistantRole::from_str(&r.get::<_, String>(2)?),
        content: r.get(3)?,
        tool_name: r.get(4)?,
        tool_call_id: r.get(5)?,
        tool_calls: r.get(6)?,
    })
}

impl Store {
    pub(crate) fn assistant_next_turn(&self) -> Result<i64> {
        let n: i64 = self
            .conn
            .query_row("SELECT COALESCE(MAX(turn), 0) + 1 FROM assistant_messages", [], |r| {
                r.get(0)
            })?;
        Ok(n)
    }

    pub(crate) fn assistant_append(&self, msg: NewAssistantMessage<'_>) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO assistant_messages (turn, role, content, tool_name, tool_call_id, tool_calls)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                msg.turn,
                msg.role.as_str(),
                msg.content,
                msg.tool_name,
                msg.tool_call_id,
                msg.tool_calls,
            ],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Whole transcript for the UI, oldest first, last `limit` rows.
    pub(crate) fn assistant_history(&self, limit: usize) -> Result<Vec<AssistantMessage>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, turn, role, content, tool_name, tool_call_id, tool_calls
             FROM assistant_messages
             WHERE NOT (role = 'assistant' AND content = '' AND tool_calls IS NOT NULL)
             ORDER BY id DESC
             LIMIT ?1",
        )?;
        let mut rows = stmt
            .query_map(params![limit as i64], map_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        rows.reverse();
        Ok(rows)
    }

    /// Model window: the last `turns` complete turns, no `system_note`.
    pub(crate) fn assistant_context(&self, turns: i64) -> Result<Vec<AssistantMessage>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, turn, role, content, tool_name, tool_call_id, tool_calls
             FROM assistant_messages
             WHERE role <> 'system_note'
               AND turn > (SELECT COALESCE(MAX(turn), 0) FROM assistant_messages) - ?1
             ORDER BY id ASC",
        )?;
        let rows = stmt
            .query_map(params![turns], map_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub(crate) fn assistant_clear(&self) -> Result<()> {
        self.conn.execute("DELETE FROM assistant_messages", [])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::Store;

    fn append(store: &Store, turn: i64, role: AssistantRole, content: &str, tool_calls: Option<&str>) {
        store
            .assistant_append(NewAssistantMessage {
                turn,
                role,
                content,
                tool_name: if role == AssistantRole::Tool {
                    Some("get_progress")
                } else {
                    None
                },
                tool_call_id: if role == AssistantRole::Tool {
                    Some("c1")
                } else {
                    None
                },
                tool_calls,
            })
            .unwrap();
    }

    #[test]
    fn history_round_trips_roles_and_order() {
        let store = Store::open(":memory:").unwrap();
        append(&store, 1, AssistantRole::User, "hi", None);
        append(&store, 1, AssistantRole::Assistant, "hello", None);
        let rows = store.assistant_history(40).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].role, AssistantRole::User);
        assert_eq!(rows[1].content, "hello");
    }

    #[test]
    fn context_window_keeps_whole_turns() {
        let store = Store::open(":memory:").unwrap();
        for turn in 1..=10 {
            append(&store, turn, AssistantRole::User, &format!("u{turn}"), None);
            append(&store, turn, AssistantRole::Assistant, &format!("a{turn}"), None);
        }
        let rows = store.assistant_context(3).unwrap();
        assert!(rows.iter().all(|r| r.turn >= 8));
        assert_eq!(rows.first().unwrap().role, AssistantRole::User);
        assert!(!matches!(rows.first().unwrap().role, AssistantRole::Tool));
    }

    #[test]
    fn context_skips_system_notes() {
        let store = Store::open(":memory:").unwrap();
        append(&store, 1, AssistantRole::User, "q", None);
        append(&store, 1, AssistantRole::SystemNote, "oops", None);
        append(&store, 1, AssistantRole::Assistant, "a", None);
        let rows = store.assistant_context(8).unwrap();
        assert!(rows.iter().all(|r| r.role != AssistantRole::SystemNote));
        assert_eq!(rows.len(), 2);
    }

    #[test]
    fn history_hides_empty_tool_call_rows() {
        let store = Store::open(":memory:").unwrap();
        append(&store, 1, AssistantRole::User, "q", None);
        append(
            &store,
            1,
            AssistantRole::Assistant,
            "",
            Some(r#"[{"id":"c1"}]"#),
        );
        append(&store, 1, AssistantRole::Assistant, "done", None);
        let rows = store.assistant_history(40).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[1].content, "done");
    }

    #[test]
    fn clear_empties_the_transcript() {
        let store = Store::open(":memory:").unwrap();
        append(&store, 1, AssistantRole::User, "q", None);
        store.assistant_clear().unwrap();
        assert!(store.assistant_history(40).unwrap().is_empty());
        assert_eq!(store.assistant_next_turn().unwrap(), 1);
    }
}
