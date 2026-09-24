//! Working with the source book: decoding, format detection, parsing, chunking.

pub(crate) mod blocks;
pub(crate) mod epub;
pub(crate) mod fb2;
mod fb2_content;
pub(crate) mod load;
pub(crate) mod parser;
pub(crate) mod pdf;
pub(crate) mod source;

pub use load::load_book;
pub use parser::Chapter;
pub use pdf::set_pdfium_lib_path;
pub use source::read_book_file;
