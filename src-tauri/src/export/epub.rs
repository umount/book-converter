//! Export to EPUB with a per-chapter table of contents (via the `epub-builder` crate).

use super::TranslatedChapter;

/// Assemble the book into an .epub: each chapter is its own section + a TOC.
pub fn export(
    _chapters: &[TranslatedChapter],
    _title: &str,
    _author: &str,
    _out_path: &str,
) -> anyhow::Result<()> {
    todo!("build the EPUB with a table of contents via epub-builder")
}
