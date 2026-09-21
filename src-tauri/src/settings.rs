//! Global application settings (e.g. the UI language), persisted in a small
//! key-value SQLite DB in the app data directory so they survive restarts. This
//! is app-wide, unlike the per-book progress DBs.

use std::path::{Path, PathBuf};

use anyhow::Result;
use rusqlite::{params, Connection, OptionalExtension};

use crate::paths;

/// Path to the app-wide settings DB (`<app_data>/settings.db`).
pub fn db_path() -> PathBuf {
    paths::app_data_dir().join("settings.db")
}

fn open(db: &Path) -> Result<Connection> {
    if let Some(dir) = db.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let conn = Connection::open(db)?;
    conn.execute(
        "CREATE TABLE IF NOT EXISTS settings (key TEXT PRIMARY KEY, value TEXT NOT NULL)",
        [],
    )?;
    Ok(conn)
}

/// Read a setting, if present.
pub fn get(db: &Path, key: &str) -> Result<Option<String>> {
    let conn = open(db)?;
    let v = conn
        .query_row("SELECT value FROM settings WHERE key = ?1", [key], |r| {
            r.get::<_, String>(0)
        })
        .optional()?;
    Ok(v)
}

/// Remove a setting, if present.
pub fn remove(db: &Path, key: &str) -> Result<()> {
    let conn = open(db)?;
    conn.execute("DELETE FROM settings WHERE key = ?1", params![key])?;
    Ok(())
}

/// Insert or update a setting.
pub fn set(db: &Path, key: &str, value: &str) -> Result<()> {
    let conn = open(db)?;
    conn.execute(
        "INSERT INTO settings (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, value],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remove_deletes_a_setting() {
        let db = std::env::temp_dir().join(format!("bc_settings_rm_{}.db", std::process::id()));
        let _ = std::fs::remove_file(&db);
        set(&db, "deepseek_api_key", "sk-test").unwrap();
        assert_eq!(
            get(&db, "deepseek_api_key").unwrap().as_deref(),
            Some("sk-test")
        );
        remove(&db, "deepseek_api_key").unwrap();
        assert!(get(&db, "deepseek_api_key").unwrap().is_none());
        // Removing what is not there is not an error.
        remove(&db, "deepseek_api_key").unwrap();
        let _ = std::fs::remove_file(&db);
    }

    #[test]
    fn round_trips() {
        let path = std::env::temp_dir().join(format!("bc_settings_{}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        assert_eq!(get(&path, "lang").unwrap(), None);
        set(&path, "lang", "ru").unwrap();
        assert_eq!(get(&path, "lang").unwrap().as_deref(), Some("ru"));
        set(&path, "lang", "zh").unwrap();
        assert_eq!(get(&path, "lang").unwrap().as_deref(), Some("zh"));
        let _ = std::fs::remove_file(&path);
    }
}
