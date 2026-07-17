//! Sequential translation orchestrator.
//!
//! Walks `pending_chapters` in order and, for each chapter:
//! 1. injects the glossary terms present in it + the rolling context (running
//!    summary + previous chapter tail) + the style exemplar;
//! 2. translates via DeepSeek and saves the result;
//! 3. updates the running summary (light call) and persists it;
//! 4. extracts new terms and merges them into the glossary.
//!
//! Running in order is what lets chapter N+1 see the summary produced by chapter N,
//! so narrative meaning is preserved (see `docs/DECISIONS.md`). State (glossary,
//! running summary, per-chapter status) lives in SQLite, so a run resumes with
//! full context.

use anyhow::{anyhow, Result};

use crate::config::Config;
use crate::glossary::{self, Term};
use crate::state::{Stats, Status, Store};
use crate::translator::{prompt, DeepSeekClient};

pub struct Orchestrator<'a> {
    client: &'a DeepSeekClient,
    store: &'a Store,
    config: &'a Config,
    glossary: Vec<Term>,
    style: Option<String>,
    summary: String,
    prev_tail: Option<String>,
}

impl<'a> Orchestrator<'a> {
    /// Build from the store's current state. `style` is an optional exemplar from a
    /// reference translation (`reference::style_exemplar`).
    pub fn new(
        client: &'a DeepSeekClient,
        store: &'a Store,
        config: &'a Config,
        style: Option<String>,
    ) -> Result<Self> {
        let glossary = store.load_glossary()?;
        let summary = store.get_meta("running_summary")?.unwrap_or_default();
        Ok(Self {
            client,
            store,
            config,
            glossary,
            style,
            summary,
            prev_tail: None,
        })
    }

    /// Translate every pending chapter, in order. `progress` is called after each.
    pub async fn run<F: FnMut(Stats)>(&mut self, mut progress: F) -> Result<()> {
        let pending = self.store.pending_chapters()?;
        for idx in pending {
            self.store.set_status(idx, Status::InProgress)?;
            let source = self
                .store
                .chapter_source(idx)?
                .ok_or_else(|| anyhow!("no source for chapter {idx}"))?;

            match self.translate_one(&source).await {
                Ok(translation) => {
                    self.store.save_translation(idx, &translation)?;
                    self.prev_tail = Some(tail(&translation, 400));

                    // Rolling summary (best-effort — a failure here is non-fatal).
                    match self.update_summary(&translation).await {
                        Ok(sum) => {
                            self.summary = sum;
                            let _ = self.store.set_meta("running_summary", &self.summary);
                        }
                        Err(e) => tracing::warn!(chapter = idx, "summary update failed: {e:#}"),
                    }

                    // Glossary enrichment (best-effort).
                    if let Err(e) = self.enrich_glossary(&source, &translation).await {
                        tracing::warn!(chapter = idx, "glossary enrichment failed: {e:#}");
                    }
                }
                Err(e) => {
                    tracing::error!(chapter = idx, "translation failed: {e:#}");
                    self.store.set_status(idx, Status::Failed)?;
                }
            }

            progress(self.store.stats()?);
        }
        Ok(())
    }

    /// Translate one chapter's source with the current context.
    async fn translate_one(&self, source: &str) -> Result<String> {
        let relevant = glossary::relevant_terms(&self.glossary, source);
        let ctx = prompt::PromptContext {
            terms: &relevant,
            summary: non_empty(&self.summary),
            prev_tail: self.prev_tail.as_deref(),
            style: self.style.as_deref(),
        };
        let system = prompt::system_prompt(self.config);
        let user = prompt::user_prompt(&ctx, source);
        self.client.translate(&system, &user).await
    }

    async fn update_summary(&self, translation: &str) -> Result<String> {
        let (system, user) = prompt::build_summary_prompt(self.config, &self.summary, translation);
        self.client.translate(&system, &user).await
    }

    async fn enrich_glossary(&mut self, source: &str, translation: &str) -> Result<()> {
        let (system, user) = glossary::build_extraction_prompt(source, translation);
        let raw = self.client.translate(&system, &user).await?;
        let new_terms = glossary::parse_extracted_terms(&raw)?;
        glossary::merge(&mut self.glossary, new_terms);
        self.store.save_glossary(&self.glossary)?;
        Ok(())
    }

    /// Current in-memory glossary (e.g. for inspection/UI).
    pub fn glossary(&self) -> &[Term] {
        &self.glossary
    }
}

/// Last `n` characters of a string (char-safe).
fn tail(s: &str, n: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    let start = chars.len().saturating_sub(n);
    chars[start..].iter().collect()
}

fn non_empty(s: &str) -> Option<&str> {
    if s.trim().is_empty() {
        None
    } else {
        Some(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tail_is_char_safe_and_bounded() {
        assert_eq!(tail("abcdef", 3), "def");
        assert_eq!(tail("ab", 5), "ab");
        // multibyte: last 2 of 3 CJK chars
        assert_eq!(tail("王林城", 2), "林城");
    }

    #[test]
    fn non_empty_filters_blank() {
        assert_eq!(non_empty("  "), None);
        assert_eq!(non_empty("x"), Some("x"));
    }
}
