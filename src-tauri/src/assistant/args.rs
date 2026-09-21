//! Typed tool arguments. Each struct's SCHEMA is what the model sees.

use serde::Deserialize;
use serde_json::Value;

pub(crate) trait ToolArgs: serde::de::DeserializeOwned {
    const SCHEMA: &'static str;
}

pub(crate) fn validator<T: ToolArgs>(v: &Value) -> Result<(), String> {
    serde_json::from_value::<T>(v.clone())
        .map(|_| ())
        .map_err(|e| e.to_string())
}

pub(crate) fn validator_replace_in_book(v: &Value) -> Result<(), String> {
    let a: ReplaceInBookArgs = serde_json::from_value(v.clone()).map_err(|e| e.to_string())?;
    if a.find.is_empty() {
        return Err("find must not be empty".into());
    }
    Ok(())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct EmptyArgs {}

impl ToolArgs for EmptyArgs {
    const SCHEMA: &'static str = r#"{"type":"object","properties":{},"additionalProperties":false}"#;
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ListChaptersArgs {
    #[serde(default)]
    pub(crate) status: Option<String>,
    #[serde(default)]
    pub(crate) only_issues: bool,
    #[serde(default = "default_list_limit")]
    pub(crate) limit: usize,
    #[serde(default)]
    pub(crate) offset: usize,
}

fn default_list_limit() -> usize {
    40
}

impl ToolArgs for ListChaptersArgs {
    const SCHEMA: &'static str = r#"{
        "type": "object",
        "properties": {
            "status": { "type": "string", "description": "pending|done|failed|in_progress" },
            "only_issues": { "type": "boolean" },
            "limit": { "type": "integer" },
            "offset": { "type": "integer" }
        },
        "additionalProperties": false
    }"#;
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct GetChapterArgs {
    pub(crate) index: usize,
}

impl ToolArgs for GetChapterArgs {
    const SCHEMA: &'static str = r#"{
        "type": "object",
        "properties": { "index": { "type": "integer" } },
        "required": ["index"],
        "additionalProperties": false
    }"#;
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SearchBookArgs {
    pub(crate) query: String,
    #[serde(default)]
    pub(crate) in_source: bool,
    #[serde(default)]
    pub(crate) match_case: bool,
    #[serde(default)]
    pub(crate) whole_word: bool,
    #[serde(default)]
    pub(crate) regex: bool,
}

impl ToolArgs for SearchBookArgs {
    const SCHEMA: &'static str = r#"{
        "type": "object",
        "properties": {
            "query": { "type": "string" },
            "in_source": { "type": "boolean" },
            "match_case": { "type": "boolean" },
            "whole_word": { "type": "boolean" },
            "regex": { "type": "boolean" }
        },
        "required": ["query"],
        "additionalProperties": false
    }"#;
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct GlossaryPageArgs {
    #[serde(default)]
    pub(crate) query: String,
    #[serde(default)]
    pub(crate) kind: Option<String>,
    #[serde(default)]
    pub(crate) offset: usize,
    #[serde(default = "default_glossary_limit")]
    pub(crate) limit: usize,
}

fn default_glossary_limit() -> usize {
    30
}

impl ToolArgs for GlossaryPageArgs {
    const SCHEMA: &'static str = r#"{
        "type": "object",
        "properties": {
            "query": { "type": "string" },
            "kind": { "type": "string" },
            "offset": { "type": "integer" },
            "limit": { "type": "integer" }
        },
        "additionalProperties": false
    }"#;
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ChapterTermsArgs {
    pub(crate) index: usize,
}

impl ToolArgs for ChapterTermsArgs {
    const SCHEMA: &'static str = r#"{
        "type": "object",
        "properties": { "index": { "type": "integer" } },
        "required": ["index"],
        "additionalProperties": false
    }"#;
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct StartTranslationArgs {
    #[serde(default)]
    pub(crate) limit: Option<usize>,
}

impl ToolArgs for StartTranslationArgs {
    const SCHEMA: &'static str = r#"{
        "type": "object",
        "properties": { "limit": { "type": "integer" } },
        "additionalProperties": false
    }"#;
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TranslateChapterArgs {
    pub(crate) index: usize,
}

impl ToolArgs for TranslateChapterArgs {
    const SCHEMA: &'static str = r#"{
        "type": "object",
        "properties": { "index": { "type": "integer" } },
        "required": ["index"],
        "additionalProperties": false
    }"#;
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ResetTranslationArgs {
    #[serde(default)]
    pub(crate) from_number: Option<usize>,
}

impl ToolArgs for ResetTranslationArgs {
    const SCHEMA: &'static str = r#"{
        "type": "object",
        "properties": {
            "from_number": { "type": "integer", "description": "Book chapter number; omit to reset all" }
        },
        "additionalProperties": false
    }"#;
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct UpdateTermArgs {
    pub(crate) source: String,
    pub(crate) target: String,
    #[serde(default = "default_kind")]
    pub(crate) kind: String,
    #[serde(default = "default_frequency")]
    pub(crate) frequency: u32,
}

fn default_kind() -> String {
    "term".into()
}
fn default_frequency() -> u32 {
    1
}

impl ToolArgs for UpdateTermArgs {
    const SCHEMA: &'static str = r#"{
        "type": "object",
        "properties": {
            "source": { "type": "string" },
            "target": { "type": "string" },
            "kind": { "type": "string" },
            "frequency": { "type": "integer" }
        },
        "required": ["source", "target"],
        "additionalProperties": false
    }"#;
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DeleteTermArgs {
    pub(crate) source: String,
}

impl ToolArgs for DeleteTermArgs {
    const SCHEMA: &'static str = r#"{
        "type": "object",
        "properties": { "source": { "type": "string" } },
        "required": ["source"],
        "additionalProperties": false
    }"#;
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RenameChangeArg {
    pub(crate) old_target: String,
    pub(crate) new_target: String,
    #[serde(default)]
    pub(crate) kind: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RetargetTermsArgs {
    pub(crate) changes: Vec<RenameChangeArg>,
}

impl ToolArgs for RetargetTermsArgs {
    const SCHEMA: &'static str = r#"{
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
    }"#;
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct HarvestGlossaryArgs {
    #[serde(default = "default_sample")]
    pub(crate) sample: usize,
    #[serde(default)]
    pub(crate) from_end: bool,
}

fn default_sample() -> usize {
    20
}

impl ToolArgs for HarvestGlossaryArgs {
    const SCHEMA: &'static str = r#"{
        "type": "object",
        "properties": {
            "sample": { "type": "integer" },
            "from_end": { "type": "boolean" }
        },
        "additionalProperties": false
    }"#;
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct BootstrapGlossaryArgs {
    #[serde(default = "default_sample")]
    pub(crate) sample: usize,
}

impl ToolArgs for BootstrapGlossaryArgs {
    const SCHEMA: &'static str = r#"{
        "type": "object",
        "properties": { "sample": { "type": "integer" } },
        "additionalProperties": false
    }"#;
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct UpdateChapterTranslationArgs {
    pub(crate) index: usize,
    #[serde(default)]
    pub(crate) translated_title: String,
    pub(crate) translated: String,
}

impl ToolArgs for UpdateChapterTranslationArgs {
    const SCHEMA: &'static str = r#"{
        "type": "object",
        "properties": {
            "index": { "type": "integer" },
            "translated_title": { "type": "string" },
            "translated": { "type": "string" }
        },
        "required": ["index", "translated"],
        "additionalProperties": false
    }"#;
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SetChapterPromptArgs {
    pub(crate) index: usize,
    pub(crate) prompt: String,
}

impl ToolArgs for SetChapterPromptArgs {
    const SCHEMA: &'static str = r#"{
        "type": "object",
        "properties": {
            "index": { "type": "integer" },
            "prompt": { "type": "string" }
        },
        "required": ["index", "prompt"],
        "additionalProperties": false
    }"#;
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SetChapterContextArgs {
    pub(crate) index: usize,
    pub(crate) summary: String,
    pub(crate) prev_tail: String,
}

impl ToolArgs for SetChapterContextArgs {
    const SCHEMA: &'static str = r#"{
        "type": "object",
        "properties": {
            "index": { "type": "integer" },
            "summary": { "type": "string" },
            "prev_tail": { "type": "string" }
        },
        "required": ["index", "summary", "prev_tail"],
        "additionalProperties": false
    }"#;
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ReplaceInBookArgs {
    pub(crate) find: String,
    pub(crate) replace: String,
    #[serde(default)]
    pub(crate) match_case: bool,
    #[serde(default)]
    pub(crate) whole_word: bool,
    #[serde(default)]
    pub(crate) regex: bool,
}

impl ToolArgs for ReplaceInBookArgs {
    const SCHEMA: &'static str = r#"{
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
    }"#;
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ExportBookArgs {
    pub(crate) format: String,
}

impl ToolArgs for ExportBookArgs {
    const SCHEMA: &'static str = r#"{
        "type": "object",
        "properties": {
            "format": { "type": "string", "enum": ["fb2", "epub", "pdf", "txt"] }
        },
        "required": ["format"],
        "additionalProperties": false
    }"#;
}

/// Dummy object covering every property the schema advertises.
#[cfg(test)]
pub(crate) fn sample_object(schema: &Value) -> Value {
    use serde_json::json;
    let Some(props) = schema.get("properties").and_then(|p| p.as_object()) else {
        return json!({});
    };
    let mut map = serde_json::Map::new();
    for (k, v) in props {
        map.insert(k.clone(), dummy(v));
    }
    Value::Object(map)
}

#[cfg(test)]
fn dummy(schema: &Value) -> Value {
    use serde_json::json;
    match schema.get("type").and_then(|t| t.as_str()) {
        Some("string") => json!("x"),
        Some("integer") | Some("number") => json!(1),
        Some("boolean") => json!(false),
        Some("array") => {
            let item = schema.get("items").cloned().unwrap_or_else(|| json!({}));
            json!([dummy(&item)])
        }
        Some("object") => sample_object(schema),
        _ => json!(null),
    }
}
