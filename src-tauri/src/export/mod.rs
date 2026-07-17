//! Exporting the translated book to a chosen format (TXT, FB2; EPUB planned).

pub mod epub;
pub mod fb2;
pub mod txt;

use std::path::Path;

use anyhow::Result;

/// A translated chapter ready for export.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranslatedChapter {
    /// Sequential position (for ordering when `number` is absent).
    pub index: usize,
    /// Chapter number, used to order/merge across sources (e.g. continuation).
    pub number: Option<usize>,
    pub title: String,
    pub body: String,
}

impl TranslatedChapter {
    /// Build from `state::Store::translated_chapters` rows `(index, title, body)`.
    pub fn from_rows(rows: Vec<(usize, String, String)>) -> Vec<Self> {
        rows.into_iter()
            .map(|(index, title, body)| Self { index, number: None, title, body })
            .collect()
    }

}

/// Merge an existing translation with newly translated chapters (the "continue
/// translation" case). Numbered chapters are ordered by number; a `new` chapter
/// overrides an `existing` one with the same number. Unnumbered chapters keep
/// their order at the end.
pub fn combine(
    existing: Vec<TranslatedChapter>,
    new: Vec<TranslatedChapter>,
) -> Vec<TranslatedChapter> {
    use std::collections::BTreeMap;

    let mut by_number: BTreeMap<usize, TranslatedChapter> = BTreeMap::new();
    let mut unnumbered: Vec<TranslatedChapter> = Vec::new();

    for ch in existing.into_iter().chain(new) {
        match ch.number {
            Some(n) => {
                by_number.insert(n, ch);
            }
            None => unnumbered.push(ch),
        }
    }

    let mut result: Vec<TranslatedChapter> = by_number.into_values().collect();
    result.extend(unnumbered);
    result
}

/// Output format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    Txt,
    Fb2,
}

impl OutputFormat {
    /// Infer the format from a file extension.
    pub fn from_path(path: &Path) -> Option<Self> {
        match path
            .extension()
            .and_then(|e| e.to_str())
            .map(|s| s.to_ascii_lowercase())
            .as_deref()
        {
            Some("txt") => Some(Self::Txt),
            Some("fb2") => Some(Self::Fb2),
            _ => None,
        }
    }
}

/// Book metadata used when writing an output file.
#[derive(Debug, Clone)]
pub struct OutputMeta {
    pub title: String,
    pub author: String,
    /// Language tag for FB2 (`<lang>`), e.g. "ru".
    pub lang: String,
}

impl Default for OutputMeta {
    fn default() -> Self {
        Self {
            title: "Untitled".into(),
            author: "Unknown".into(),
            lang: "ru".into(),
        }
    }
}

/// Write `chapters` to `out_path` in the given format.
pub fn export(
    chapters: &[TranslatedChapter],
    format: OutputFormat,
    meta: &OutputMeta,
    out_path: &Path,
) -> Result<()> {
    match format {
        OutputFormat::Txt => txt::export(chapters, out_path),
        OutputFormat::Fb2 => fb2::export(chapters, meta, out_path),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ch(number: usize, body: &str) -> TranslatedChapter {
        TranslatedChapter {
            index: number,
            number: Some(number),
            title: format!("Гл {number}"),
            body: body.into(),
        }
    }

    #[test]
    fn combine_appends_and_orders() {
        let existing = vec![ch(1, "pro1"), ch(2, "pro2")];
        let new = vec![ch(3, "new3"), ch(4, "new4")];
        let merged = combine(existing, new);
        assert_eq!(
            merged.iter().map(|c| c.number.unwrap()).collect::<Vec<_>>(),
            vec![1, 2, 3, 4]
        );
        assert_eq!(merged[2].body, "new3");
    }

    #[test]
    fn combine_new_overrides_same_number() {
        let existing = vec![ch(1, "old")];
        let new = vec![ch(1, "fresh")];
        let merged = combine(existing, new);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].body, "fresh");
    }
}
