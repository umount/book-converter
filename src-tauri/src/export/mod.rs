//! Exporting the translated book to a chosen format (TXT, FB2, EPUB, PDF).

pub(crate) mod epub;
pub(crate) mod fb2;
pub(crate) mod pdf;
pub(crate) mod txt;

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::Result;


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

/// Explicit blocks preserve image identity and literal text independently.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChapterBody {
    Blocks(Vec<ExportBlock>),
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExportBlock {
    Text(String),
    Image(String),
}
impl From<String> for ChapterBody {
    fn from(value: String) -> Self {
        Self::Blocks(vec![ExportBlock::Text(value)])
    }
}
impl From<&str> for ChapterBody {
    fn from(value: &str) -> Self {
        Self::Blocks(vec![ExportBlock::Text(value.into())])
    }
}
impl ChapterBody {
    pub fn pieces(&self) -> Vec<Piece<'_>> {
        match self {
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

/// Output format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    Txt,
    Fb2,
    Epub,
    Pdf,
}

impl OutputFormat {
    /// File extension for this format.
    pub fn ext(self) -> &'static str {
        match self {
            Self::Txt => "txt",
            Self::Fb2 => "fb2",
            Self::Epub => "epub",
            Self::Pdf => "pdf",
        }
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
    /// Pictures referenced by explicit image blocks, keyed by asset ID.
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn structural_images_and_literal_markers_remain_distinct() {
        let body=ChapterBody::Blocks(vec![ExportBlock::Text("[[img:ab12]]".into()),ExportBlock::Image("ab12".into()),ExportBlock::Text("After.".into())]);
        assert_eq!(body.pieces(),vec![Piece::Para("[[img:ab12]]"),Piece::Image("ab12"),Piece::Para("After.")]);
        assert_eq!(body.plain_text(),"[[img:ab12]]\n\nAfter.");
    }
}
