//! Working with the source book: decoding, format detection, parsing, chunking.

pub(crate) mod blocks;
pub(crate) mod chunker;
pub(crate) mod epub;
pub(crate) mod fb2;
mod fb2_content;
pub(crate) mod load;
pub(crate) mod parser;
pub(crate) mod pdf;
pub(crate) mod source;

pub use blocks::{
    marker_id, markers_in, restore_markers, strip_markers, AssetRef, ChapterBlocks, ChapterKind,
};
pub use chunker::split_chapter;
pub use epub::{asset_file_name, extract_assets as extract_epub_assets};
pub use load::{detect_format, load_book, InputFormat, LoadedBook};
pub use parser::{
    build_delimiter_prompt, parse_chapters_with, parse_inferred_pattern, validate, BookMeta,
    Chapter,
};
pub use pdf::{
    cover as extract_pdf_cover, looks_like_text, set_pdfium_lib_path,
    toc_chapters as extract_pdf_toc_chapters,
};
pub use source::read_book_file;
