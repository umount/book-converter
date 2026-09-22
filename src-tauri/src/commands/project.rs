//! Project creation and activation commands.

mod archive;
mod blocks;
mod catalog;

pub use archive::*;
pub use catalog::*;

use std::path::Path;

use tauri::State;

use crate::book::load_book;
use crate::config::Config;
use crate::dto::{err, BookInfo};
use crate::export::fb2::Cover;
use crate::session::{db_path_for_project, read_manifest, write_manifest, AppState};
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
        let code = if path.to_lowercase().ends_with(".pdf") {
            "no_text_extracted"
        } else {
            "empty_book"
        };
        return Err(code.into());
    }

    let db = db_path_for_project(&project_id).map_err(err)?;
    let store = Store::open(&db).map_err(err)?;
    store.init_chapters(&book.chapters).map_err(err)?;
    blocks::persist(&store, &project_id, &path, &book);
    write_manifest(&project_id, &path, None).map_err(err)?;

    let title = book
        .meta
        .title
        .clone()
        .filter(|s| !s.trim().is_empty())
        .or_else(|| {
            Path::new(&path)
                .file_stem()
                .and_then(|s| s.to_str())
                .map(str::to_string)
                .filter(|s| !s.is_empty())
        })
        .unwrap_or_default();
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
    let epub_cover = book.cover.as_ref().map(|(content_type, bytes)| {
        use base64::Engine as _;
        Cover {
            content_type: content_type.clone(),
            base64: base64::engine::general_purpose::STANDARD.encode(bytes),
        }
    });

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
        .or(pdf_cover)
        .or(epub_cover);

    let format = format!("{:?}", book.format);
    store
        .set_source_metadata(&title, &author, &format, &book.encoding)
        .map_err(err)?;
    let cfg = Config::load();
    let sample = crate::language::sample_book(&book.chapters);
    let detected = crate::language::detect_source_lang(&sample, &cfg.source_lang).await;
    store
        .set_translation_langs(&detected.name, &cfg.target_lang)
        .map_err(err)?;
    if let Some(blurb) = book
        .meta
        .summary
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        store.set_meta("source_summary", blurb).map_err(err)?;
    }
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
            source_detected: detected.detected,
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
    backfill_txt_meta(&store, &project_id);
    blocks::backfill(&store, &project_id);
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
            source_detected: metadata.source_lang.is_some(),
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
    let source = crate::language::canonical_lang(&source_lang).ok_or("bad_lang")?;
    let target = crate::language::canonical_lang(&target_lang).ok_or("bad_lang")?;
    let store = super::ops::project_store(&state, &project_id)?;
    store.set_translation_langs(source, target).map_err(err)?;
    Ok(())
}

/// Repair title/author/简介 on already-imported TXT projects that were parsed
/// before plain (non-`《》`) titles and blurbs were extracted.
fn backfill_txt_meta(store: &Store, project_id: &str) {
    let Ok(metadata) = store.project_metadata() else {
        return;
    };
    if metadata.format.as_deref() != Some("Txt") {
        return;
    }
    let missing_title = metadata
        .title
        .as_deref()
        .map(str::trim)
        .unwrap_or("")
        .is_empty();
    let missing_author = metadata
        .author
        .as_deref()
        .map(str::trim)
        .unwrap_or("")
        .is_empty();
    let missing_blurb = metadata
        .source_summary
        .as_deref()
        .map(str::trim)
        .unwrap_or("")
        .is_empty();
    if !missing_title {
        return;
    }
    let Ok(manifest) = read_manifest(project_id) else {
        return;
    };
    let path = Path::new(&manifest.source_path);
    if !path.exists() {
        return;
    }
    let Ok(decoded) = crate::book::read_book_file(path) else {
        return;
    };
    let parsed = crate::book::parser::parse_book_meta(&decoded.text);
    if missing_title {
        if let Some(title) = parsed.title.filter(|s| !s.trim().is_empty()) {
            let _ = store.set_meta("title", &title);
        } else if let Some(stem) = path
            .file_stem()
            .and_then(|s| s.to_str())
            .filter(|s| !s.is_empty())
        {
            let _ = store.set_meta("title", stem);
        }
    }
    if missing_author {
        if let Some(author) = parsed.author.filter(|s| !s.trim().is_empty()) {
            let _ = store.set_meta("author", &author);
        }
    }
    if missing_blurb {
        if let Some(blurb) = parsed.summary.filter(|s| !s.trim().is_empty()) {
            let _ = store.set_meta("source_summary", &blurb);
        }
    }
}

fn with_langs(store: &Store, mut info: BookInfo) -> BookInfo {
    let meta = store.project_metadata().ok();
    let cfg = Config::load().with_langs(
        meta.as_ref().and_then(|m| m.source_lang.as_deref()),
        meta.as_ref().and_then(|m| m.target_lang.as_deref()),
    );
    info.source_lang = cfg.source_lang;
    info.target_lang = cfg.target_lang;
    info
}
