//! Parse the source `.txt` into chapters by markers like `第一章 ...`.
//!
//! The book `光阴之外` has ~990 chapters; boundaries are given by the line
//! "第N章 Title". Line endings may be mixed CRLF/CR — normalize on read.

/// A single chapter of the book.
#[derive(Debug, Clone)]
pub struct Chapter {
    /// Ordinal number (1-based), as in the source.
    pub index: usize,
    /// Chapter title, e.g. "第一章 活着".
    pub title: String,
    /// Full chapter text (without the title).
    pub body: String,
}

/// Split the raw book text into chapters.
///
/// TODO:
/// 1. Normalize line endings (\r\n, \r → \n).
/// 2. Find chapter markers with the regex `第[一二三四五六七八九十百千零两]+章`.
/// 3. Collect a `Chapter` between adjacent markers.
pub fn parse_chapters(_raw: &str) -> Vec<Chapter> {
    todo!("split the book into chapters by 第N章 markers")
}
