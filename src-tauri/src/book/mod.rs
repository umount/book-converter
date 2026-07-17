//! Working with the source book: parsing into chapters and splitting into chunks.

pub mod chunker;
pub mod parser;

pub use chunker::{split_chapter, Chunk};
pub use parser::{parse_chapters, Chapter};
