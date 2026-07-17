//! Работа с исходной книгой: парсинг на главы и разбиение на чанки.

pub mod chunker;
pub mod parser;

pub use chunker::{split_chapter, Chunk};
pub use parser::{parse_chapters, Chapter};
