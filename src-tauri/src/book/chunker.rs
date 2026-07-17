//! Split a chapter into chunks that fit a DeepSeek request limit.
//!
//! The base unit of translation is a chapter (~4300 Chinese chars on average,
//! fits in one request). This module is only a fallback: if a chapter is
//! abnormally long (> max_chunk_chars), split it on paragraph boundaries,
//! NEVER mid-sentence.

use super::parser::Chapter;

/// A part of a chapter sent as a single request.
#[derive(Debug, Clone)]
pub struct Chunk {
    /// Index of the chapter this chunk belongs to.
    pub chapter_index: usize,
    /// Ordinal number of the chunk within the chapter (0-based).
    pub part: usize,
    /// Total number of chunks in the chapter (for later reassembly).
    pub total_parts: usize,
    /// Chunk text.
    pub text: String,
}

/// Split a chapter into chunks no longer than `max_chunk_chars` characters.
///
/// If the chapter fits whole, returns a single `Chunk`.
/// Otherwise splits on paragraph boundaries (`\n\n`), never mid-sentence.
pub fn split_chapter(_chapter: &Chapter, _max_chunk_chars: usize) -> Vec<Chunk> {
    todo!("split the chapter into chunks on paragraph boundaries")
}
