//! App-wide settings commands.

fn err(error: impl std::fmt::Display) -> String {
    error.to_string()
}

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
    pub target_lang: String,
    pub base_url: String,
    pub max_retries: usize,
    pub temperature: f32,
    pub request_timeout_secs: u64,
    pub max_output_tokens: u32,
    pub has_key: bool,
    /// Enough of the key to recognise it, never enough to use it.
    pub key_hint: Option<String>,
    /// True when the key in effect comes from `DEEPSEEK_API_KEY` because no key
    /// is stored in settings. Saving one in the UI takes over.
    pub key_from_env: bool,
    /// Setting keys currently pinned by an environment variable (UI value ignored).
    pub env_locked: Vec<String>,
}

#[tauri::command]
pub async fn get_effective_config() -> Result<EffectiveConfig, String> {
    let cfg = crate::config::Config::load();
    let has_key = cfg.has_key();
    let key_hint = cfg.key_hint();
    let key_from_env = crate::config::Config::key_from_env();
    let mut env_locked = Vec::new();
    for (key, var) in [
        ("target_lang", "TARGET_LANG"),
        ("model", "DEEPSEEK_MODEL"),
        ("base_url", "DEEPSEEK_BASE_URL"),
    ] {
        if std::env::var(var).is_ok() {
            env_locked.push(key.to_string());
        }
    }
    Ok(EffectiveConfig {
        target_lang: std::env::var("TARGET_LANG").ok()
            .filter(|value| !value.trim().is_empty())
            .or(crate::settings::get(&settings_db(), "target_lang").map_err(err)?
                .filter(|value| !value.trim().is_empty()))
            .unwrap_or_else(|| "ru".into()),
        model: cfg.model,
        base_url: cfg.base_url,
        max_retries: cfg.max_retries,
        temperature: cfg.temperature,
        request_timeout_secs: cfg.request_timeout_secs,
        max_output_tokens: cfg.max_output_tokens,
        has_key,
        key_hint,
        key_from_env,
        env_locked,
    })
}

/// Build provenance shown in the About dialog: which version, from which
/// sources, on which platform.
#[derive(serde::Serialize)]
pub struct AppInfo {
    /// Full product name for display.
    pub name: String,
    pub version: String,
    /// Short git hash the binary was built from ("unknown" outside a checkout).
    pub commit: String,
    pub commit_date: String,
    pub tauri: String,
    pub os: String,
    pub arch: String,
}

/// Name / version / build stamp for the About dialog.
#[tauri::command]
pub async fn get_app_info() -> Result<AppInfo, String> {
    Ok(AppInfo {
        name: crate::APP_NAME.to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        commit: env!("BC_COMMIT").to_string(),
        commit_date: env!("BC_COMMIT_DATE").to_string(),
        tauri: tauri::VERSION.to_string(),
        os: std::env::consts::OS.to_string(),
        arch: std::env::consts::ARCH.to_string(),
    })
}

/// Read a persisted app setting (e.g. the UI language).
///
/// The API key is deliberately not readable here: it goes in through
/// [`set_api_key`] and comes back only as the masked hint in
/// [`EffectiveConfig`], so a stored secret never crosses the IPC boundary in
/// full once it has been saved.
#[tauri::command]
pub async fn get_setting(key: String) -> Result<Option<String>, String> {
    if key == crate::config::API_KEY_SETTING || key.starts_with("ai_credential:") {
        return Ok(None);
    }
    crate::settings::get(&settings_db(), &key).map_err(err)
}

/// Persist an app setting. Secrets go through [`set_api_key`] instead.
#[tauri::command]
pub async fn set_setting(key: String, value: String) -> Result<(), String> {
    if key == crate::config::API_KEY_SETTING
        || key.starts_with("ai_credential:")
        || key.starts_with("ai_profile:")
        || key.starts_with("ai_profile_revision:")
        || key.starts_with("ai_profile_name:")
    {
        return Err("use_set_api_key".into());
    }
    crate::settings::set(&settings_db(), &key, &value).map_err(err)
}

/// Store the DeepSeek API key, or clear it when given an empty string.
///
/// Clearing falls back to `DEEPSEEK_API_KEY`, if that is set, rather than
/// leaving the app with no key at all.
#[tauri::command]
pub async fn set_api_key(key: String) -> Result<(), String> {
    let key = key.trim();
    let db = settings_db();
    if key.is_empty() {
        crate::settings::remove(&db, crate::config::API_KEY_SETTING).map_err(err)?;
        tracing::info!("API key cleared from settings");
    } else {
        crate::settings::set(&db, crate::config::API_KEY_SETTING, key).map_err(err)?;
        tracing::info!("API key saved to settings");
    }
    Ok(())
}
