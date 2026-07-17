//! Application configuration: DeepSeek API access and translation parameters.
//!
//! Loaded from `config.local.toml` or environment variables.
//! Secrets (the API key) are not committed to the repo (see `.gitignore`).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// DeepSeek API key (env: DEEPSEEK_API_KEY).
    pub api_key: String,
    /// API base URL (OpenAI-compatible).
    pub base_url: String,
    /// Model: "deepseek-chat" or "deepseek-reasoner".
    pub model: String,

    /// Source language, e.g. "Chinese".
    pub source_lang: String,
    /// Target language, e.g. "Russian".
    pub target_lang: String,

    /// How many chapters to translate in parallel.
    pub concurrency: usize,
    /// Max chunk size in characters (fallback splitting of long chapters).
    pub max_chunk_chars: usize,
    /// Number of retries on network errors / 429.
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
            concurrency: 4,
            max_chunk_chars: 6000,
            max_retries: 5,
        }
    }
}

impl Config {
    /// TODO: load from file/env, key from env DEEPSEEK_API_KEY.
    pub fn load() -> anyhow::Result<Self> {
        todo!("load configuration from config.local.toml + env")
    }
}
