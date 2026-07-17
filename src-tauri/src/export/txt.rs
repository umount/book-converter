//! Export to a single .txt: chapters in order, separated by titles.

use super::TranslatedChapter;

/// Assemble the book into a single .txt file.
pub fn export(_chapters: &[TranslatedChapter], _out_path: &str) -> anyhow::Result<()> {
    todo!("concatenate chapters into a single .txt with titles")
}
