//! Application configuration: DeepSeek API access and translation parameters.
//!
//! The API key is read from the `DEEPSEEK_API_KEY` environment variable and is
//! never written to the repo (see `.gitignore`: `.env`, `config.local.toml`).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// DeepSeek API key (env: DEEPSEEK_API_KEY). Never logged or persisted.
    pub api_key: String,
    /// API base URL (OpenAI-compatible).
    pub base_url: String,
    /// Model: "deepseek-chat" or "deepseek-reasoner".
    pub model: String,

    /// Source language, e.g. "Chinese".
    pub source_lang: String,
    /// Target language, e.g. "Russian".
    pub target_lang: String,

    /// Sampling temperature. Kept low for faithful translation: high values
    /// (DeepSeek's nominal 1.3) make long chapter outputs degenerate into
    /// gibberish near the end.
    pub temperature: f32,
    /// Per-request timeout in seconds.
    pub request_timeout_secs: u64,

    /// How many chapters to translate in parallel.
    pub concurrency: usize,
    /// Max chunk size in characters (fallback splitting of long chapters).
    pub max_chunk_chars: usize,
    /// Number of retries on network errors / 429 / 5xx.
    pub max_retries: usize,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            api_key: String::new(),
            base_url: "https://api.deepseek.com".into(),
            model: "deepseek-chat".into(),
            source_lang: "Chinese".into(),
            target_lang: "Russian".into(),
            temperature: 0.3,
            request_timeout_secs: 120,
            concurrency: 4,
            max_chunk_chars: 6000,
            max_retries: 5,
        }
    }
}

impl Config {
    /// Build config from defaults, overlaying environment variables.
    ///
    /// A local `.env` (walked up from the working dir) is loaded first, so the
    /// key is available at startup without exporting it manually. Real
    /// environment variables take precedence over `.env`.
    ///
    /// `DEEPSEEK_API_KEY` (required for real requests), plus optional overrides:
    /// `DEEPSEEK_MODEL`, `DEEPSEEK_BASE_URL`.
    pub fn load() -> Self {
        // Ignore "not found" — running without a .env (env vars only) is valid.
        let _ = dotenvy::dotenv();

        let mut cfg = Config::default();
        if let Ok(key) = std::env::var("DEEPSEEK_API_KEY") {
            cfg.api_key = key;
        }
        if let Ok(model) = std::env::var("DEEPSEEK_MODEL") {
            cfg.model = model;
        }
        if let Ok(base) = std::env::var("DEEPSEEK_BASE_URL") {
            cfg.base_url = base;
        }
        cfg
    }

    /// True when an API key is present.
    pub fn has_key(&self) -> bool {
        !self.api_key.trim().is_empty()
    }
}
