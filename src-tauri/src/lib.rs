//! book-converter — ядро приложения (Rust) для перевода больших книг
//! (китайский → русский) через DeepSeek API. GUI — Tauri + React.
//!
//! Архитектура: `docs/ARCHITECTURE.md`, план работ: `docs/ROADMAP.md`.
//! Модули ниже UI-агностичны; фронтенд вызывает их через `commands`.

mod book;
mod commands;
mod config;
mod export;
mod glossary;
mod state;
mod translator;

/// Собрать и запустить Tauri-приложение.
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "book_converter=info".into()),
        )
        .init();

    tracing::info!("book-converter запускается");

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            commands::parse_book,
            commands::start_translation,
            commands::pause_translation,
            commands::get_progress,
            commands::get_glossary,
            commands::update_term,
            commands::export_book,
        ])
        .run(tauri::generate_context!())
        .expect("ошибка запуска Tauri-приложения");
}
