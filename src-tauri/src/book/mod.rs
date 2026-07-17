//! Working with the source book: decoding, parsing into chapters, and chunking.

pub mod chunker;
pub mod parser;
pub mod source;

pub use chunker::{split_chapter, Chunk};
pub use parser::{parse_book_meta, parse_chapters, validate, BookMeta, Chapter, ParseReport};
pub use source::{decode_book_bytes, read_book_file, DecodedText};
