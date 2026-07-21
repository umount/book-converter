//! Working with the source book: decoding, format detection, parsing, chunking.

pub mod chunker;
pub mod fb2;
pub mod load;
pub mod parser;
pub mod pdf;
pub mod source;

pub use chunker::{split_chapter, Chunk};
pub use fb2::{fb2_to_chapters, parse_fb2, Fb2Doc, Fb2Section};
pub use load::{detect_format, load_book, load_book_text, InputFormat, LoadedBook};
pub use parser::{
    build_delimiter_prompt, detect_chapter_pattern, parse_book_meta, parse_chapters,
    parse_chapters_with, parse_inferred_pattern, validate, BookMeta, Chapter, ParseReport,
};
pub use pdf::{
    cover as extract_pdf_cover, looks_like_text, set_pdfium_lib_path,
    toc_chapters as extract_pdf_toc_chapters,
};
pub use source::{decode_book_bytes, read_book_file, DecodedText};
