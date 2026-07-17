//! DeepSeek API client (OpenAI-compatible `/chat/completions`).
//!
//! Handles a single translation request: retry with exponential backoff,
//! 429/5xx handling, timeouts. Concurrency and queueing live in `state`/UI.

use crate::config::Config;

pub struct DeepSeekClient {
    http: reqwest::Client,
    config: Config,
}

impl DeepSeekClient {
    pub fn new(config: Config) -> Self {
        Self {
            http: reqwest::Client::new(),
            config,
        }
    }

    /// Translate a single ready prompt (system + user) and return the text.
    ///
    /// TODO:
    /// - POST {base_url}/chat/completions with model config.model;
    /// - header Authorization: Bearer {api_key};
    /// - retry with backoff on 429/5xx/network errors (up to max_retries);
    /// - return the content of the first choice.
    pub async fn translate(&self, _system: &str, _user: &str) -> anyhow::Result<String> {
        let _ = (&self.http, &self.config);
        todo!("DeepSeek /chat/completions request with retry")
    }
}
