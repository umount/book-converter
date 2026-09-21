//! DeepSeek API client (OpenAI-compatible `/chat/completions`).
//!
//! Handles a single translation request: retry with exponential backoff on
//! 429 / 5xx / network errors, and timeouts. If the model hits the output token
//! limit (`finish_reason = length`), the client continues the completion so long
//! chapters are not silently truncated. Concurrency and queueing live in the
//! orchestration layer, not here.
//!
//! Also supports OpenAI-style tool/function calling for the project assistant.

use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::config::Config;

pub struct DeepSeekClient {
    http: reqwest::Client,
    config: Config,
}

// --- Request/response payloads (only the fields we use) ---

#[derive(Serialize)]
struct ChatRequest<'a> {
    model: &'a str,
    messages: Vec<ChatMessage>,
    temperature: f32,
    /// Cap output tokens at the API maximum so long chapters are not truncated.
    /// DeepSeek V4 allows up to 384K.
    max_tokens: u32,
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    response_format: Option<ResponseFormat>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<&'a [ToolSpec]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_choice: Option<&'a str>,
}

/// OpenAI-compatible structured-output selector. `json_object` makes the model
/// return one parseable JSON document, which is only safe for replies that
/// cannot hit the output limit: a truncated JSON document is unrecoverable,
/// whereas truncated prose can be continued.
#[derive(Serialize, Clone, Copy)]
struct ResponseFormat {
    #[serde(rename = "type")]
    kind: &'static str,
}

impl ResponseFormat {
    const JSON: Self = ResponseFormat { kind: "json_object" };
}

/// One chat message for the API (prose, tool calls, or tool results).
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ChatMessage {
    pub role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

impl ChatMessage {
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: "system".into(),
            content: Some(content.into()),
            tool_calls: None,
            tool_call_id: None,
            name: None,
        }
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: "user".into(),
            content: Some(content.into()),
            tool_calls: None,
            tool_call_id: None,
            name: None,
        }
    }

    pub fn assistant_text(content: impl Into<String>) -> Self {
        Self {
            role: "assistant".into(),
            content: Some(content.into()),
            tool_calls: None,
            tool_call_id: None,
            name: None,
        }
    }

    pub fn assistant_tools(tool_calls: Vec<ToolCall>) -> Self {
        Self::assistant_turn(None, tool_calls)
    }

    pub fn assistant_turn(content: Option<String>, tool_calls: Vec<ToolCall>) -> Self {
        Self {
            role: "assistant".into(),
            content: content.filter(|s| !s.is_empty()),
            tool_calls: Some(tool_calls),
            tool_call_id: None,
            name: None,
        }
    }

    pub fn tool_result(tool_call_id: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: "tool".into(),
            content: Some(content.into()),
            tool_calls: None,
            tool_call_id: Some(tool_call_id.into()),
            name: None,
        }
    }
}

/// OpenAI-compatible tool definition.
#[derive(Serialize, Clone, Debug)]
pub struct ToolSpec {
    #[serde(rename = "type")]
    pub kind: &'static str,
    pub function: ToolFunction,
}

#[derive(Serialize, Clone, Debug)]
pub struct ToolFunction {
    pub name: String,
    pub description: String,
    pub parameters: Value,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ToolCall {
    pub id: String,
    #[serde(rename = "type", default = "default_tool_type")]
    pub kind: String,
    pub function: ToolCallFunction,
}

fn default_tool_type() -> String {
    "function".into()
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ToolCallFunction {
    pub name: String,
    pub arguments: String,
}

#[derive(Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
}

#[derive(Deserialize)]
struct Choice {
    message: ResponseMessage,
    #[serde(default)]
    finish_reason: Option<String>,
}

#[derive(Deserialize)]
struct ResponseMessage {
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    tool_calls: Option<Vec<ToolCall>>,
}

/// Outcome of a tools-capable chat turn.
#[derive(Debug, Clone)]
pub struct ToolsCompletion {
    pub content: Option<String>,
    pub tool_calls: Vec<ToolCall>,
    #[allow(dead_code)]
    pub finish_reason: Option<String>,
}

struct Completion {
    content: String,
    /// `stop` | `length` | `content_filter` | …
    finish_reason: Option<String>,
}

/// Internal error carrying whether the failure is worth retrying.
struct ApiError {
    retryable: bool,
    error: anyhow::Error,
}

/// How many times to continue after an output-length truncation.
const MAX_CONTINUATIONS: usize = 4;

impl DeepSeekClient {
    pub fn new(config: Config) -> Result<Self> {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(config.request_timeout_secs))
            .build()
            .context("building HTTP client")?;
        Ok(Self { http, config })
    }

    /// Translate a single prompt (system + user) and return the model output.
    ///
    /// Retries transient failures with exponential backoff up to
    /// `config.max_retries`. If a successful reply was cut off by the output
    /// token limit, continues the generation until `finish_reason` is not
    /// `length` (or [`MAX_CONTINUATIONS`] is exhausted).
    pub async fn translate(&self, system: &str, user: &str) -> Result<String> {
        if !self.config.has_key() {
            return Err(anyhow!("DEEPSEEK_API_KEY is not set"));
        }

        let mut messages = vec![ChatMessage::system(system), ChatMessage::user(user)];

        let mut first = self.chat_with_retries(&messages, None, None).await?;
        let mut full = first.content;

        let mut cont = 0;
        while first.finish_reason.as_deref() == Some("length") && cont < MAX_CONTINUATIONS {
            tracing::warn!(
                continuation = cont + 1,
                so_far_chars = full.chars().count(),
                "DeepSeek output truncated (finish_reason=length); continuing"
            );
            messages.push(ChatMessage::assistant_text(full.clone()));
            messages.push(ChatMessage::user(continuation_prompt()));
            first = self.chat_with_retries(&messages, None, None).await?;
            // Drop the continuation instruction before the next loop turn; keep
            // the growing assistant text as a single assistant message.
            messages.pop(); // continuation user
            messages.pop(); // previous assistant snapshot
            if !first.content.is_empty() {
                if !full.ends_with('\n') && !first.content.starts_with('\n') {
                    full.push('\n');
                }
                full.push_str(&first.content);
            }
            cont += 1;
        }

        if first.finish_reason.as_deref() == Some("length") {
            tracing::warn!(
                chars = full.chars().count(),
                "translation still truncated after {MAX_CONTINUATIONS} continuations"
            );
        }

        Ok(full)
    }

    /// Ask for one JSON document (`response_format: json_object`) and return it
    /// raw, for the caller to deserialize.
    ///
    /// Unlike [`Self::translate`] this does **not** continue a truncated reply:
    /// half a JSON document cannot be parsed and two halves cannot be rejoined.
    /// Use it only for bounded replies (term extraction, line repair), and keep
    /// prose on `translate`. A truncation here is logged and surfaced as an
    /// error rather than silently returning an unparseable fragment.
    pub async fn translate_json(&self, system: &str, user: &str) -> Result<String> {
        if !self.config.has_key() {
            return Err(anyhow!("DEEPSEEK_API_KEY is not set"));
        }
        let messages = vec![ChatMessage::system(system), ChatMessage::user(user)];
        let completion = self
            .chat_with_retries(&messages, Some(ResponseFormat::JSON), None)
            .await?;
        if completion.finish_reason.as_deref() == Some("length") {
            return Err(anyhow!(
                "JSON reply hit the output token limit and cannot be parsed; \
                 ask for a smaller batch"
            ));
        }
        Ok(completion.content)
    }

    /// One chat turn that may return tool calls (OpenAI-compatible function calling).
    pub async fn chat_tools(
        &self,
        messages: &[ChatMessage],
        tools: &[ToolSpec],
    ) -> Result<ToolsCompletion> {
        if !self.config.has_key() {
            return Err(anyhow!("DEEPSEEK_API_KEY is not set"));
        }
        let mut attempt = 0;
        loop {
            match self.try_once_tools(messages, tools).await {
                Ok(c) => return Ok(c),
                Err(ApiError { retryable, error }) => {
                    if !retryable || attempt >= self.config.max_retries {
                        return Err(error);
                    }
                    let delay = backoff_delay(attempt);
                    tracing::warn!(
                        attempt,
                        ?delay,
                        "DeepSeek tools request failed, retrying: {error:#}"
                    );
                    tokio::time::sleep(delay).await;
                    attempt += 1;
                }
            }
        }
    }

    async fn chat_with_retries(
        &self,
        messages: &[ChatMessage],
        response_format: Option<ResponseFormat>,
        tools: Option<&[ToolSpec]>,
    ) -> Result<Completion> {
        let mut attempt = 0;
        loop {
            match self.try_once(messages, response_format, tools).await {
                Ok(c) => return Ok(c),
                Err(ApiError { retryable, error }) => {
                    if !retryable || attempt >= self.config.max_retries {
                        return Err(error);
                    }
                    let delay = backoff_delay(attempt);
                    tracing::warn!(
                        attempt,
                        ?delay,
                        "DeepSeek request failed, retrying: {error:#}"
                    );
                    tokio::time::sleep(delay).await;
                    attempt += 1;
                }
            }
        }
    }

    /// One attempt. Classifies the outcome as retryable or fatal.
    async fn try_once(
        &self,
        messages: &[ChatMessage],
        response_format: Option<ResponseFormat>,
        tools: Option<&[ToolSpec]>,
    ) -> std::result::Result<Completion, ApiError> {
        let parsed = self
            .post_chat(messages, response_format, tools, None)
            .await?;
        let choice = parsed.choices.into_iter().next().ok_or_else(|| ApiError {
            retryable: false,
            error: anyhow!("DeepSeek response had no choices"),
        })?;

        Ok(Completion {
            content: choice.message.content.unwrap_or_default(),
            finish_reason: choice.finish_reason,
        })
    }

    async fn try_once_tools(
        &self,
        messages: &[ChatMessage],
        tools: &[ToolSpec],
    ) -> std::result::Result<ToolsCompletion, ApiError> {
        let parsed = self
            .post_chat(messages, None, Some(tools), Some("auto"))
            .await?;
        let choice = parsed.choices.into_iter().next().ok_or_else(|| ApiError {
            retryable: false,
            error: anyhow!("DeepSeek response had no choices"),
        })?;
        Ok(ToolsCompletion {
            content: choice
                .message
                .content
                .filter(|s| !s.trim().is_empty()),
            tool_calls: choice.message.tool_calls.unwrap_or_default(),
            finish_reason: choice.finish_reason,
        })
    }

    async fn post_chat(
        &self,
        messages: &[ChatMessage],
        response_format: Option<ResponseFormat>,
        tools: Option<&[ToolSpec]>,
        tool_choice: Option<&str>,
    ) -> std::result::Result<ChatResponse, ApiError> {
        let url = format!(
            "{}/chat/completions",
            self.config.base_url.trim_end_matches('/')
        );
        let body = ChatRequest {
            model: &self.config.model,
            messages: messages.to_vec(),
            temperature: self.config.temperature,
            max_tokens: self.config.max_output_tokens,
            stream: false,
            response_format,
            tools,
            tool_choice,
        };

        let resp = self
            .http
            .post(&url)
            .bearer_auth(&self.config.api_key)
            .json(&body)
            .send()
            .await
            .map_err(|e| ApiError {
                retryable: true,
                error: anyhow::Error::new(e).context("sending request to DeepSeek"),
            })?;

        let status = resp.status();
        if !status.is_success() {
            let retryable = status.as_u16() == 429 || status.is_server_error();
            let text = resp.text().await.unwrap_or_default();
            return Err(ApiError {
                retryable,
                error: anyhow!("DeepSeek returned {status}: {text}"),
            });
        }

        resp.json().await.map_err(|e| ApiError {
            retryable: false,
            error: anyhow::Error::new(e).context("decoding DeepSeek response"),
        })
    }
}

fn continuation_prompt() -> String {
    "Your previous reply was cut off because of the output length limit. \
     Continue the translation EXACTLY from where you stopped. \
     Do not repeat any text already produced. \
     Do not add explanations — output only the continuation of the translation."
        .into()
}

/// Exponential backoff: ~1s, 2s, 4s, 8s … capped at 30s.
fn backoff_delay(attempt: usize) -> Duration {
    let secs = 1u64 << attempt.min(5);
    Duration::from_secs(secs.min(30))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn continuation_prompt_asks_not_to_repeat() {
        let p = continuation_prompt();
        assert!(p.contains("Continue"));
        assert!(p.contains("Do not repeat"));
    }

    #[test]
    fn parses_tool_calls_payload() {
        let raw = r#"{
          "choices": [{
            "finish_reason": "tool_calls",
            "message": {
              "role": "assistant",
              "content": null,
              "tool_calls": [{
                "id": "call_1",
                "type": "function",
                "function": { "name": "get_progress", "arguments": "{}" }
              }]
            }
          }]
        }"#;
        let parsed: ChatResponse = serde_json::from_str(raw).unwrap();
        let calls = parsed.choices[0].message.tool_calls.as_ref().unwrap();
        assert_eq!(calls[0].function.name, "get_progress");
        assert_eq!(calls[0].id, "call_1");
    }

    #[test]
    fn chat_message_tool_result_serializes() {
        let m = ChatMessage::tool_result("call_1", r#"{"ok":true}"#);
        let v = serde_json::to_value(&m).unwrap();
        assert_eq!(v["role"], "tool");
        assert_eq!(v["tool_call_id"], "call_1");
        assert!(v.get("tool_calls").is_none());
    }

    #[test]
    fn assistant_tools_omits_empty_content() {
        let call = ToolCall {
            id: "c1".into(),
            kind: "function".into(),
            function: ToolCallFunction {
                name: "get_progress".into(),
                arguments: "{}".into(),
            },
        };
        let m = ChatMessage::assistant_tools(vec![call]);
        let v = serde_json::to_value(&m).unwrap();
        assert_eq!(v["role"], "assistant");
        assert!(v.get("content").is_none());
        assert_eq!(v["tool_calls"][0]["id"], "c1");
    }
}
