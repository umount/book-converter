//! Tauri commands — the bridge between the React frontend and the Rust core.
//!
//! Each `#[tauri::command]` is called from the frontend via `invoke(...)`.
//! Long-running translation pushes progress via events (`app.emit("progress", ...)`)
//! so the UI updates without blocking.

use serde::{Deserialize, Serialize};

/// Book summary after parsing (for display in the UI).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BookSummary {
    pub title: String,
    pub author: String,
    pub total_chapters: usize,
}

/// Translation progress (pushed via event and/or fetched on demand).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Progress {
    pub done: usize,
    pub total: usize,
    pub failed: usize,
    pub running: bool,
    pub status_line: String,
}

/// A glossary entry in a frontend-friendly shape.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TermDto {
    pub source: String,
    pub target: String,
    pub kind: String,
    pub frequency: u32,
    pub pinned: bool,
}

/// Parse the book at `path`: split into chapters, initialize the SQLite progress store.
#[tauri::command]
pub async fn parse_book(_path: String) -> Result<BookSummary, String> {
    todo!("parse the book + initialize state::Store")
}

/// Start/resume translation (only `pending`/`failed` chapters).
#[tauri::command]
pub async fn start_translation() -> Result<(), String> {
    todo!("start the worker pool with concurrency and emit progress")
}

/// Pause translation.
#[tauri::command]
pub async fn pause_translation() -> Result<(), String> {
    todo!("stop the pool after in-flight chapters finish")
}

/// Current progress.
#[tauri::command]
pub async fn get_progress() -> Result<Progress, String> {
    todo!("read progress from state::Store")
}

/// The whole glossary for the UI table.
#[tauri::command]
pub async fn get_glossary() -> Result<Vec<TermDto>, String> {
    todo!("read the glossary")
}

/// Manually edit/pin a term from the UI.
#[tauri::command]
pub async fn update_term(_term: TermDto) -> Result<(), String> {
    todo!("upsert a term with pinned=true")
}

/// Export the result to TXT and/or EPUB.
#[tauri::command]
pub async fn export_book(_out_dir: String, _formats: Vec<String>) -> Result<Vec<String>, String> {
    todo!("export to txt/epub, return the file paths")
}
