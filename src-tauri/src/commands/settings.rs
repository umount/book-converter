//! App-wide settings commands.

use crate::dto::err;

/// Global settings DB (app-wide, survives restarts).
fn settings_db() -> std::path::PathBuf {
    crate::settings::db_path()
}

/// Effective (non-secret) configuration for display in the Settings page: the
/// resolved values after defaults + settings DB + environment, which keys an
/// environment variable is overriding, and whether an API key is present. The
/// API key itself is never returned.
#[derive(serde::Serialize)]
pub struct EffectiveConfig {
    pub model: String,
    pub base_url: String,
    pub source_lang: String,
    pub target_lang: String,
    pub max_chunk_chars: usize,
    pub max_retries: usize,
    pub temperature: f32,
    pub request_timeout_secs: u64,
    pub max_output_tokens: u32,
    pub has_key: bool,
    /// Setting keys currently pinned by an environment variable (UI value ignored).
    pub env_locked: Vec<String>,
}

#[tauri::command]
pub async fn get_effective_config() -> Result<EffectiveConfig, String> {
    let cfg = crate::config::Config::load();
    let has_key = cfg.has_key();
    let mut env_locked = Vec::new();
    for (key, var) in [
        ("model", "DEEPSEEK_MODEL"),
        ("base_url", "DEEPSEEK_BASE_URL"),
        ("source_lang", "SOURCE_LANG"),
        ("target_lang", "TARGET_LANG"),
    ] {
        if std::env::var(var).is_ok() {
            env_locked.push(key.to_string());
        }
    }
    Ok(EffectiveConfig {
        model: cfg.model,
        base_url: cfg.base_url,
        source_lang: cfg.source_lang,
        target_lang: cfg.target_lang,
        max_chunk_chars: cfg.max_chunk_chars,
        max_retries: cfg.max_retries,
        temperature: cfg.temperature,
        request_timeout_secs: cfg.request_timeout_secs,
        max_output_tokens: cfg.max_output_tokens,
        has_key,
        env_locked,
    })
}

/// Read a persisted app setting (e.g. the UI language).
#[tauri::command]
pub async fn get_setting(key: String) -> Result<Option<String>, String> {
    crate::settings::get(&settings_db(), &key).map_err(err)
}

/// Persist an app setting.
#[tauri::command]
pub async fn set_setting(key: String, value: String) -> Result<(), String> {
    crate::settings::set(&settings_db(), &key, &value).map_err(err)
}
