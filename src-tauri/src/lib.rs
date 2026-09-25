//! book-converter — application core (Rust) for translating book-length works
//! between languages via provider profiles. GUI is Tauri + React.
//!
//! The language pair is fixed at project creation: nothing here is
//! written for one pair or one book (see `DECISIONS.md`, "Universal converter").
//!
//! Architecture and design: see `docs/ARCHITECTURE.md` and `docs/DECISIONS.md`.
//! The modules below are UI-agnostic; the frontend calls them via `commands`.

pub mod ai;
pub mod app;
pub mod application;
pub mod project;
pub mod assets;
mod book;
mod commands;
mod config;
mod export;
mod i18n;
pub mod jobs;
pub mod models;
mod language;
mod paths;
mod retarget;
mod settings;
pub mod storage;
mod textutil;

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
        .invoke_handler(tauri::generate_handler![
            commands::model_list,
            commands::model_download,
            commands::model_pause,
            commands::model_remove,
            commands::project_inspect_manifest,
            commands::project_settings_get,
            commands::project_settings_update,
            commands::glossary_list,
            commands::glossary_put,
            commands::glossary_delete,
            commands::book_start_glossary,
            commands::book_update_instructions,
            commands::book_start_metadata,
            commands::book_metadata_get,
            commands::book_presentation_get,
            commands::book_presentation_update,
            commands::book_cover_set,
            commands::book_reference_import,
            commands::book_reference_get,
            commands::book_reference_map,
            commands::book_export,
            commands::book_update_block,
            commands::book_replace_preview,
            commands::book_replace_apply,
            commands::book_get_chapter,
            commands::book_search,
            commands::book_list_chapters,
            commands::book_start_translation,
            commands::job_cancel,
            commands::job_resume,
            commands::job_get,
            commands::job_list,
            commands::book_update_translation_block,
            commands::book_update_translation_title,
            commands::book_start_title,
            commands::book_start_retarget,
            commands::book_retarget_preview,

            commands::manga_list_pages,
            commands::manga_list_volumes,
            commands::manga_start_stage,
            commands::manga_start_automatic,
            commands::manga_get_page,
            commands::manga_preflight,
            commands::project_list,
            commands::project_inspect_source,
            commands::project_create,
            commands::project_cancel_import,
            commands::project_open,
            commands::project_delete,
            commands::project_archive_export,
            commands::project_archive_import,

            commands::get_setting,
            commands::set_setting,
            commands::set_api_key,
            commands::get_effective_config,
            commands::provider_profiles_list,
            commands::provider_profile_save,
            commands::get_app_info,
            commands::assistant_project_view,
            commands::assistant_project_send,
            commands::assistant_project_confirm,
            commands::assistant_project_cancel,
        ])
        .run(tauri::generate_context!())
        .expect("failed to start the Tauri application");
}
