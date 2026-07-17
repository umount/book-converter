//! Exporting the translated book to TXT and EPUB.

pub mod epub;
pub mod txt;

/// A translated chapter ready for export.
#[derive(Debug, Clone)]
pub struct TranslatedChapter {
    pub index: usize,
    pub title: String,
    pub body: String,
}
