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

use std::path::Path;

use anyhow::Result;

use crate::book::{detect_format, load_book_text, read_book_file, BookMeta, Chapter, InputFormat};
use crate::export::fb2::{extract_head, Fb2Head};
use crate::config::Config;
use crate::glossary::{self, Term};
use crate::translator::Translate;

/// A loaded reference translation.
#[derive(Debug, Clone)]
pub struct Reference {
    pub meta: BookMeta,
    pub chapters: Vec<Chapter>,
    /// Original FB2 head (description + cover), preserved on continuation export.
    pub head: Option<Fb2Head>,
}

/// Load a reference translation from a file (any supported format).
pub fn load_reference(path: &Path) -> Result<Reference> {
    let decoded = read_book_file(path)?;
    let book = load_book_text(&decoded.text, decoded.encoding)?;
    let head = if detect_format(&decoded.text) == InputFormat::Fb2 {
        Some(extract_head(&decoded.text))
    } else {
        None
    };
    Ok(Reference {
        meta: book.meta,
        chapters: book.chapters,
        head,
    })
}

/// Highest chapter number the reference translation covers.
pub fn max_covered_number(reference: &Reference) -> Option<usize> {
    reference.chapters.iter().filter_map(|c| c.number).max()
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
/// `pairs` are `(chapter index, source text, professional translation)`, already
/// aligned. Alignment happens once, when the reference is imported into the
/// project database; this no longer re-reads either book to redo it.
pub async fn bootstrap_glossary<C: Translate>(
    client: &C,
    config: &Config,
    pairs: &[(usize, String, String)],
) -> Result<Vec<Term>> {
    let mut merged: Vec<Term> = Vec::new();

    // Best-effort per chapter: retries on bad JSON (see `translator::extract_terms`);
    // if a chapter still fails it is skipped, not fatal — the rest yields canon.
    for (index, source, translated) in pairs {
        match crate::translator::extract_terms(client, config, source, translated, 2).await {
            Ok(mut terms) => {
                for t in &mut terms {
                    t.pinned = true; // reference-derived canon
                }
                glossary::merge(&mut merged, terms);
                tracing::info!(chapter = index, terms = merged.len(), "bootstrapped from reference");
            }
            Err(e) => tracing::warn!(chapter = index, "bootstrap extraction failed: {e:#}"),
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
    fn max_covered_is_highest_reference_number() {
        let reference = Reference {
            meta: BookMeta::default(),
            chapters: (1..=4).map(|i| chapter(i, Some(i), "r")).collect(),
            head: None,
        };
        assert_eq!(max_covered_number(&reference), Some(4));
    }

}
