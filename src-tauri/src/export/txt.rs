//! Экспорт в единый .txt: главы по порядку, разделённые заголовками.

use super::TranslatedChapter;

/// Собрать книгу в один .txt-файл.
pub fn export(_chapters: &[TranslatedChapter], _out_path: &str) -> anyhow::Result<()> {
    todo!("склейка глав в один .txt с заголовками")
}
