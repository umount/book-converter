//! Tauri-команды — мост между React-фронтендом и Rust-ядром.
//!
//! Каждая `#[tauri::command]` вызывается из фронтенда через `invoke(...)`.
//! Долгий перевод шлёт прогресс событиями (`app.emit("progress", ...)`),
//! чтобы UI обновлялся, не блокируясь.

use serde::{Deserialize, Serialize};

/// Сводка по книге после парсинга (для отображения в UI).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BookSummary {
    pub title: String,
    pub author: String,
    pub total_chapters: usize,
}

/// Прогресс перевода (шлётся событием и/или по запросу).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Progress {
    pub done: usize,
    pub total: usize,
    pub failed: usize,
    pub running: bool,
    pub status_line: String,
}

/// Запись глоссария в виде, удобном фронтенду.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TermDto {
    pub source: String,
    pub target: String,
    pub kind: String,
    pub frequency: u32,
    pub pinned: bool,
}

/// Разобрать книгу по пути: распарсить главы, инициализировать SQLite-прогресс.
#[tauri::command]
pub async fn parse_book(_path: String) -> Result<BookSummary, String> {
    todo!("парсинг книги + инициализация state::Store")
}

/// Запустить/возобновить перевод (переводит только pending/failed главы).
#[tauri::command]
pub async fn start_translation() -> Result<(), String> {
    todo!("запуск пула перевода с параллелизмом и emit прогресса")
}

/// Поставить перевод на паузу.
#[tauri::command]
pub async fn pause_translation() -> Result<(), String> {
    todo!("остановка пула после текущих глав")
}

/// Текущий прогресс.
#[tauri::command]
pub async fn get_progress() -> Result<Progress, String> {
    todo!("чтение прогресса из state::Store")
}

/// Весь глоссарий для таблицы в UI.
#[tauri::command]
pub async fn get_glossary() -> Result<Vec<TermDto>, String> {
    todo!("чтение глоссария")
}

/// Ручная правка/фиксация термина (pinned) из UI.
#[tauri::command]
pub async fn update_term(_term: TermDto) -> Result<(), String> {
    todo!("upsert термина с pinned=true")
}

/// Экспорт результата в TXT и/или EPUB.
#[tauri::command]
pub async fn export_book(_out_dir: String, _formats: Vec<String>) -> Result<Vec<String>, String> {
    todo!("экспорт в txt/epub, вернуть пути к файлам")
}
