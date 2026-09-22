//! Format-agnostic book loading: detect the input format, decode, and parse into
//! chapters — the single entry point the rest of the app uses.
//!
//! Supports TXT, FB2, PDF and EPUB. When a TXT layout has no recognizable chapter
//! headings, `needs_delimiter` is set so the caller can fall back to a
//! model-inferred delimiter (`parser::build_delimiter_prompt` +
//! `parser::parse_chapters_with`).

use std::path::Path;

use anyhow::Result;

use super::fb2::{fb2_to_chapters, parse_fb2};
use super::parser::{parse_book_meta, parse_chapters, validate, BookMeta, Chapter, ParseReport};
use super::source::{is_epub, read_book_file};

/// Detected input format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputFormat {
    Txt,
    Fb2,
    Epub,
}

/// A loaded book, format-agnostic.
#[derive(Debug, Clone)]
pub struct LoadedBook {
    pub format: InputFormat,
    /// Encoding the bytes were decoded from (e.g. "UTF-8", "GBK").
    pub encoding: String,
    pub meta: BookMeta,
    pub chapters: Vec<Chapter>,
    pub report: ParseReport,
    /// TXT only: no chapter pattern matched — needs a model-inferred delimiter.
    pub needs_delimiter: bool,
    /// The decode produced replacement characters — the encoding guess is likely
    /// wrong (garbled text). Only set by `load_book` (which owns the raw bytes).
    pub encoding_had_errors: bool,
    /// Embedded cover (EPUB), if the package declared one.
    pub cover: Option<(String, Vec<u8>)>,
}

/// Detect the input format from the decoded text (content-based, not extension).
pub fn detect_format(text: &str) -> InputFormat {
    let head: String = text.chars().take(2000).collect();
    if head.contains("<FictionBook") {
        InputFormat::Fb2
    } else {
        InputFormat::Txt
    }
}

/// Read a file, decode it, detect the format, and parse into chapters.
pub fn load_book(path: &Path) -> Result<LoadedBook> {
    if is_epub(path) {
        return super::epub::load(path);
    }
    let decoded = read_book_file(path)?;
    let mut book = load_book_text(&decoded.text, decoded.encoding)?;
    book.encoding_had_errors = decoded.had_errors;
    Ok(book)
}

/// Parse already-decoded text (encoding is passed through for reporting).
pub fn load_book_text(text: &str, encoding: &str) -> Result<LoadedBook> {
    let book = match detect_format(text) {
        InputFormat::Fb2 => {
            let doc = parse_fb2(text)?;
            let chapters = fb2_to_chapters(&doc);
            let report = validate(&chapters, &doc.meta);
            LoadedBook {
                format: InputFormat::Fb2,
                encoding: encoding.to_string(),
                meta: doc.meta,
                chapters,
                report,
                needs_delimiter: false,
                encoding_had_errors: false,
                cover: None,
            }
        }
        InputFormat::Txt => {
            let meta = parse_book_meta(text);
            let chapters = parse_chapters(text);
            let needs_delimiter = chapters.is_empty();
            let report = validate(&chapters, &meta);
            LoadedBook {
                format: InputFormat::Txt,
                encoding: encoding.to_string(),
                meta,
                chapters,
                report,
                needs_delimiter,
                encoding_had_errors: false,
                cover: None,
            }
        }
        InputFormat::Epub => anyhow::bail!("epub is loaded from a zip, not decoded text"),
    };
    Ok(book)
}

#[cfg(test)]
mod tests {
    use super::*;

    const FB2: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<FictionBook xmlns="http://www.gribuser.ru/xml/fictionbook/2.0">
<description><title-info><book-title>T</book-title></title-info></description>
<body>
<section><title><p>第1章 A</p></title><p>one</p></section>
<section><title><p>第2章 B</p></title><p>two</p></section>
</body></FictionBook>"#;

    #[test]
    fn detects_fb2_and_parses() {
        let book = load_book_text(FB2, "UTF-8").unwrap();
        assert_eq!(book.format, InputFormat::Fb2);
        assert_eq!(book.chapters.len(), 2);
    }

    #[test]
    fn detects_txt_and_parses() {
        let txt = "Book\n\n第1章 A\none\n\n第2章 B\ntwo\n";
        let book = load_book_text(txt, "UTF-8").unwrap();
        assert_eq!(book.format, InputFormat::Txt);
        assert_eq!(book.chapters.len(), 2);
        assert!(!book.needs_delimiter);
    }

    #[test]
    fn unknown_layout_flags_needs_delimiter() {
        let txt = "Just some prose with no headings whatsoever.\n";
        let book = load_book_text(txt, "UTF-8").unwrap();
        assert_eq!(book.format, InputFormat::Txt);
        assert!(book.chapters.is_empty());
        assert!(book.needs_delimiter);
    }

    #[test]
    fn numbered_dot_web_novel_sample_if_present() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../samples/苟在诸天从黑暗佛门开始.txt");
        if !path.exists() {
            return;
        }
        let book = load_book(&path).unwrap();
        assert_eq!(book.format, InputFormat::Txt);
        assert!(
            book.chapters.len() >= 140,
            "got {} chapters",
            book.chapters.len()
        );
        assert_eq!(book.chapters[0].number, Some(1));
        assert!(!book.needs_delimiter);
        assert_eq!(book.meta.title.as_deref(), Some("苟在诸天从黑暗佛门开始"));
        assert_eq!(book.meta.author.as_deref(), Some("是桃花酥呀"));
        let blurb = book.meta.summary.as_deref().expect("parsed 简介");
        assert!(blurb.contains("黑暗佛门"));
    }
}
