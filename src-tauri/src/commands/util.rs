//! Shared helpers used by multiple command modules.

use std::collections::HashMap;
use std::path::Path;

use crate::book::load_book;
use crate::config::Config;
use crate::dto::err;
use crate::export::OutputFormat;
use crate::state::Store;
use crate::translator::DeepSeekClient;

pub(crate) fn client() -> Result<DeepSeekClient, String> {
    DeepSeekClient::new(Config::load()).map_err(err)
}

/// Persist a single meta value to the current project's DB (best-effort).
pub(crate) fn persist_meta(db: &Option<String>, key: &str, value: &str) {
    if let Some(db) = db {
        if let Ok(store) = Store::open(db) {
            let _ = store.set_meta(key, value);
        }
    }
}

pub(crate) fn cover_mime(path: &str) -> String {
    let ext = std::path::Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "png" => "image/png",
        "gif" => "image/gif",
        "webp" => "image/webp",
        _ => "image/jpeg",
    }
    .to_string()
}

/// When no chapter pattern matched, split the book intelligently: a PDF's table of
/// contents (outline) first, then a model-inferred delimiter, and finally the whole
/// book as one chapter so its text is never lost. Best-effort.
pub(crate) async fn ensure_chapters(path: &str, book: &mut crate::book::LoadedBook) {
    // 0. PDF with a table of contents (bookmarks): the most accurate split.
    if path.to_lowercase().ends_with(".pdf") {
        if let Some(toc) = crate::book::extract_pdf_toc_chapters(Path::new(path)) {
            if toc.len() >= 2 {
                book.chapters = toc
                    .into_iter()
                    .enumerate()
                    .map(|(i, (title, body))| crate::book::Chapter {
                        index: i + 1,
                        number: None,
                        title: title.replace('_', " "),
                        body,
                    })
                    .collect();
                book.report = crate::book::validate(&book.chapters, &book.meta);
                book.needs_delimiter = false;
                return;
            }
        }
    }

    let text = match crate::book::read_book_file(Path::new(path)) {
        Ok(d) => d.text,
        Err(_) => return,
    };
    // Don't build chapters from mis-decoded / empty text (a broken PDF): let the
    // caller report that instead of producing garbage.
    if !crate::book::looks_like_text(&text) {
        return;
    }

    // 1. Ask the model to infer a chapter-heading regex for this layout.
    if let Ok(cl) = DeepSeekClient::new(Config::load()) {
        let (system, user) = crate::book::build_delimiter_prompt(&text);
        if let Ok(reply) = cl.translate(&system, &user).await {
            if let Ok(re) = crate::book::parse_inferred_pattern(&reply) {
                let chapters = crate::book::parse_chapters_with(&text, &re);
                if chapters.len() >= 2 {
                    book.report = crate::book::validate(&chapters, &book.meta);
                    book.chapters = chapters;
                    book.needs_delimiter = false;
                    return;
                }
            }
        }
    }

    // 2. Fallback: a single chapter with the whole text (still translatable).
    let title = book.meta.title.clone().unwrap_or_else(|| "Book".to_string());
    let body = text.trim().to_string();
    if !body.is_empty() {
        book.chapters = vec![crate::book::Chapter {
            index: 1,
            number: Some(1),
            title,
            body,
        }];
        book.report = crate::book::validate(&book.chapters, &book.meta);
        book.needs_delimiter = false;
    }
}

/// Seed still-`pending` chapters from a reference translation (aligned by chapter
/// number), marking them `done` with `origin = 'reference'`. Never overwrites work
/// already done. Returns how many chapters were filled.
pub(crate) fn import_reference_pending(
    db: &str,
    source_path: &str,
    reference: &crate::reference::Reference,
) -> anyhow::Result<usize> {
    let source = load_book(Path::new(source_path))?;
    let store = Store::open(db)?;

    // Prefer chapter numbers: they survive different editions and languages.
    let idx_by_number: HashMap<usize, usize> = source
        .chapters
        .iter()
        .filter_map(|c| c.number.map(|n| (n, c.index)))
        .collect();

    let mut count = 0;
    if !idx_by_number.is_empty() {
        for rc in &reference.chapters {
            if let Some(&idx) = rc.number.and_then(|n| idx_by_number.get(&n)) {
                if store.save_reference_chapter(idx, &rc.title, &rc.body)? {
                    count += 1;
                }
            }
        }
    }

    // Positional fallback, for a reference (or a source) whose chapters carry no
    // numbers. Import is the only place alignment happens now, so the fallback
    // has to live here: everything downstream reads the aligned pairs from the
    // database.
    if count == 0 {
        for (sc, rc) in source.chapters.iter().zip(reference.chapters.iter()) {
            if store.save_reference_chapter(sc.index, &rc.title, &rc.body)? {
                count += 1;
            }
        }
    }
    Ok(count)
}

/// Resolved output: final path, format, whether to zip, and the inner file name.
pub(crate) struct OutputTarget {
    pub(crate) path: String,
    pub(crate) format: OutputFormat,
    pub(crate) zipped: bool,
    pub(crate) inner_name: String,
}

impl OutputTarget {
    pub(crate) fn resolve(out_path: &str, zipped_input: bool) -> Result<Self, String> {
        let ends_zip = out_path.to_ascii_lowercase().ends_with(".zip");
        // The book file part (path without a trailing .zip).
        let inner_path = if ends_zip {
            out_path[..out_path.len() - 4].to_string()
        } else {
            out_path.to_string()
        };
        let format = OutputFormat::from_path(Path::new(&inner_path)).unwrap_or(OutputFormat::Fb2);
        // Binary formats (EPUB/PDF) are written directly, never re-zipped.
        let zipped = (ends_zip || zipped_input) && !format.is_binary();

        let ext = format.ext();
        let inner_stem = Path::new(&inner_path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("book");
        let inner_name = if inner_stem.to_ascii_lowercase().ends_with(&format!(".{ext}")) {
            inner_stem.to_string()
        } else {
            format!("{inner_stem}.{ext}")
        };
        let path = if !zipped {
            out_path.to_string()
        } else if ends_zip {
            out_path.to_string()
        } else {
            format!("{out_path}.zip")
        };

        Ok(OutputTarget {
            path,
            format,
            zipped,
            inner_name,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_plain_fb2() {
        let t = OutputTarget::resolve("/tmp/book.fb2", false).unwrap();
        assert_eq!(t.path, "/tmp/book.fb2");
        assert_eq!(t.format, OutputFormat::Fb2);
        assert!(!t.zipped);
        assert_eq!(t.inner_name, "book.fb2");
    }

    #[test]
    fn resolve_zip_wrap_for_text_when_input_was_zipped() {
        let t = OutputTarget::resolve("/tmp/out.fb2", true).unwrap();
        assert!(t.zipped);
        assert_eq!(t.path, "/tmp/out.fb2.zip");
        assert_eq!(t.inner_name, "out.fb2");
    }

    #[test]
    fn resolve_never_rezips_binary_formats() {
        let epub = OutputTarget::resolve("/tmp/book.epub", true).unwrap();
        assert!(!epub.zipped);
        assert_eq!(epub.path, "/tmp/book.epub");
        let pdf = OutputTarget::resolve("/tmp/book.pdf.zip", true).unwrap();
        assert!(!pdf.zipped);
        assert_eq!(pdf.format, OutputFormat::Pdf);
    }
}

