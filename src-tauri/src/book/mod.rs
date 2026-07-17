//! Working with the source book: decoding, parsing into chapters, and chunking.

pub mod chunker;
pub mod fb2;
pub mod parser;
pub mod source;

pub use chunker::{split_chapter, Chunk};
pub use fb2::{fb2_to_chapters, parse_fb2, Fb2Doc, Fb2Section};
pub use parser::{parse_book_meta, parse_chapters, validate, BookMeta, Chapter, ParseReport};
pub use source::{decode_book_bytes, read_book_file, DecodedText};
