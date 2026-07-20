//! Exporting the translated book to a chosen format (TXT, FB2; EPUB planned).

pub mod epub;
pub mod fb2;
pub mod pdf;
pub mod txt;

use std::path::Path;

use anyhow::{Context, Result};
use regex::Regex;

/// A translated chapter ready for export.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranslatedChapter {
    /// Sequential position (for ordering when `number` is absent).
    pub index: usize,
    /// Chapter number, used to order/merge across sources (e.g. continuation).
    pub number: Option<usize>,
    pub title: String,
    pub body: String,
}

impl TranslatedChapter {
    /// Build from `state::Store::translated_chapters` rows `(index, title, body)`.
    pub fn from_rows(rows: Vec<(usize, String, String)>) -> Vec<Self> {
        rows.into_iter()
            .map(|(index, title, body)| Self { index, number: None, title, body })
            .collect()
    }

}

/// Merge an existing translation with newly translated chapters (the "continue
/// translation" case). Numbered chapters are ordered by number; a `new` chapter
/// overrides an `existing` one with the same number. Unnumbered chapters keep
/// their order at the end.
pub fn combine(
    existing: Vec<TranslatedChapter>,
    new: Vec<TranslatedChapter>,
) -> Vec<TranslatedChapter> {
    use std::collections::BTreeMap;

    let mut by_number: BTreeMap<usize, TranslatedChapter> = BTreeMap::new();
    let mut unnumbered: Vec<TranslatedChapter> = Vec::new();

    for ch in existing.into_iter().chain(new) {
        match ch.number {
            Some(n) => {
                by_number.insert(n, ch);
            }
            None => unnumbered.push(ch),
        }
    }

    let mut result: Vec<TranslatedChapter> = by_number.into_values().collect();
    result.extend(unnumbered);
    result
}

/// Normalize numbered chapter titles to a uniform "`<label> <n>. <name>`" form.
///
/// Strips a leading chapter marker the model may have left untranslated (`第N章`,
/// `Том N`, `Chapter N`, a bare number, or an existing `<label> N[.M]`) and
/// re-prefixes a consistent label. Idempotent, so re-running is safe.
pub fn normalize_titles(chapters: &mut [TranslatedChapter], label: &str) {
    let marker = leading_marker_regex();
    for ch in chapters.iter_mut() {
        if let Some(n) = ch.number {
            let name = marker.replace(ch.title.trim(), "").trim().to_string();
            ch.title = if name.is_empty() {
                format!("{label} {n}")
            } else {
                format!("{label} {n}. {name}")
            };
        }
    }
}

fn leading_marker_regex() -> Regex {
    // A leading chapter marker + trailing separators, any of these forms.
    Regex::new(
        r"(?ix)^\s*(?:
            第[0-9一二三四五六七八九十百千零两〇]+章 |
            глава \s+ [0-9.]+ |
            том \s+ [0-9]+ |
            chapter \s+ [0-9]+ |
            [0-9]+
        )\s*[.:、]?\s*",
    )
    .expect("leading marker regex is valid")
}

/// Output format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    Txt,
    Fb2,
    Epub,
    Pdf,
}

impl OutputFormat {
    /// Infer the format from a file extension.
    pub fn from_path(path: &Path) -> Option<Self> {
        match path
            .extension()
            .and_then(|e| e.to_str())
            .map(|s| s.to_ascii_lowercase())
            .as_deref()
        {
            Some("txt") => Some(Self::Txt),
            Some("fb2") => Some(Self::Fb2),
            Some("epub") => Some(Self::Epub),
            Some("pdf") => Some(Self::Pdf),
            _ => None,
        }
    }

    /// File extension for this format.
    pub fn ext(self) -> &'static str {
        match self {
            Self::Txt => "txt",
            Self::Fb2 => "fb2",
            Self::Epub => "epub",
            Self::Pdf => "pdf",
        }
    }

    /// Binary formats (EPUB, PDF) are written directly, never rendered to a
    /// string or re-zipped.
    pub fn is_binary(self) -> bool {
        matches!(self, Self::Epub | Self::Pdf)
    }
}

/// Book metadata used when writing an output file.
#[derive(Debug, Clone)]
pub struct OutputMeta {
    pub title: String,
    pub author: String,
    /// Language tag for FB2 (`<lang>`), e.g. "ru".
    pub lang: String,
    /// Annotation / summary (target language).
    pub annotation: Option<String>,
    /// Cover image.
    pub cover: Option<fb2::Cover>,
}

impl Default for OutputMeta {
    fn default() -> Self {
        Self {
            title: "Untitled".into(),
            author: "Unknown".into(),
            lang: "ru".into(),
            annotation: None,
            cover: None,
        }
    }
}

/// Render the book to a string in the given format.
pub fn render(chapters: &[TranslatedChapter], format: OutputFormat, meta: &OutputMeta) -> String {
    match format {
        OutputFormat::Txt => txt::render(chapters),
        OutputFormat::Fb2 => fb2::render(chapters, meta),
        // Binary formats are written directly by `export`, not rendered to a
        // string; callers must not route them through render()/export_zip().
        OutputFormat::Epub | OutputFormat::Pdf => String::new(),
    }
}

/// Write `chapters` to `out_path` in the given format.
pub fn export(
    chapters: &[TranslatedChapter],
    format: OutputFormat,
    meta: &OutputMeta,
    out_path: &Path,
) -> Result<()> {
    match format {
        OutputFormat::Txt => txt::export(chapters, out_path),
        OutputFormat::Fb2 => fb2::export(chapters, meta, out_path),
        OutputFormat::Epub => epub::export(chapters, meta, out_path),
        OutputFormat::Pdf => pdf::export(chapters, meta, out_path),
    }
}

/// Write `chapters` into a `.zip` at `zip_path`, containing a single file named
/// `inner_name` (e.g. "book.fb2"). Used when the output should be zipped.
pub fn export_zip(
    chapters: &[TranslatedChapter],
    format: OutputFormat,
    meta: &OutputMeta,
    inner_name: &str,
    zip_path: &Path,
) -> Result<()> {
    use std::io::Write as _;

    let content = render(chapters, format, meta);
    let file = std::fs::File::create(zip_path)
        .with_context(|| format!("creating {}", zip_path.display()))?;
    let mut zip = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    zip.start_file(inner_name, options)?;
    zip.write_all(content.as_bytes())?;
    zip.finish()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ch(number: usize, body: &str) -> TranslatedChapter {
        TranslatedChapter {
            index: number,
            number: Some(number),
            title: format!("Гл {number}"),
            body: body.into(),
        }
    }

    #[test]
    fn combine_appends_and_orders() {
        let existing = vec![ch(1, "pro1"), ch(2, "pro2")];
        let new = vec![ch(3, "new3"), ch(4, "new4")];
        let merged = combine(existing, new);
        assert_eq!(
            merged.iter().map(|c| c.number.unwrap()).collect::<Vec<_>>(),
            vec![1, 2, 3, 4]
        );
        assert_eq!(merged[2].body, "new3");
    }

    #[test]
    fn combine_new_overrides_same_number() {
        let existing = vec![ch(1, "old")];
        let new = vec![ch(1, "fresh")];
        let merged = combine(existing, new);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].body, "fresh");
    }

    #[test]
    fn normalize_titles_fixes_markers() {
        let mut chs = vec![
            TranslatedChapter { index: 1, number: Some(516), title: "第516章 Запретная Земля".into(), body: "b".into() },
            TranslatedChapter { index: 2, number: Some(525), title: "Том 525 Вырвать добычу".into(), body: "b".into() },
            TranslatedChapter { index: 3, number: Some(514), title: "Глава 514. Но я его учитель!".into(), body: "b".into() },
        ];
        normalize_titles(&mut chs, "Глава");
        assert_eq!(chs[0].title, "Глава 516. Запретная Земля");
        assert_eq!(chs[1].title, "Глава 525. Вырвать добычу");
        // already-correct titles stay stable (idempotent)
        assert_eq!(chs[2].title, "Глава 514. Но я его учитель!");
    }
}
