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
use std::time::Instant;

use anyhow::{anyhow, Result};

use crate::book::{split_chapter, Chapter};
use crate::config::Config;
use crate::glossary::{self, Term};
use crate::state::{Stats, Status, Store};
use crate::translator::{prompt, DeepSeekClient};

/// Live progress callback payload for the UI / console.
#[derive(Debug, Clone)]
pub struct ProgressEvent {
    pub stats: Stats,
    pub job_done: usize,
    pub job_total: usize,
    pub current_idx: Option<usize>,
    /// Book chapter number from the title (`第N章`), when known.
    pub current_number: Option<usize>,
    pub current_title: Option<String>,
    /// First still-pending chapter's book number (for resume hints).
    pub next_number: Option<usize>,
    /// `start` | `chapter_start` | `chapter_done`
    pub phase: &'static str,
    pub last_ms: Option<u64>,
    pub eta_secs: Option<u64>,
}

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

    fn eta_secs(&self, remaining: usize, run_avg_ms: Option<u64>) -> Option<u64> {
        if remaining == 0 {
            return Some(0);
        }
        let avg = run_avg_ms
            .or_else(|| self.store.avg_translate_ms(30).ok().flatten())?;
        Some(((avg as u128) * (remaining as u128) / 1000) as u64)
    }

    /// Translate pending chapters in order, at most `limit` of them (`None` = all).
    pub async fn run<F: FnMut(ProgressEvent)>(
        &mut self,
        limit: Option<usize>,
        cancel: &AtomicBool,
        mut progress: F,
    ) -> Result<()> {
        let pending = self.store.pending_chapters()?;
        let queue: Vec<usize> = pending
            .into_iter()
            .take(limit.unwrap_or(usize::MAX))
            .collect();
        let job_total = queue.len();
        let mut job_done = 0usize;
        let mut run_sum_ms: u64 = 0;
        let mut run_n: usize = 0;

        progress(ProgressEvent {
            stats: self.store.stats()?,
            job_done,
            job_total,
            current_idx: None,
            current_number: None,
            current_title: None,
            next_number: self.next_pending_number()?,
            phase: "start",
            last_ms: None,
            eta_secs: self.eta_secs(job_total, None),
        });

        let mut first = true;
        for idx in queue {
            if cancel.load(Ordering::Relaxed) {
                break;
            }
            if first {
                self.hydrate_before(idx)?;
                first = false;
            }

            let (title, _) = self
                .store
                .chapter(idx)?
                .ok_or_else(|| anyhow!("no source for chapter {idx}"))?;
            let number = self.store.chapter_number(idx)?;

            progress(ProgressEvent {
                stats: self.store.stats()?,
                job_done,
                job_total,
                current_idx: Some(idx),
                current_number: number,
                current_title: Some(title.clone()),
                next_number: number.or(self.next_pending_number()?),
                phase: "chapter_start",
                last_ms: None,
                eta_secs: self.eta_secs(
                    job_total.saturating_sub(job_done),
                    if run_n > 0 {
                        Some(run_sum_ms / run_n as u64)
                    } else {
                        None
                    },
                ),
            });

            let started = Instant::now();
            self.translate_chapter(idx).await?;
            let last_ms = started.elapsed().as_millis() as u64;
            // Persist timing even if translate_chapter already saved the text
            // (it records ms itself on success; this is a fallback for failed paths).
            let _ = last_ms;

            let status_ok = self
                .store
                .chapter_full(idx)?
                .map(|r| r.3 == "done")
                .unwrap_or(false);
            if status_ok {
                job_done += 1;
                run_sum_ms += last_ms;
                run_n += 1;
            }

            progress(ProgressEvent {
                stats: self.store.stats()?,
                job_done,
                job_total,
                current_idx: Some(idx),
                current_number: number,
                current_title: Some(title),
                next_number: self.next_pending_number()?,
                phase: "chapter_done",
                last_ms: Some(last_ms),
                eta_secs: self.eta_secs(
                    job_total.saturating_sub(job_done),
                    if run_n > 0 {
                        Some(run_sum_ms / run_n as u64)
                    } else {
                        None
                    },
                ),
            });
        }
        Ok(())
    }

    /// Translate a single chapter (by index), using the previous chapter's saved
    /// rolling context. Used by the reader "Translate this chapter" action.
    pub async fn run_one<F: FnMut(ProgressEvent)>(
        &mut self,
        index: usize,
        cancel: &AtomicBool,
        mut progress: F,
    ) -> Result<()> {
        if cancel.load(Ordering::Relaxed) {
            return Ok(());
        }
        self.hydrate_before(index)?;
        let (title, _) = self
            .store
            .chapter(index)?
            .ok_or_else(|| anyhow!("no source for chapter {index}"))?;

        let number = self.store.chapter_number(index)?;
        progress(ProgressEvent {
            stats: self.store.stats()?,
            job_done: 0,
            job_total: 1,
            current_idx: Some(index),
            current_number: number,
            current_title: Some(title.clone()),
            next_number: number,
            phase: "chapter_start",
            last_ms: None,
            eta_secs: self.eta_secs(1, None),
        });

        let started = Instant::now();
        self.translate_chapter(index).await?;
        let last_ms = started.elapsed().as_millis() as u64;

        progress(ProgressEvent {
            stats: self.store.stats()?,
            job_done: 1,
            job_total: 1,
            current_idx: Some(index),
            current_number: number,
            current_title: Some(title),
            next_number: self.next_pending_number()?,
            phase: "chapter_done",
            last_ms: Some(last_ms),
            eta_secs: Some(0),
        });
        Ok(())
    }

    fn next_pending_number(&self) -> Result<Option<usize>> {
        Ok(self.store.next_pending()?.map(|(idx, number)| number.unwrap_or(idx)))
    }

    async fn translate_chapter(&mut self, idx: usize) -> Result<()> {
        self.store.set_status(idx, Status::InProgress)?;
        let (title, source) = self
            .store
            .chapter(idx)?
            .ok_or_else(|| anyhow!("no source for chapter {idx}"))?;
        let user_note = self.store.chapter_user_prompt(idx)?;

        let started = Instant::now();
        match self.translate_one(&title, &source, user_note.as_deref()).await {
            Ok(full) => {
                let (t_title, t_body) = split_title_body(&full, &title);
                let ms = started.elapsed().as_millis() as u64;
                self.store
                    .save_translation_timed(idx, &t_title, &t_body, Some(ms))?;
                let chapter_tail = crate::textutil::closing_excerpt(&t_body, 400);
                self.prev_tail = Some(chapter_tail.clone());

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

    async fn translate_one(
        &self,
        title: &str,
        source: &str,
        user_note: Option<&str>,
    ) -> Result<String> {
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
            user_note,
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
