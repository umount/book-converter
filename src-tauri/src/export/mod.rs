//! Экспорт переведённой книги в TXT и EPUB.

pub mod epub;
pub mod txt;

/// Переведённая глава для экспорта.
#[derive(Debug, Clone)]
pub struct TranslatedChapter {
    pub index: usize,
    pub title: String,
    pub body: String,
}
