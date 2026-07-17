//! DeepSeek API client (OpenAI-compatible `/chat/completions`).
//!
//! Handles a single translation request: retry with exponential backoff on
//! 429 / 5xx / network errors, and timeouts. Concurrency and queueing live in
//! the orchestration layer (`state`/UI), not here.

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
    messages: Vec<Message<'a>>,
    temperature: f32,
    stream: bool,
}

#[derive(Serialize)]
struct Message<'a> {
    role: &'a str,
    content: &'a str,
}

#[derive(Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
}

#[derive(Deserialize)]
struct Choice {
    message: ResponseMessage,
}

#[derive(Deserialize)]
struct ResponseMessage {
    content: String,
}

/// Internal error carrying whether the failure is worth retrying.
struct ApiError {
    retryable: bool,
    error: anyhow::Error,
}

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
    /// `config.max_retries`.
    pub async fn translate(&self, system: &str, user: &str) -> Result<String> {
        if !self.config.has_key() {
            return Err(anyhow!("DEEPSEEK_API_KEY is not set"));
        }

        let mut attempt = 0;
        loop {
            match self.try_once(system, user).await {
                Ok(text) => return Ok(text),
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
    async fn try_once(&self, system: &str, user: &str) -> std::result::Result<String, ApiError> {
        let url = format!("{}/chat/completions", self.config.base_url.trim_end_matches('/'));
        let body = ChatRequest {
            model: &self.config.model,
            messages: vec![
                Message { role: "system", content: system },
                Message { role: "user", content: user },
            ],
            temperature: self.config.temperature,
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
                // Network/timeout errors are worth retrying.
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

        parsed
            .choices
            .into_iter()
            .next()
            .map(|c| c.message.content)
            .ok_or_else(|| ApiError {
                retryable: false,
                error: anyhow!("DeepSeek response had no choices"),
            })
    }
}

/// Exponential backoff: ~1s, 2s, 4s, 8s … capped at 30s.
fn backoff_delay(attempt: usize) -> Duration {
    let secs = 1u64 << attempt.min(5); // 1,2,4,8,16,32 → capped below
    Duration::from_secs(secs.min(30))
}
