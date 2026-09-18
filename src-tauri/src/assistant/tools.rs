//! Tool allowlist and OpenAI-compatible tool schemas for the assistant.

use serde_json::json;

use crate::translator::ToolSpec;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolPolicy {
    Auto,
    Confirm,
    Heavy,
    Forbidden,
}

pub fn tool_policy(name: &str) -> ToolPolicy {
    match name {
        "get_progress" | "list_chapters" | "get_chapter" | "search_book"
        | "get_glossary_page" | "chapter_terms" | "get_book_details" | "get_reference_info" => {
            ToolPolicy::Auto
        }
        "start_translation" | "pause_translation" | "translate_chapter" | "update_term"
        | "delete_term" | "retarget_terms" | "harvest_glossary" | "bootstrap_glossary"
        | "update_chapter_translation" | "set_chapter_prompt" | "set_chapter_context"
        | "replace_in_book" => ToolPolicy::Confirm,
        "reset_translation" | "use_reference_as_base" | "export_book" => ToolPolicy::Heavy,
        _ => ToolPolicy::Forbidden,
    }
}

fn tool(name: &str, description: &str, parameters: serde_json::Value) -> ToolSpec {
    ToolSpec {
        kind: "function",
        function: crate::translator::deepseek::ToolFunction {
            name: name.into(),
            description: description.into(),
            parameters,
        },
    }
}

/// All tools exposed to the model.
pub fn assistant_tools() -> Vec<ToolSpec> {
    vec![
        tool(
            "get_progress",
            "Current translation progress counts and whether a job is running.",
            json!({ "type": "object", "properties": {}, "additionalProperties": false }),
        ),
        tool(
            "list_chapters",
            "List chapters (reading-order idx, book number, titles, status, lang_issues). Optional filters.",
            json!({
                "type": "object",
                "properties": {
                    "status": { "type": "string", "description": "pending|done|failed|in_progress" },
                    "only_issues": { "type": "boolean", "description": "Only chapters with lang_issues" },
                    "limit": { "type": "integer", "description": "Max rows (default 40)" },
                    "offset": { "type": "integer" }
                },
                "additionalProperties": false
            }),
        ),
        tool(
            "get_chapter",
            "Load one chapter by reading-order idx (source + translation, truncated if huge).",
            json!({
                "type": "object",
                "properties": {
                    "index": { "type": "integer" }
                },
                "required": ["index"],
                "additionalProperties": false
            }),
        ),
        tool(
            "search_book",
            "Search translations (or source) for a query.",
            json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string" },
                    "in_source": { "type": "boolean" },
                    "match_case": { "type": "boolean" },
                    "whole_word": { "type": "boolean" }
                },
                "required": ["query"],
                "additionalProperties": false
            }),
        ),
        tool(
            "get_glossary_page",
            "Paged glossary lookup.",
            json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string" },
                    "kind": { "type": "string" },
                    "offset": { "type": "integer" },
                    "limit": { "type": "integer" }
                },
                "additionalProperties": false
            }),
        ),
        tool(
            "chapter_terms",
            "Glossary terms that appear in a chapter's source text.",
            json!({
                "type": "object",
                "properties": { "index": { "type": "integer" } },
                "required": ["index"],
                "additionalProperties": false
            }),
        ),
        tool(
            "get_book_details",
            "Title, author, summary metadata.",
            json!({ "type": "object", "properties": {}, "additionalProperties": false }),
        ),
        tool(
            "get_reference_info",
            "Reference translation import stats.",
            json!({ "type": "object", "properties": {}, "additionalProperties": false }),
        ),
        tool(
            "start_translation",
            "Start translating pending chapters. Optional limit.",
            json!({
                "type": "object",
                "properties": { "limit": { "type": "integer" } },
                "additionalProperties": false
            }),
        ),
        tool(
            "pause_translation",
            "Request pause after the current chapter.",
            json!({ "type": "object", "properties": {}, "additionalProperties": false }),
        ),
        tool(
            "translate_chapter",
            "Translate or retranslate a single chapter by reading-order idx.",
            json!({
                "type": "object",
                "properties": { "index": { "type": "integer" } },
                "required": ["index"],
                "additionalProperties": false
            }),
        ),
        tool(
            "reset_translation",
            "Reset chapters to pending from a book chapter number (or whole book if omitted). DESTRUCTIVE.",
            json!({
                "type": "object",
                "properties": {
                    "from_number": { "type": "integer", "description": "Book chapter number; omit to reset all" }
                },
                "additionalProperties": false
            }),
        ),
        tool(
            "update_term",
            "Create or update a glossary term (pinned).",
            json!({
                "type": "object",
                "properties": {
                    "source": { "type": "string" },
                    "target": { "type": "string" },
                    "kind": { "type": "string" },
                    "frequency": { "type": "integer" }
                },
                "required": ["source", "target"],
                "additionalProperties": false
            }),
        ),
        tool(
            "delete_term",
            "Delete a glossary term by source form.",
            json!({
                "type": "object",
                "properties": { "source": { "type": "string" } },
                "required": ["source"],
                "additionalProperties": false
            }),
        ),
        tool(
            "retarget_terms",
            "Propagate glossary renames into translated text and rolling context.",
            json!({
                "type": "object",
                "properties": {
                    "changes": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "old_target": { "type": "string" },
                                "new_target": { "type": "string" },
                                "kind": { "type": "string" }
                            },
                            "required": ["old_target", "new_target"]
                        }
                    }
                },
                "required": ["changes"],
                "additionalProperties": false
            }),
        ),
        tool(
            "harvest_glossary",
            "Extract glossary terms from already-translated chapters.",
            json!({
                "type": "object",
                "properties": {
                    "sample": { "type": "integer" },
                    "from_end": { "type": "boolean" }
                },
                "additionalProperties": false
            }),
        ),
        tool(
            "bootstrap_glossary",
            "Bootstrap pinned glossary from reference translation pairs.",
            json!({
                "type": "object",
                "properties": { "sample": { "type": "integer" } },
                "additionalProperties": false
            }),
        ),
        tool(
            "update_chapter_translation",
            "Manually save an edited chapter translation.",
            json!({
                "type": "object",
                "properties": {
                    "index": { "type": "integer" },
                    "translated_title": { "type": "string" },
                    "translated": { "type": "string" }
                },
                "required": ["index", "translated"],
                "additionalProperties": false
            }),
        ),
        tool(
            "set_chapter_prompt",
            "Set per-chapter translation instruction.",
            json!({
                "type": "object",
                "properties": {
                    "index": { "type": "integer" },
                    "prompt": { "type": "string" }
                },
                "required": ["index", "prompt"],
                "additionalProperties": false
            }),
        ),
        tool(
            "set_chapter_context",
            "Set rolling summary + prev_tail used before translating this chapter.",
            json!({
                "type": "object",
                "properties": {
                    "index": { "type": "integer" },
                    "summary": { "type": "string" },
                    "prev_tail": { "type": "string" }
                },
                "required": ["index", "summary", "prev_tail"],
                "additionalProperties": false
            }),
        ),
        tool(
            "replace_in_book",
            "Literal/regex replace across all translations.",
            json!({
                "type": "object",
                "properties": {
                    "find": { "type": "string" },
                    "replace": { "type": "string" },
                    "match_case": { "type": "boolean" },
                    "whole_word": { "type": "boolean" },
                    "regex": { "type": "boolean" }
                },
                "required": ["find", "replace"],
                "additionalProperties": false
            }),
        ),
        tool(
            "use_reference_as_base",
            "Re-seed still-pending chapters from the reference translation. DESTRUCTIVE for pending slots.",
            json!({ "type": "object", "properties": {}, "additionalProperties": false }),
        ),
        tool(
            "export_book",
            "Export translated book to a filesystem path (extension selects format).",
            json!({
                "type": "object",
                "properties": { "out_path": { "type": "string" } },
                "required": ["out_path"],
                "additionalProperties": false
            }),
        ),
    ]
}