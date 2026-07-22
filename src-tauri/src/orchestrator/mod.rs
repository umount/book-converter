//! Sequential translation orchestrator.
//!
//! Walks `pending_chapters` in order and, for each chapter:
//! 1. injects the glossary terms present in it + the rolling context (running
//!    summary + previous chapter tail) + the style exemplar;
//! 2. translates via DeepSeek and saves the result;
//! 3. updates the running summary (light call) and persists it **per chapter**
//!    plus the book-level meta key;
//! 4. extracts new terms and merges them into the glossary.
//!
//! Running in order is what lets chapter N+1 see the summary produced by chapter N,
//! so narrative meaning is preserved (see `docs/DECISIONS.md`). State (glossary,
//! running summary, per-chapter status) lives in SQLite, so a run resumes with
//! full context.

use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{anyhow, Result};

use crate::book::{split_chapter, Chapter};
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

    /// Load continuity context for translating `index` (previous chapter's saved
    /// rolling summary + prev_tail, with meta fallback).
    fn hydrate_before(&mut self, index: usize) -> Result<()> {
        let (summary, prev_tail) = self.store.context_before(index)?;
        self.summary = summary;
        self.prev_tail = prev_tail;
        Ok(())
    }

    /// Translate pending chapters in order, at most `limit` of them (`None` = all).
    /// `progress` is called after each. A `limit` supports "translate the next N".
    pub async fn run<F: FnMut(Stats)>(
        &mut self,
        limit: Option<usize>,
        cancel: &AtomicBool,
        mut progress: F,
    ) -> Result<()> {
        let pending = self.store.pending_chapters()?;
        let take = limit.unwrap_or(usize::MAX);
        let mut first = true;
        for idx in pending.into_iter().take(take) {
            if cancel.load(Ordering::Relaxed) {
                break;
            }
            // On resume / first chapter of this job, restore context from SQLite
            // (prev_tail was not kept in memory across restarts). Subsequent
            // chapters in the same run already carry forward in-memory state.
            if first {
                self.hydrate_before(idx)?;
                first = false;
            }
            self.translate_chapter(idx).await?;
            progress(self.store.stats()?);
        }
        Ok(())
    }

    /// Translate a single chapter (by index), using the previous chapter's saved
    /// rolling context. Used by the reader "Translate this chapter" action.
    pub async fn run_one<F: FnMut(Stats)>(
        &mut self,
        index: usize,
        cancel: &AtomicBool,
        mut progress: F,
    ) -> Result<()> {
        if cancel.load(Ordering::Relaxed) {
            return Ok(());
        }
        self.hydrate_before(index)?;
        self.translate_chapter(index).await?;
        progress(self.store.stats()?);
        Ok(())
    }

    async fn translate_chapter(&mut self, idx: usize) -> Result<()> {
        self.store.set_status(idx, Status::InProgress)?;
        let (title, source) = self
            .store
            .chapter(idx)?
            .ok_or_else(|| anyhow!("no source for chapter {idx}"))?;

        match self.translate_one(&title, &source).await {
            Ok(full) => {
                let (t_title, t_body) = split_title_body(&full, &title);
                self.store.save_translation(idx, &t_title, &t_body)?;
                let chapter_tail = tail(&t_body, 400);
                self.prev_tail = Some(chapter_tail.clone());

                // Rolling summary (best-effort — a failure here is non-fatal).
                match self.update_summary(&t_body).await {
                    Ok(sum) => {
                        self.summary = sum;
                        let _ = self.store.set_meta("running_summary", &self.summary);
                        let _ = self.store.save_chapter_context(
                            idx,
                            &self.summary,
                            &chapter_tail,
                        );
                    }
                    Err(e) => {
                        tracing::warn!(chapter = idx, "summary update failed: {e:#}");
                        // Still persist the prev_tail so the next chapter has continuity.
                        let _ = self.store.save_chapter_context(
                            idx,
                            &self.summary,
                            &chapter_tail,
                        );
                    }
                }

                if let Err(e) = self.enrich_glossary(&source, &t_body).await {
                    tracing::warn!(chapter = idx, "glossary enrichment failed: {e:#}");
                }
            }
            Err(e) => {
                tracing::error!(chapter = idx, "translation failed: {e:#}");
                self.store.set_status(idx, Status::Failed)?;
            }
        }
        Ok(())
    }

    /// Translate one chapter (title + body) with the current context.
    ///
    /// Abnormally long chapters are split on paragraph boundaries
    /// (`max_chunk_chars`); the title is prepended only to the first chunk, and
    /// chunk translations are joined with blank lines.
    async fn translate_one(&self, title: &str, source: &str) -> Result<String> {
        let chapter = Chapter {
            index: 0,
            number: None,
            title: title.to_string(),
            body: source.to_string(),
        };
        let chunks = split_chapter(&chapter, self.config.max_chunk_chars);
        let relevant = glossary::relevant_terms(&self.glossary, source);
        let ctx = prompt::PromptContext {
            terms: &relevant,
            summary: non_empty(&self.summary),
            prev_tail: self.prev_tail.as_deref(),
            style: self.style.as_deref(),
        };
        let system = prompt::system_prompt(self.config);

        let mut parts: Vec<String> = Vec::with_capacity(chunks.len());
        for chunk in &chunks {
            let input = if chunk.part == 0 && !title.trim().is_empty() {
                format!("{}\n\n{}", title.trim(), chunk.text)
            } else {
                chunk.text.clone()
            };
            let user = prompt::user_prompt(&ctx, &input);
            let out = self.client.translate(&system, &user).await?;
            parts.push(out);
        }
        Ok(parts.join("\n\n"))
    }

    async fn update_summary(&self, translation: &str) -> Result<String> {
        let (system, user) = prompt::build_summary_prompt(self.config, &self.summary, translation);
        self.client.translate(&system, &user).await
    }

    async fn enrich_glossary(&mut self, source: &str, translation: &str) -> Result<()> {
        let new_terms = crate::translator::extract_terms(self.client, source, translation, 2).await?;
        glossary::merge(&mut self.glossary, new_terms);
        self.store.save_glossary(&self.glossary)?;
        Ok(())
    }

    /// Current in-memory glossary (e.g. for inspection/UI).
    pub fn glossary(&self) -> &[Term] {
        &self.glossary
    }
}

fn split_title_body(full: &str, source_title: &str) -> (String, String) {
    if source_title.trim().is_empty() {
        return (String::new(), full.trim().to_string());
    }
    let trimmed = full.trim_start();
    match trimmed.split_once('\n') {
        Some((first, rest)) if !rest.trim().is_empty() => {
            (clean_title(first), rest.trim().to_string())
        }
        _ => (source_title.trim().to_string(), trimmed.trim().to_string()),
    }
}

fn clean_title(line: &str) -> String {
    line.trim()
        .trim_start_matches(|c: char| c == '#' || c == '*' || c.is_whitespace())
        .trim()
        .to_string()
}

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
        assert_eq!(tail("王林城", 2), "林城");
    }

    #[test]
    fn non_empty_filters_blank() {
        assert_eq!(non_empty("  "), None);
        assert_eq!(non_empty("x"), Some("x"));
    }

    #[test]
    fn split_title_body_variants() {
        let (t, b) = split_title_body("Глава 1\n\nТекст главы.", "第1章");
        assert_eq!(t, "Глава 1");
        assert_eq!(b, "Текст главы.");
        let (t, _) = split_title_body("### Глава 3\n\nтекст", "第3章");
        assert_eq!(t, "Глава 3");
        let (t, b) = split_title_body("Просто текст.", "");
        assert_eq!(t, "");
        assert_eq!(b, "Просто текст.");
        let (t, b) = split_title_body("Одна строка", "第2章");
        assert_eq!(t, "第2章");
        assert_eq!(b, "Одна строка");
    }
}
