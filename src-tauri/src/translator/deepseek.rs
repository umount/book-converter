//! DeepSeek API client (OpenAI-compatible `/chat/completions`).
//!
//! Handles a single translation request: retry with exponential backoff on
//! 429 / 5xx / network errors, and timeouts. If the model hits the output token
//! limit (`finish_reason = length`), the client continues the completion so long
//! chapters are not silently truncated. Concurrency and queueing live in the
//! orchestration layer, not here.

use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};

use crate::config::Config;

pub struct DeepSeekClient {
    http: reqwest::Client,
    config: Config,
}

// --- Request/response payloads (only the fields we use) ---

#[derive(Serialize)]
struct ChatRequest<'a> {
    model: &'a str,
    messages: Vec<MessageOwned>,
    temperature: f32,
    /// Cap output tokens at the API maximum so long chapters are not truncated.
    /// DeepSeek V4 allows up to 384K.
    max_tokens: u32,
    stream: bool,
}

#[derive(Serialize, Clone)]
struct MessageOwned {
    role: String,
    content: String,
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
    content: String,
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

        let mut messages = vec![
            MessageOwned {
                role: "system".into(),
                content: system.to_string(),
            },
            MessageOwned {
                role: "user".into(),
                content: user.to_string(),
            },
        ];

        let mut first = self.chat_with_retries(&messages).await?;
        let mut full = first.content;

        let mut cont = 0;
        while first.finish_reason.as_deref() == Some("length") && cont < MAX_CONTINUATIONS {
            tracing::warn!(
                continuation = cont + 1,
                so_far_chars = full.chars().count(),
                "DeepSeek output truncated (finish_reason=length); continuing"
            );
            messages.push(MessageOwned {
                role: "assistant".into(),
                content: full.clone(),
            });
            messages.push(MessageOwned {
                role: "user".into(),
                content: continuation_prompt(),
            });
            first = self.chat_with_retries(&messages).await?;
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

    async fn chat_with_retries(&self, messages: &[MessageOwned]) -> Result<Completion> {
        let mut attempt = 0;
        loop {
            match self.try_once(messages).await {
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
    async fn try_once(&self, messages: &[MessageOwned]) -> std::result::Result<Completion, ApiError> {
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

        let parsed: ChatResponse = resp.json().await.map_err(|e| ApiError {
            retryable: false,
            error: anyhow::Error::new(e).context("decoding DeepSeek response"),
        })?;

        let choice = parsed.choices.into_iter().next().ok_or_else(|| ApiError {
            retryable: false,
            error: anyhow!("DeepSeek response had no choices"),
        })?;

        Ok(Completion {
            content: choice.message.content,
            finish_reason: choice.finish_reason,
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
}
