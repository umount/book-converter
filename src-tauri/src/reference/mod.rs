//! Reference-translation subsystem.
//!
//! When the user supplies a professional translation of the same book, it is a
//! high-quality source of **names, lore, and style**. We:
//! 1. load it through the normal format layer (`book::load`);
//! 2. align its chapters to the source (by chapter number, falling back to
//!    reading order) — general, not tuned to one book;
//! 3. bootstrap a **pinned** glossary from a sample of aligned pairs by extracting
//!    `source → professional rendering` term pairs via DeepSeek;
//! 4. expose a style exemplar (a professional excerpt) to inject into prompts.
//!
//! All chapters are still machine-translated (uniform style); the reference is a
//! knowledge source, not the output. Where no reference is supplied, the pipeline
//! runs on the auto-grown glossary alone.

use std::collections::HashMap;
use std::path::Path;

use anyhow::Result;

use crate::book::{load_book, BookMeta, Chapter};
use crate::export::TranslatedChapter;
use crate::glossary::{self, Term};
use crate::translator::DeepSeekClient;

/// A loaded reference translation.
#[derive(Debug, Clone)]
pub struct Reference {
    pub meta: BookMeta,
    pub chapters: Vec<Chapter>,
}

/// Load a reference translation from a file (any supported format).
pub fn load_reference(path: &Path) -> Result<Reference> {
    let book = load_book(path)?;
    Ok(Reference {
        meta: book.meta,
        chapters: book.chapters,
    })
}

/// Pair source chapters with reference chapters, up to `sample` pairs.
///
/// Prefers matching by chapter number (works across editions/languages); falls
/// back to reading order when numbers are absent on either side.
pub fn align<'a>(
    source: &'a [Chapter],
    reference: &'a [Chapter],
    sample: usize,
) -> Vec<(&'a Chapter, &'a Chapter)> {
    let ref_by_number: HashMap<usize, &Chapter> = reference
        .iter()
        .filter_map(|c| c.number.map(|n| (n, c)))
        .collect();

    let mut pairs: Vec<(&Chapter, &Chapter)> = Vec::new();

    if !ref_by_number.is_empty() {
        for s in source {
            if pairs.len() >= sample {
                break;
            }
            if let Some(n) = s.number {
                if let Some(&r) = ref_by_number.get(&n) {
                    pairs.push((s, r));
                }
            }
        }
    }

    // Positional fallback (e.g. neither side numbered).
    if pairs.is_empty() {
        for (s, r) in source.iter().zip(reference.iter()).take(sample) {
            pairs.push((s, r));
        }
    }

    pairs
}

/// Highest chapter number the reference translation covers.
pub fn max_covered_number(reference: &Reference) -> Option<usize> {
    reference.chapters.iter().filter_map(|c| c.number).max()
}

/// Source chapters to translate in order to **continue** past the reference,
/// i.e. those numbered beyond what the reference covers, up to `limit`.
pub fn continue_from<'a>(
    source: &'a [Chapter],
    reference: &Reference,
    limit: usize,
) -> Vec<&'a Chapter> {
    let after = max_covered_number(reference).unwrap_or(0);
    let mut chosen: Vec<&Chapter> = source
        .iter()
        .filter(|c| c.number.map_or(false, |n| n > after))
        .collect();
    chosen.sort_by_key(|c| c.number.unwrap_or(usize::MAX));
    chosen.truncate(limit);
    chosen
}

/// The reference's chapters as export-ready `TranslatedChapter`s — the existing
/// (human) translation to prepend when continuing a book.
pub fn as_translated(reference: &Reference) -> Vec<TranslatedChapter> {
    reference
        .chapters
        .iter()
        .map(|c| TranslatedChapter {
            index: c.index,
            number: c.number,
            title: c.title.clone(),
            body: c.body.clone(),
        })
        .collect()
}

/// A short professional excerpt to use as a few-shot style reference in prompts.
pub fn style_exemplar(reference: &Reference, max_chars: usize) -> Option<String> {
    reference
        .chapters
        .iter()
        .find(|c| c.body.chars().count() > 200)
        .map(|c| c.body.chars().take(max_chars).collect())
}

/// Bootstrap a **pinned** glossary from aligned source↔reference chapter pairs.
///
/// For each pair, DeepSeek extracts `source → professional rendering` named
/// entities; those become pinned canon (human-quality, never overwritten by
/// later auto-extraction). Returns the merged glossary.
pub async fn bootstrap_glossary(
    client: &DeepSeekClient,
    source: &[Chapter],
    reference: &Reference,
    sample: usize,
) -> Result<Vec<Term>> {
    let pairs = align(source, &reference.chapters, sample);
    let mut merged: Vec<Term> = Vec::new();

    // Best-effort per chapter: retries on bad JSON (see `translator::extract_terms`);
    // if a chapter still fails it is skipped, not fatal — the rest yields canon.
    for (src, refc) in pairs {
        match crate::translator::extract_terms(client, &src.body, &refc.body, 2).await {
            Ok(mut terms) => {
                for t in &mut terms {
                    t.pinned = true; // reference-derived canon
                }
                glossary::merge(&mut merged, terms);
                tracing::info!(chapter = src.index, terms = merged.len(), "bootstrapped from reference");
            }
            Err(e) => tracing::warn!(chapter = src.index, "bootstrap extraction failed: {e:#}"),
        }
    }

    Ok(merged)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chapter(index: usize, number: Option<usize>, body: &str) -> Chapter {
        Chapter {
            index,
            number,
            title: format!("ch{index}"),
            body: body.into(),
        }
    }

    #[test]
    fn aligns_by_number() {
        let source = vec![
            chapter(1, Some(1), "s1"),
            chapter(2, Some(2), "s2"),
            chapter(3, Some(3), "s3"),
        ];
        // reference numbered, but only 1 and 2 present, out of order
        let reference = vec![chapter(1, Some(2), "r2"), chapter(2, Some(1), "r1")];
        let pairs = align(&source, &reference, 10);
        assert_eq!(pairs.len(), 2);
        assert_eq!(pairs[0].0.body, "s1");
        assert_eq!(pairs[0].1.body, "r1"); // matched by number, not position
        assert_eq!(pairs[1].0.body, "s2");
        assert_eq!(pairs[1].1.body, "r2");
    }

    #[test]
    fn align_respects_sample_limit() {
        let source: Vec<Chapter> = (1..=50).map(|i| chapter(i, Some(i), "s")).collect();
        let reference: Vec<Chapter> = (1..=50).map(|i| chapter(i, Some(i), "r")).collect();
        assert_eq!(align(&source, &reference, 30).len(), 30);
    }

    #[test]
    fn continue_from_picks_chapters_past_coverage() {
        let source: Vec<Chapter> = (1..=10).map(|i| chapter(i, Some(i), "s")).collect();
        // reference covers 1..=4
        let reference = Reference {
            meta: BookMeta::default(),
            chapters: (1..=4).map(|i| chapter(i, Some(i), "r")).collect(),
        };
        assert_eq!(max_covered_number(&reference), Some(4));
        let next = continue_from(&source, &reference, 3);
        assert_eq!(
            next.iter().map(|c| c.number.unwrap()).collect::<Vec<_>>(),
            vec![5, 6, 7]
        );
    }

    #[test]
    fn positional_fallback_when_unnumbered() {
        let source = vec![chapter(1, None, "s1"), chapter(2, None, "s2")];
        let reference = vec![chapter(1, None, "r1"), chapter(2, None, "r2")];
        let pairs = align(&source, &reference, 10);
        assert_eq!(pairs.len(), 2);
        assert_eq!(pairs[0].1.body, "r1");
    }
}
