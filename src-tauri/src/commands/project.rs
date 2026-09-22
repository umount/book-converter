//! Project creation and activation commands.

mod archive;
mod catalog;

pub use archive::*;
pub use catalog::*;

use std::path::Path;

use tauri::State;

use crate::book::load_book;
use crate::config::Config;
use crate::dto::{err, BookInfo};
use crate::export::fb2::Cover;
use crate::session::{db_path_for_project, write_manifest, AppState};
use crate::state::Store;

use super::util::ensure_chapters;

#[tauri::command]
pub async fn load_source(
    project_id: String,
    path: String,
    state: State<'_, AppState>,
) -> Result<BookInfo, String> {
    let mut book = load_book(Path::new(&path)).map_err(err)?;
    if book.chapters.is_empty() {
        ensure_chapters(&path, &mut book).await;
    }
    if book.chapters.is_empty() {
        return Err("no_text_extracted".into());
    }

    let db = db_path_for_project(&project_id).map_err(err)?;
    let store = Store::open(&db).map_err(err)?;
    store.init_chapters(&book.chapters).map_err(err)?;
    write_manifest(&project_id, &path, None).map_err(err)?;

    let title = book.meta.title.clone().unwrap_or_default();
    let author = book.meta.author.clone().unwrap_or_default();
    let head = if book.format == crate::book::InputFormat::Fb2 {
        crate::book::read_book_file(Path::new(&path))
            .ok()
            .map(|decoded| crate::export::fb2::extract_head(&decoded.text))
    } else {
        None
    };
    let pdf_cover = if path.to_lowercase().ends_with(".pdf") {
        use base64::Engine as _;
        std::fs::read(&path)
            .ok()
            .and_then(|bytes| crate::book::extract_pdf_cover(&bytes))
            .map(|(content_type, bytes)| Cover {
                content_type,
                base64: base64::engine::general_purpose::STANDARD.encode(bytes),
            })
    } else {
        None
    };

    let saved = store.project_metadata().map_err(err)?;
    let saved_cover = match (saved.cover_content_type.clone(), saved.cover_base64.clone()) {
        (Some(content_type), Some(base64)) => Some(Cover {
            content_type,
            base64,
        }),
        _ => None,
    };
    let summary = saved
        .summary
        .or_else(|| head.as_ref().and_then(|value| value.annotation.clone()));
    let cover = saved_cover
        .or_else(|| head.as_ref().and_then(|value| value.cover.clone()))
        .or(pdf_cover);

    let format = format!("{:?}", book.format);
    store
        .set_source_metadata(&title, &author, &format, &book.encoding)
        .map_err(err)?;
    // Snapshot the language pair at import so later Settings changes do not
    // rewrite this book. Detection / the setup modal may overwrite these.
    let cfg = Config::load();
    store
        .set_translation_langs(&cfg.source_lang, &cfg.target_lang)
        .map_err(err)?;
    if let Some(summary) = &summary {
        store.set_meta("summary", summary).map_err(err)?;
    }
    if let Some(cover) = &cover {
        store
            .set_cover_meta(Some(&cover.content_type), Some(&cover.base64))
            .map_err(err)?;
    }

    state.with(&project_id, |session| {
        session.cancel = None;
        session.running = false;
        session.db_path = Some(db);
    });

    Ok(with_langs(
        &store,
        BookInfo {
            title,
            author,
            total_chapters: book.chapters.len(),
            format,
            encoding: book.encoding,
            needs_delimiter: book.needs_delimiter,
            missing: book.report.missing_numbers.len(),
            duplicates: book.report.duplicate_numbers.len(),
            had_errors: book.encoding_had_errors,
            source_lang: String::new(),
            target_lang: String::new(),
        },
    ))
}

#[tauri::command]
pub async fn open_project(
    project_id: String,
    state: State<'_, AppState>,
) -> Result<BookInfo, String> {
    let db = db_path_for_project(&project_id).map_err(err)?;
    if !Path::new(&db).exists() {
        return Err("no_source".into());
    }
    let store = Store::open(&db).map_err(err)?;
    let _ = store.recover();
    let stats = store.stats().map_err(err)?;
    if stats.total == 0 {
        return Err("no_source".into());
    }
    let metadata = store.project_metadata().map_err(err)?;

    state.with(&project_id, |session| {
        session.cancel = None;
        session.running = false;
        session.db_path = Some(db);
    });

    Ok(with_langs(
        &store,
        BookInfo {
            title: metadata.title.unwrap_or_default(),
            author: metadata.author.unwrap_or_default(),
            total_chapters: stats.total,
            format: metadata.format.unwrap_or_else(|| "-".into()),
            encoding: metadata.encoding.unwrap_or_else(|| "-".into()),
            needs_delimiter: false,
            missing: 0,
            duplicates: 0,
            had_errors: false,
            source_lang: String::new(),
            target_lang: String::new(),
        },
    ))
}

/// Persist the language pair chosen in the new-book setup modal.
#[tauri::command]
pub async fn set_project_languages(
    project_id: String,
    source_lang: String,
    target_lang: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let source = source_lang.trim();
    let target = target_lang.trim();
    if source.is_empty() || target.is_empty() {
        return Err("bad_lang".into());
    }
    let store = super::ops::project_store(&state, &project_id)?;
    store.set_translation_langs(source, target).map_err(err)?;
    Ok(())
}

fn with_langs(store: &Store, mut info: BookInfo) -> BookInfo {
    let cfg = Config::load_for(store);
    info.source_lang = cfg.source_lang;
    info.target_lang = cfg.target_lang;
    info
}
