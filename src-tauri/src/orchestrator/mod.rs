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

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use anyhow::{anyhow, Result};

use crate::book::{split_chapter, Chapter};
use crate::config::Config;
use crate::glossary::{self, Term};
use crate::state::{Stats, Status, Store};
use crate::textutil;
use crate::translator::prompt::ReplyShape;
use crate::translator::{prompt, repair, reply, DeepSeekClient};

/// Shortest run of unexpected-script letters treated as a leftover foreign word.
/// Single letters (an initial, a unit) are ignored; Han is always reported.
const MIN_FOREIGN_RUN: usize = 2;

/// How many repair passes a chapter may get before the leftovers are just logged.
const MAX_LANGUAGE_REPAIRS: usize = 2;

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
    /// Source forms whose glossary row changed during this run and has not been
    /// written yet. See [`Orchestrator::flush_glossary`].
    dirty_terms: HashSet<String>,
    style: Option<String>,
    summary: String,
    prev_tail: Option<String>,
    /// Title / author of the book, injected into every chapter prompt.
    book: BookIdentity,
}

/// Owned counterpart of `prompt::BookRef`, read once from the project's meta.
#[derive(Default)]
struct BookIdentity {
    title: Option<String>,
    author: Option<String>,
    title_translated: Option<String>,
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
        let meta = |k: &str| store.get_meta(k).ok().flatten().filter(|v| !v.trim().is_empty());
        Ok(Self {
            client,
            store,
            config,
            glossary,
            dirty_terms: HashSet::new(),
            style,
            summary,
            prev_tail: None,
            book: BookIdentity {
                title: meta("title"),
                author: meta("author"),
                title_translated: meta("title_translated"),
            },
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
    ///
    /// Terms are written after each chapter; the flush here is the safety net
    /// for a run that ended on a path which skipped one. See
    /// [`Orchestrator::flush_glossary`].
    pub async fn run<F: FnMut(ProgressEvent)>(
        &mut self,
        limit: Option<usize>,
        cancel: &AtomicBool,
        progress: F,
    ) -> Result<()> {
        let result = self.run_chapters(limit, cancel, progress).await;
        if let Err(e) = self.flush_glossary() {
            tracing::warn!("glossary flush failed: {e:#}");
        }
        result
    }

    async fn run_chapters<F: FnMut(ProgressEvent)>(
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
        progress: F,
    ) -> Result<()> {
        let result = self.run_one_chapter(index, cancel, progress).await;
        // A run of one is still a run: flush what it learned.
        if let Err(e) = self.flush_glossary() {
            tracing::warn!("glossary flush failed: {e:#}");
        }
        result
    }

    async fn run_one_chapter<F: FnMut(ProgressEvent)>(
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
            Ok((title_out, body)) => {
                let (t_title, t_body, lang_issues) = self
                    .enforce_target_language(idx, &title_out, &body, &source)
                    .await;
                let ms = started.elapsed().as_millis() as u64;
                self.store
                    .save_translation_timed(idx, &t_title, &t_body, Some(ms))?;
                // Flag (or clear) the chapter so leftovers are findable in the UI.
                let _ = self.store.set_language_issues(idx, &lang_issues);
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

    /// Translate one chapter's text, returning `(title, body)` as the model
    /// framed them. Long chapters go out as several chunks: the first carries
    /// the title, the rest continue the body.
    async fn translate_one(
        &self,
        title: &str,
        source: &str,
        user_note: Option<&str>,
    ) -> Result<(String, String)> {
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
            book: Some(prompt::BookRef {
                title: self.book.title.as_deref(),
                author: self.book.author.as_deref(),
                title_translated: self.book.title_translated.as_deref(),
            }),
        };
        let system = prompt::system_prompt(self.config);

        let mut out_title: Option<String> = None;
        let mut bodies: Vec<String> = Vec::with_capacity(chunks.len());
        for chunk in &chunks {
            // Only the opening chunk of a titled chapter is asked for a title.
            let carries_title = chunk.part == 0 && !title.trim().is_empty();
            let input = if carries_title {
                format!("{}\n\n{}", title.trim(), chunk.text)
            } else {
                chunk.text.clone()
            };
            let shape = if carries_title {
                ReplyShape::TitleAndBody
            } else {
                ReplyShape::BodyOnly
            };
            let user = prompt::user_prompt(&ctx, &input, shape);
            let raw = self.client.translate(&system, &user).await?;
            if carries_title {
                let (t, b) = reply::parse_chapter_reply(&raw, title);
                out_title = Some(t);
                bodies.push(b);
            } else {
                bodies.push(reply::parse_body_reply(&raw));
            }
        }
        Ok((
            out_title.unwrap_or_else(|| title.trim().to_string()),
            bodies.join("\n\n"),
        ))
    }

    /// Catch words the model left in the source language (or pulled in from a
    /// third one) and ask it to redo **just the lines they sit in**.
    ///
    /// The title is line 0 of that list, so a title that kept a foreign word is
    /// repaired in the same request as the body rather than needing its own.
    /// Retries while it keeps making progress, up to `MAX_LANGUAGE_REPAIRS`: in
    /// practice the first pass clears most of a chapter and the second finishes
    /// the stubborn genre jargon ("cultivation"). Whatever survives is logged
    /// with the chapter, so it can be found afterwards.
    ///
    /// Returns the repaired `(title, body)` and the fragments still present.
    async fn enforce_target_language(
        &self,
        idx: usize,
        title: &str,
        body: &str,
        source: &str,
    ) -> (String, String, Vec<String>) {
        let Some(expected) = textutil::expected_script(&self.config.target_lang) else {
            return (title.to_string(), body.to_string(), Vec::new());
        };

        // Line 0 is the title; the body follows. Splitting on '\n' rather than
        // `lines()` keeps blank lines, so paragraph structure survives the join.
        let mut lines: Vec<String> = std::iter::once(title.to_string())
            .chain(body.split('\n').map(|l| l.to_string()))
            .collect();

        let joined = |lines: &[String]| lines.join("\n");
        let mut bad = textutil::foreign_fragments(&joined(&lines), expected, source, MIN_FOREIGN_RUN);

        for attempt in 0..MAX_LANGUAGE_REPAIRS {
            if bad.is_empty() {
                break;
            }
            let targets = repair::lines_with_fragments(&lines, &bad);
            if targets.is_empty() {
                // The fragments live in whitespace-only or joined-away text; no
                // line to send, so another pass cannot help.
                break;
            }
            let numbered: Vec<repair::NumberedLine> = targets
                .iter()
                .map(|&n| repair::NumberedLine { n, text: lines[n].clone() })
                .collect();
            let sent_chars: usize = numbered.iter().map(|l| l.text.chars().count()).sum();
            tracing::info!(
                chapter = idx,
                attempt,
                fragments = ?bad,
                lines = numbered.len(),
                of_lines = lines.len(),
                chars = sent_chars,
                "translation kept foreign words; repairing the affected lines"
            );

            let mut changed = 0usize;
            for batch in repair::batches(&numbered, repair::MAX_BATCH_CHARS) {
                let (system, user) = repair::build_prompt(self.config, &bad, &batch);
                let raw = match self.client.translate_json(&system, &user).await {
                    Ok(raw) => raw,
                    Err(e) => {
                        tracing::warn!(chapter = idx, "language repair request failed: {e:#}");
                        break;
                    }
                };
                match repair::parse_reply(&raw) {
                    // Per-line validation lives in `splice`: a mangled reply can
                    // now only lose its own line, never the chapter.
                    Ok(fixed) => changed += repair::splice(&mut lines, &fixed),
                    Err(e) => tracing::warn!(chapter = idx, "unparseable repair reply: {e:#}"),
                }
            }
            if changed == 0 {
                // Nothing was accepted, so an identical request will not help.
                break;
            }

            let still = textutil::foreign_fragments(&joined(&lines), expected, source, MIN_FOREIGN_RUN);
            let progressed = still.len() < bad.len();
            bad = still;
            if !progressed {
                break;
            }
        }

        if !bad.is_empty() {
            tracing::warn!(chapter = idx, fragments = ?bad, "foreign words remain after repair");
        }

        let title = lines.remove(0);
        (title, lines.join("\n").trim().to_string(), bad)
    }

    async fn update_summary(&self, translation: &str) -> Result<String> {
        let (system, user) = prompt::build_summary_prompt(self.config, &self.summary, translation);
        self.client.translate(&system, &user).await
    }

    /// Extract this chapter's terms, fold them into the glossary and persist
    /// them, so the **next** chapter's prompt already carries what this one
    /// taught. That is the whole point of growing a glossary mid-run: a name
    /// first seen in chapter 40 must be fixed by the time chapter 41 is
    /// translated, and it must be visible in the UI right away, not at the end
    /// of a run that may be hundreds of chapters long.
    ///
    /// What changed is that only the terms this chapter touched are written,
    /// instead of the entire list.
    async fn enrich_glossary(&mut self, source: &str, translation: &str) -> Result<()> {
        let new_terms =
            crate::translator::extract_terms(self.client, self.config, source, translation, 2)
                .await?;
        for t in &new_terms {
            self.dirty_terms.insert(t.source.clone());
        }
        glossary::merge(&mut self.glossary, new_terms);
        self.flush_glossary()?;
        Ok(())
    }

    /// Write the terms learned since the last flush, and clear the dirty set.
    ///
    /// Runs after every chapter, and again when a run ends however it ends
    /// (finished, paused, cancelled, failed) so terms extracted on a path that
    /// skipped the per-chapter flush are not lost. Only the rows that actually
    /// changed are written, so the cost is proportional to what was learned
    /// rather than to the size of the glossary.
    fn flush_glossary(&mut self) -> Result<usize> {
        if self.dirty_terms.is_empty() {
            return Ok(0);
        }
        // The in-memory glossary was read when the run started and can be hours
        // old. A term the user edited in the UI meanwhile must keep the
        // rendering they chose; only the frequency this run counted is ours to
        // write. New terms are inserted as extracted.
        let mut rows: Vec<Term> = Vec::with_capacity(self.dirty_terms.len());
        for term in self
            .glossary
            .iter()
            .filter(|t| self.dirty_terms.contains(&t.source))
        {
            match self.store.term(&term.source)? {
                Some(stored) => rows.push(Term {
                    frequency: term.frequency.max(stored.frequency),
                    ..stored
                }),
                None => rows.push(term.clone()),
            }
        }
        let refs: Vec<&Term> = rows.iter().collect();
        self.store.upsert_terms(&refs)?;
        self.dirty_terms.clear();
        tracing::info!(terms = rows.len(), "glossary flushed at end of run");
        Ok(rows.len())
    }

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

}
