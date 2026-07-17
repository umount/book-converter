//! Экспорт в EPUB с оглавлением по главам (через crate `epub-builder`).

use super::TranslatedChapter;

/// Собрать книгу в .epub: каждая глава — отдельная секция + оглавление (TOC).
pub fn export(
    _chapters: &[TranslatedChapter],
    _title: &str,
    _author: &str,
    _out_path: &str,
) -> anyhow::Result<()> {
    todo!("сборка EPUB с оглавлением через epub-builder")
}
