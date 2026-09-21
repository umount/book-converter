//! Replay stored assistant rows into the OpenAI message list.

use crate::state::assistant::{AssistantMessage, AssistantRole};
use crate::translator::{ChatMessage, ToolCall};

use super::HistoryMessage;

const TOOL_RESULT_REPLAY_CHARS: usize = 2000;

/// Rebuild the OpenAI message list from stored rows.
/// A `tool` row whose `tool_calls` parent is missing is dropped: the API
/// rejects an orphan tool result. An assistant `tool_calls` row whose results
/// are incomplete is replayed as text only, so a cancelled turn cannot 400
/// the next request.
pub(crate) fn replay(rows: &[AssistantMessage]) -> Vec<ChatMessage> {
    let mut out = Vec::new();
    let mut known_calls: Vec<String> = Vec::new();
    let finished_ids: std::collections::HashSet<&str> = rows
        .iter()
        .filter(|r| r.role == AssistantRole::Tool)
        .filter_map(|r| r.tool_call_id.as_deref())
        .collect();
    for row in rows {
        match row.role {
            AssistantRole::User => out.push(ChatMessage::user(&row.content)),
            AssistantRole::Assistant => {
                if let Some(raw) = &row.tool_calls {
                    if let Ok(calls) = serde_json::from_str::<Vec<ToolCall>>(raw) {
                        let complete = calls
                            .iter()
                            .all(|c| finished_ids.contains(c.id.as_str()));
                        if complete {
                            known_calls.extend(calls.iter().map(|c| c.id.clone()));
                            if row.content.is_empty() {
                                out.push(ChatMessage::assistant_tools(calls));
                            } else {
                                out.push(ChatMessage::assistant_turn(
                                    Some(row.content.clone()),
                                    calls,
                                ));
                            }
                            continue;
                        }
                    }
                }
                if !row.content.is_empty() {
                    out.push(ChatMessage::assistant_text(&row.content));
                }
            }
            AssistantRole::Tool => {
                let Some(id) = &row.tool_call_id else { continue };
                if !known_calls.iter().any(|k| k == id) {
                    continue;
                }
                out.push(ChatMessage::tool_result(id, clip(&row.content, TOOL_RESULT_REPLAY_CHARS)));
            }
            AssistantRole::SystemNote => {}
        }
    }
    out
}

pub(crate) fn to_dto(rows: Vec<AssistantMessage>) -> Vec<HistoryMessage> {
    rows.into_iter()
        .map(|r| HistoryMessage {
            id: r.id,
            role: r.role.as_str().to_string(),
            content: r.content,
            tool_name: r.tool_name,
            tool_call_id: r.tool_call_id,
        })
        .collect()
}

fn clip(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let clipped: String = s.chars().take(max).collect();
    format!("{clipped}…")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::assistant::AssistantMessage;

    fn row(
        id: i64,
        role: AssistantRole,
        content: &str,
        tool_calls: Option<&str>,
        tool_call_id: Option<&str>,
    ) -> AssistantMessage {
        AssistantMessage {
            id,
            turn: 1,
            role,
            content: content.into(),
            tool_name: None,
            tool_call_id: tool_call_id.map(str::to_string),
            tool_calls: tool_calls.map(str::to_string),
        }
    }

    #[test]
    fn replay_restores_tool_calls() {
        let calls = r#"[{"id":"c1","type":"function","function":{"name":"get_progress","arguments":"{}"}}]"#;
        let rows = [
            row(1, AssistantRole::User, "status?", None, None),
            row(2, AssistantRole::Assistant, "", Some(calls), None),
            row(3, AssistantRole::Tool, "{\"done\":1}", None, Some("c1")),
            row(4, AssistantRole::Assistant, "1 left", None, None),
        ];
        let msgs = replay(&rows);
        assert_eq!(msgs.len(), 4);
        assert!(msgs[1].tool_calls.is_some());
        assert_eq!(msgs[2].role, "tool");
        assert_eq!(msgs[3].content.as_deref(), Some("1 left"));
    }

    #[test]
    fn orphan_tool_row_is_dropped() {
        let rows = [row(
            1,
            AssistantRole::Tool,
            "x",
            None,
            Some("missing"),
        )];
        assert!(replay(&rows).is_empty());
    }

    #[test]
    fn incomplete_tool_calls_are_not_replayed() {
        let calls = r#"[{"id":"c1","type":"function","function":{"name":"get_progress","arguments":"{}"}}]"#;
        let rows = [
            row(1, AssistantRole::User, "status?", None, None),
            row(2, AssistantRole::Assistant, "checking", Some(calls), None),
        ];
        let msgs = replay(&rows);
        assert_eq!(msgs.len(), 2);
        assert!(msgs[1].tool_calls.is_none());
        assert_eq!(msgs[1].content.as_deref(), Some("checking"));
    }

    #[test]
    fn system_note_never_goes_to_the_model() {
        let rows = [row(1, AssistantRole::SystemNote, "cancelled", None, None)];
        assert!(replay(&rows).is_empty());
    }

    #[test]
    fn to_dto_keeps_visible_content() {
        let rows = vec![row(1, AssistantRole::User, "hi", None, None)];
        let dto = to_dto(rows);
        assert_eq!(dto[0].content, "hi");
        assert_eq!(dto[0].role, "user");
    }
}
