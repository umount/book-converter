//! book-converter — application core (Rust) for translating large books
//! (Chinese → Russian) via the DeepSeek API. GUI is Tauri + React.
//!
//! Architecture and design: see `docs/ARCHITECTURE.md` and `docs/DECISIONS.md`.
//! The modules below are UI-agnostic; the frontend calls them via `commands`.

mod book;
mod commands;
mod config;
mod export;
mod glossary;
mod orchestrator;
mod reference;
mod state;
mod translator;

/// Build and run the Tauri application.
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "book_converter=info".into()),
        )
        .init();

    tracing::info!("book-converter starting");

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .manage(commands::AppState::new())
        .invoke_handler(tauri::generate_handler![
            commands::load_source,
            commands::load_reference,
            commands::bootstrap_glossary,
            commands::use_reference_as_base,
            commands::start_translation,
            commands::pause_translation,
            commands::get_progress,
            commands::get_glossary,
            commands::update_term,
            commands::export_book,
            commands::get_book_details,
            commands::translate_title,
            commands::set_summary,
            commands::set_cover,
            commands::list_chapters,
            commands::get_chapter,
        ])
        .run(tauri::generate_context!())
        .expect("failed to start the Tauri application");
}
