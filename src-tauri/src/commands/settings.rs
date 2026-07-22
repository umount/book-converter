//! App-wide settings commands.

use crate::dto::err;

/// Global settings DB (app-wide, survives restarts).
fn settings_db() -> std::path::PathBuf {
    crate::settings::db_path()
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
