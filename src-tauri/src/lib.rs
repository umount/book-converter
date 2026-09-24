//! book-converter — application core (Rust) for translating book-length works
//! between languages via the DeepSeek API. GUI is Tauri + React.
//!
//! The language pair is a user setting, not a build-time fact: nothing here is
//! written for one pair or one book (see `DECISIONS.md`, "Universal converter").
//!
//! Architecture and design: see `docs/ARCHITECTURE.md` and `docs/DECISIONS.md`.
//! The modules below are UI-agnostic; the frontend calls them via `commands`.

pub mod ai;
pub mod app;
pub mod application;
pub mod project;
pub mod assets;
mod assistant;
mod book;
mod commands;
mod config;
mod dto;
mod export;
mod glossary;
mod i18n;
pub mod jobs;
mod language;
mod orchestrator;
mod paths;
mod reference;
mod retarget;
mod session;
mod settings;
mod state;
pub mod storage;
mod textutil;
mod translator;

/// Full product name, shown in the window title and the About dialog. The
/// package/bundle id stays `book-converter`; this is the human-facing name.
pub const APP_NAME: &str = "Book Converter";

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
        .manage(app::services::AppContext::default())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        // Page images are fetched by the webview instead of travelling through
        // the IPC bridge as data URLs; see `assets`.
        .register_uri_scheme_protocol(assets::SCHEME, assets::respond)
        .setup(|app| {
            // Locate the bundled pdfium library (for cross-platform PDF text): the
            // app's resource dir (packaged builds) or next to the executable (raw
            // `cargo`/`target/release` runs). Falls back to env / system pdfium.
            use tauri::Manager;
            let context = app.state::<app::services::AppContext>();
            application::runtime::recover_interrupted(&context.manager)
                .map_err(|error| std::io::Error::other(format!("Job recovery failed: {error:?}")))?;
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
        .manage(std::sync::Arc::new(assistant::AssistantRuntime::new()))
        .invoke_handler(tauri::generate_handler![
            commands::project_inspect_manifest,
            commands::book_update_instructions,
            commands::book_start_metadata,
            commands::book_metadata_get,
            commands::book_reference_import,
            commands::book_reference_get,
            commands::book_reference_map,
            commands::book_export,
            commands::book_update_block,
            commands::book_replace_preview,
            commands::book_replace_apply,
            commands::book_get_chapter,
            commands::book_list_chapters,
            commands::book_start_translation,
            commands::job_cancel,
            commands::job_resume,
            commands::job_get,
            commands::job_list,
            commands::book_update_translation_block,

            commands::project_list,
            commands::project_inspect_source,
            commands::project_create,
            commands::project_cancel_import,
            commands::project_open,
            commands::project_delete,
            commands::project_archive_export,
            commands::project_archive_import,

            commands::load_source,
            commands::open_project,
            commands::set_project_languages,
            commands::list_projects,
            commands::load_reference,
            commands::get_reference_info,
            commands::backfill_reference_head,
            commands::bootstrap_glossary,
            commands::harvest_glossary,
            commands::use_reference_as_base,
            commands::start_translation,
            commands::pause_translation,
            commands::reset_translation,
            commands::translate_chapter,
            commands::translate_chapter_title,
            commands::update_chapter_translation,
            commands::replace_in_book,
            commands::search_book,
            commands::get_progress,
            commands::get_glossary_page,
            commands::chapter_terms,
            commands::update_term,
            commands::delete_term,
            commands::retarget_terms,
            commands::export_book,
            commands::get_book_details,
            commands::translate_title,
            commands::set_summary,
            commands::set_book_prompt,
            commands::generate_summary,
            commands::set_cover,
            commands::list_chapters,
            commands::get_chapter,
            commands::set_chapter_prompt,
            commands::set_chapter_context,
            commands::get_setting,
            commands::set_setting,
            commands::set_api_key,
            commands::get_effective_config,
            commands::get_app_info,
            commands::delete_project,
            commands::export_project,
            commands::import_project,
            commands::assistant_history,
            commands::assistant_clear,
            commands::assistant_state,
            commands::assistant_send,
            commands::assistant_approve,
            commands::assistant_set_auto_run,
            commands::assistant_cancel,
        ])
        .run(tauri::generate_context!())
        .expect("failed to start the Tauri application");
}
