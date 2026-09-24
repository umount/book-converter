//! Exporting the translated book to a chosen format (TXT, FB2, EPUB, PDF).

pub(crate) mod epub;
pub(crate) mod fb2;
pub(crate) mod pdf;
pub(crate) mod txt;

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use regex::Regex;

use crate::book::marker_id;

/// A picture an exported book embeds, as it sits in the project directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportImage {
    pub path: PathBuf,
    pub content_type: String,
}

/// A piece of a chapter body, as the writers consume it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Piece<'a> {
    Para(&'a str),
    /// A picture that belongs at this exact point, by asset id.
    Image(&'a str),
}

/// Split a chapter body into its paragraphs and the pictures between them.
///
/// Every format walks a body this way, so the `[[img:…]]` convention is read in
/// one place — and a format that cannot hold a picture (plain text) simply
/// ignores the image pieces.
pub fn pieces(body: &str) -> Vec<Piece<'_>> {
    body.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(|line| match marker_id(line) {
            Some(id) => Piece::Image(id),
            None => Piece::Para(line),
        })
        .collect()
}

/// A translated chapter ready for export.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranslatedChapter {
    /// Sequential position (for ordering when `number` is absent).
    pub index: usize,
    /// Chapter number, used to order/merge across sources (e.g. continuation).
    pub number: Option<usize>,
    pub title: String,
    pub body: ChapterBody,
}

/// The versioned pipeline supplies explicit blocks. Legacy strings are isolated here
/// until old callers are removed; structural text is never parsed as image markup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChapterBody {
    Legacy(String),
    Blocks(Vec<ExportBlock>),
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExportBlock {
    Text(String),
    Image(String),
}
impl From<String> for ChapterBody {
    fn from(value: String) -> Self {
        Self::Legacy(value)
    }
}
impl From<&str> for ChapterBody {
    fn from(value: &str) -> Self {
        Self::Legacy(value.into())
    }
}
impl ChapterBody {
    pub fn pieces(&self) -> Vec<Piece<'_>> {
        match self {
            Self::Legacy(text) => pieces(text),
            Self::Blocks(blocks) => blocks
                .iter()
                .flat_map(|block| match block {
                    ExportBlock::Text(text) => text.lines().map(Piece::Para).collect(),
                    ExportBlock::Image(id) => vec![Piece::Image(id)],
                })
                .collect(),
        }
    }
    pub fn plain_text(&self) -> String {
        match self {
            Self::Legacy(text) => crate::book::strip_markers(text),
            Self::Blocks(blocks) => blocks
                .iter()
                .filter_map(|block| match block {
                    ExportBlock::Text(text) => Some(text.as_str()),
                    ExportBlock::Image(_) => None,
                })
                .collect::<Vec<_>>()
                .join("\n\n"),
        }
    }
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
        path.extension()
            .and_then(|e| e.to_str())
            .and_then(Self::from_ext)
    }

    pub fn from_ext(ext: &str) -> Option<Self> {
        match ext
            .trim()
            .trim_start_matches('.')
            .to_ascii_lowercase()
            .as_str()
        {
            "txt" => Some(Self::Txt),
            "fb2" => Some(Self::Fb2),
            "epub" => Some(Self::Epub),
            "pdf" => Some(Self::Pdf),
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
    /// Pictures the chapter bodies point at with `[[img:<id>]]`, by id.
    pub images: HashMap<String, ExportImage>,
}

impl Default for OutputMeta {
    fn default() -> Self {
        Self {
            title: "Untitled".into(),
            author: "Unknown".into(),
            lang: "ru".into(),
            annotation: None,
            cover: None,
            images: HashMap::new(),
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

    /// Every writer reads a body through `pieces`, so this is where the
    /// `[[img:…]]` convention is pinned down.
    #[test]
    fn pieces_separate_paragraphs_from_pictures() {
        assert_eq!(
            pieces("До.\n\n[[img:ab12]]\n\nПосле."),
            vec![
                Piece::Para("До."),
                Piece::Image("ab12"),
                Piece::Para("После."),
            ]
        );
        // A mention of the syntax inside a sentence is just text.
        assert_eq!(
            pieces("он сказал [[img:ab12]] вслух"),
            vec![Piece::Para("он сказал [[img:ab12]] вслух")]
        );
    }

    #[test]
    fn normalize_titles_fixes_markers() {
        let mut chs = vec![
            TranslatedChapter {
                index: 1,
                number: Some(516),
                title: "第516章 Запретная Земля".into(),
                body: "b".into(),
            },
            TranslatedChapter {
                index: 2,
                number: Some(525),
                title: "Том 525 Вырвать добычу".into(),
                body: "b".into(),
            },
            TranslatedChapter {
                index: 3,
                number: Some(514),
                title: "Глава 514. Но я его учитель!".into(),
                body: "b".into(),
            },
        ];
        normalize_titles(&mut chs, "Глава");
        assert_eq!(chs[0].title, "Глава 516. Запретная Земля");
        assert_eq!(chs[1].title, "Глава 525. Вырвать добычу");
        // already-correct titles stay stable (idempotent)
        assert_eq!(chs[2].title, "Глава 514. Но я его учитель!");
    }
}
