//! book-converter — application core (Rust) for translating large books
//! (Chinese → Russian) via the DeepSeek API. GUI is Tauri + React.
//!
//! Architecture and design: see `docs/ARCHITECTURE.md` and `docs/DECISIONS.md`.
//! The modules below are UI-agnostic; the frontend calls them via `commands`.

mod book;
mod commands;
mod config;
mod dto;
mod export;
mod glossary;
mod i18n;
mod jobs;
mod orchestrator;
mod paths;
mod reference;
mod retarget;
mod session;
mod settings;
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

    // Drop legacy flat progress DBs from before the per-project layout.
    commands::cleanup_legacy_data();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            // Locate the bundled pdfium library (for cross-platform PDF text): the
            // app's resource dir (packaged builds) or next to the executable (raw
            // `cargo`/`target/release` runs). Falls back to env / system pdfium.
            use tauri::Manager;
            let name = pdfium_render::prelude::Pdfium::pdfium_platform_library_name_at_path;
            let mut dirs: Vec<std::path::PathBuf> = Vec::new();
            if let Ok(d) = app.path().resource_dir() {
                dirs.push(d.join("pdfium"));
            }
            if let Ok(exe) = std::env::current_exe() {
                if let Some(d) = exe.parent() {
                    dirs.push(d.join("pdfium"));
                }
            }
            if let Some(lib) = dirs.into_iter().map(|d| name(&d)).find(|p| p.exists()) {
                book::set_pdfium_lib_path(Some(lib.to_string_lossy().into_owned()));
            }
            Ok(())
        })
        .manage(commands::AppState::new())
        .invoke_handler(tauri::generate_handler![
            commands::load_source,
            commands::open_project,
            commands::load_reference,
            commands::bootstrap_glossary,
            commands::use_reference_as_base,
            commands::start_translation,
            commands::pause_translation,
            commands::reset_translation,
            commands::translate_chapter,
            commands::update_chapter_translation,
            commands::get_progress,
            commands::get_glossary,
            commands::update_term,
            commands::delete_term,
            commands::retarget_terms,
            commands::export_book,
            commands::get_book_details,
            commands::translate_title,
            commands::set_summary,
            commands::generate_summary,
            commands::set_cover,
            commands::list_chapters,
            commands::get_chapter,
            commands::set_chapter_prompt,
            commands::get_setting,
            commands::set_setting,
            commands::delete_project,
            commands::export_project,
            commands::import_project,
        ])
        .run(tauri::generate_context!())
        .expect("failed to start the Tauri application");
}
