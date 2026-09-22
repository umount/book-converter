//! Application configuration: DeepSeek API access and translation parameters.
//!
//! The API key is entered on the Settings page and kept in the settings DB;
//! `DEEPSEEK_API_KEY` (or a local `.env`) is the fallback for a machine with no
//! UI. Neither is ever written to the repo (see `.gitignore`: `.env`,
//! `config.local.toml`).

use serde::{Deserialize, Serialize};

/// Settings-DB key holding the DeepSeek API key.
pub const API_KEY_SETTING: &str = "deepseek_api_key";

#[derive(Clone, Serialize, Deserialize)]
pub struct Config {
    /// DeepSeek API key. Stored in the settings DB (entered in the UI), or taken
    /// from `DEEPSEEK_API_KEY` when no setting is present. Never logged.
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
    /// Per-request timeout in seconds. Long chapters can take several minutes
    /// even when the output is well under the token cap.
    pub request_timeout_secs: u64,

    /// Max chunk size in characters (fallback splitting of long chapters).
    pub max_chunk_chars: usize,
    /// Max tokens the model may generate per reply (DeepSeek V4: up to 384K).
    pub max_output_tokens: u32,
    /// Number of retries on network errors / 429 / 5xx.
    pub max_retries: usize,
}

/// Redacts the API key, so no accidental `{config:?}` can print it.
impl std::fmt::Debug for Config {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Config")
            .field("api_key", &if self.has_key() { "<set>" } else { "<unset>" })
            .field("base_url", &self.base_url)
            .field("model", &self.model)
            .field("source_lang", &self.source_lang)
            .field("target_lang", &self.target_lang)
            .field("temperature", &self.temperature)
            .field("request_timeout_secs", &self.request_timeout_secs)
            .field("max_chunk_chars", &self.max_chunk_chars)
            .field("max_output_tokens", &self.max_output_tokens)
            .field("max_retries", &self.max_retries)
            .finish()
    }
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
            request_timeout_secs: 600,
            max_chunk_chars: 10000,
            max_output_tokens: 384_000,
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
    /// `DEEPSEEK_API_KEY` (used when no key is saved in settings), plus optional overrides:
    /// `DEEPSEEK_MODEL`, `DEEPSEEK_BASE_URL`.
    pub fn load() -> Self {
        // Ignore "not found" — running without a .env (env vars only) is valid.
        let _ = dotenvy::dotenv();

        let mut cfg = Config::default();

        // The translation language pair is a user setting (chosen in the UI,
        // persisted in the settings DB), so the tool is not tied to one pair.
        let sdb = crate::settings::db_path();
        if let Ok(Some(v)) = crate::settings::get(&sdb, "source_lang") {
            if !v.trim().is_empty() {
                cfg.source_lang = v;
            }
        }
        if let Ok(Some(v)) = crate::settings::get(&sdb, "target_lang") {
            if !v.trim().is_empty() {
                cfg.target_lang = v;
            }
        }

        // Advanced generation settings, also chosen in the UI (Settings page).
        if let Ok(Some(v)) = crate::settings::get(&sdb, "model") {
            if !v.trim().is_empty() {
                cfg.model = v;
            }
        }
        if let Ok(Some(v)) = crate::settings::get(&sdb, "base_url") {
            if !v.trim().is_empty() {
                cfg.base_url = v;
            }
        }
        if let Ok(Some(v)) = crate::settings::get(&sdb, "max_chunk_chars") {
            if let Ok(n) = v.trim().parse::<usize>() {
                if n > 0 {
                    cfg.max_chunk_chars = n;
                }
            }
        }
        if let Ok(Some(v)) = crate::settings::get(&sdb, "max_retries") {
            if let Ok(n) = v.trim().parse::<usize>() {
                cfg.max_retries = n;
            }
        }
        if let Ok(Some(v)) = crate::settings::get(&sdb, "temperature") {
            if let Ok(f) = v.trim().parse::<f32>() {
                if (0.0..=2.0).contains(&f) {
                    cfg.temperature = f;
                }
            }
        }

        // The API key is the one setting the UI owns outright: it is entered on
        // the Settings page and kept in the settings DB. The environment
        // variable is the fallback for a machine with no UI (CI, a headless
        // run), which is the reverse of every other key below, where the
        // environment wins so an operator can pin a value.
        cfg.api_key = match crate::settings::get(&sdb, API_KEY_SETTING) {
            Ok(Some(v)) if !v.trim().is_empty() => v.trim().to_string(),
            _ => std::env::var("DEEPSEEK_API_KEY").unwrap_or_default(),
        };

        // Environment variables (for power users / CI) take precedence.
        if let Ok(model) = std::env::var("DEEPSEEK_MODEL") {
            cfg.model = model;
        }
        if let Ok(base) = std::env::var("DEEPSEEK_BASE_URL") {
            cfg.base_url = base;
        }
        if let Ok(v) = std::env::var("SOURCE_LANG") {
            cfg.source_lang = v;
        }
        if let Ok(v) = std::env::var("TARGET_LANG") {
            cfg.target_lang = v;
        }
        cfg
    }

    /// Overlay a project's stored language pair. Empty / missing values keep
    /// the global setting, so older projects without meta keys still work.
    pub fn with_langs(mut self, source: Option<&str>, target: Option<&str>) -> Self {
        if let Some(v) = source.map(str::trim).filter(|v| !v.is_empty()) {
            self.source_lang = v.to_string();
        }
        if let Some(v) = target.map(str::trim).filter(|v| !v.is_empty()) {
            self.target_lang = v.to_string();
        }
        self
    }

    /// Global config with this project's `source_lang` / `target_lang` overlaid.
    pub fn load_for(store: &crate::state::Store) -> Self {
        let source = store.get_meta("source_lang").ok().flatten();
        let target = store.get_meta("target_lang").ok().flatten();
        Self::load().with_langs(source.as_deref(), target.as_deref())
    }

    /// True when an API key is present.
    pub fn has_key(&self) -> bool {
        !self.api_key.trim().is_empty()
    }

    /// Whether the effective key came from the environment rather than settings.
    pub fn key_from_env() -> bool {
        let stored = crate::settings::get(&crate::settings::db_path(), API_KEY_SETTING)
            .ok()
            .flatten()
            .filter(|v| !v.trim().is_empty());
        stored.is_none() && std::env::var("DEEPSEEK_API_KEY").is_ok_and(|v| !v.trim().is_empty())
    }

    /// A key shown to a human without revealing it: enough to tell two keys
    /// apart, not enough to use one.
    pub fn key_hint(&self) -> Option<String> {
        let key = self.api_key.trim();
        if key.is_empty() {
            return None;
        }
        let chars: Vec<char> = key.chars().collect();
        if chars.len() <= 12 {
            return Some("•".repeat(chars.len().max(4)));
        }
        let head: String = chars.iter().take(5).collect();
        let tail: String = chars.iter().skip(chars.len() - 4).collect();
        Some(format!("{head}…{tail}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The hint has to identify a key without being usable as one.
    #[test]
    fn key_hint_masks_the_key() {
        let cfg = Config {
            api_key: "sk-f656052576dd45408b235cd387c332d3".into(),
            ..Config::default()
        };
        let hint = cfg.key_hint().unwrap();
        assert_eq!(hint, "sk-f6\u{2026}32d3");
        assert!(!cfg.api_key.contains(&hint));

        let short = Config {
            api_key: "abc".into(),
            ..Config::default()
        };
        assert_eq!(
            short.key_hint().unwrap(),
            "\u{2022}\u{2022}\u{2022}\u{2022}"
        );

        assert!(Config {
            api_key: "   ".into(),
            ..Config::default()
        }
        .key_hint()
        .is_none());
    }

    /// A stray `{config:?}` must not print the key.
    #[test]
    fn debug_redacts_the_key() {
        let cfg = Config {
            api_key: "sk-secret-value-here".into(),
            ..Config::default()
        };
        let printed = format!("{cfg:?}");
        assert!(!printed.contains("secret"), "{printed}");
        assert!(printed.contains("<set>"));
        assert!(format!("{:?}", Config::default()).contains("<unset>"));
    }

    #[test]
    fn with_langs_overlays_nonempty_only() {
        let cfg = Config::default().with_langs(Some("Japanese"), Some("German"));
        assert_eq!(cfg.source_lang, "Japanese");
        assert_eq!(cfg.target_lang, "German");

        let kept = Config::default().with_langs(Some("  "), None);
        assert_eq!(kept.source_lang, "Chinese");
        assert_eq!(kept.target_lang, "Russian");
    }

    #[test]
    fn load_for_overlays_project_langs() {
        let store = crate::state::Store::open(":memory:").unwrap();
        store.set_translation_langs("Korean", "French").unwrap();
        let cfg = Config::load_for(&store);
        assert_eq!(cfg.source_lang, "Korean");
        assert_eq!(cfg.target_lang, "French");
    }
}
