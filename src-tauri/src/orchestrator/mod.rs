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

mod glossary;
mod language;

use self::glossary::GlossarySession;
use self::language::LanguageRepairer;
use crate::book::{split_chapter, Chapter};
use crate::config::Config;
use crate::state::{Stats, Status, Store};
use crate::translator::prompt::ReplyShape;
use crate::translator::{prompt, reply, Translate};

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

pub struct Orchestrator<'a, C: Translate> {
    client: &'a C,
    store: &'a Store,
    config: &'a Config,
    glossary: GlossarySession,
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

impl<'a, C: Translate> Orchestrator<'a, C> {
    /// Build from the store's current state. `style` is an optional exemplar from a
    /// reference translation (`reference::style_exemplar`).
    pub fn new(
        client: &'a C,
        store: &'a Store,
        config: &'a Config,
        style: Option<String>,
    ) -> Result<Self> {
        let glossary = GlossarySession::load(store)?;
        let summary = store.get_meta("running_summary")?.unwrap_or_default();
        let metadata = store.project_metadata()?;
        Ok(Self {
            client,
            store,
            config,
            glossary,
            style,
            summary,
            prev_tail: None,
            book: BookIdentity {
                title: metadata.title,
                author: metadata.author,
                title_translated: metadata.title_translated,
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
        let avg = run_avg_ms.or_else(|| self.store.avg_translate_ms(30).ok().flatten())?;
        Some(((avg as u128) * (remaining as u128) / 1000) as u64)
    }

    /// Translate pending chapters in order, at most `limit` of them (`None` = all).
    ///
    /// Terms are written after each chapter; the final flush is a safety net.
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
                .map(|chapter| chapter.status == "done")
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
        Ok(self
            .store
            .next_pending()?
            .map(|(idx, number)| number.unwrap_or(idx)))
    }

    async fn translate_chapter(&mut self, idx: usize) -> Result<()> {
        self.store.set_status(idx, Status::InProgress)?;
        let (title, source) = self
            .store
            .chapter(idx)?
            .ok_or_else(|| anyhow!("no source for chapter {idx}"))?;
        let user_note = self.store.chapter_user_prompt(idx)?;
        let book_note = self.store.book_prompt()?;
        // The book chapter number lets the reply parser tell a heading that
        // slipped into the body from an ordinary opening paragraph.
        let number = self.store.chapter_number(idx)?;

        let started = Instant::now();
        match self
            .translate_one(
                &title,
                &source,
                book_note.as_deref(),
                user_note.as_deref(),
                number,
            )
            .await
        {
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
                        let _ = self
                            .store
                            .save_chapter_context(idx, &self.summary, &chapter_tail);
                    }
                    Err(e) => {
                        tracing::warn!(chapter = idx, "summary update failed: {e:#}");
                        let _ = self
                            .store
                            .save_chapter_context(idx, &self.summary, &chapter_tail);
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
        book_note: Option<&str>,
        user_note: Option<&str>,
        number: Option<usize>,
    ) -> Result<(String, String)> {
        let chapter = Chapter {
            index: 0,
            number: None,
            title: title.to_string(),
            body: source.to_string(),
        };
        let chunks = split_chapter(&chapter, self.config.max_chunk_chars);
        let relevant = self.glossary.relevant(source);
        let ctx = prompt::PromptContext {
            terms: &relevant,
            summary: non_empty(&self.summary),
            prev_tail: self.prev_tail.as_deref(),
            style: self.style.as_deref(),
            book_note,
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
                let (t, b) = reply::parse_chapter_reply(&raw, title, number);
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
        LanguageRepairer::new(self.client, self.config)
            .enforce(idx, title, body, source)
            .await
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
        self.glossary
            .learn(self.client, self.config, self.store, source, translation)
            .await
    }

    /// Write the terms learned since the last flush, and clear the dirty set.
    ///
    /// Runs after every chapter, and again when a run ends however it ends
    /// (finished, paused, cancelled, failed) so terms extracted on a path that
    /// skipped the per-chapter flush are not lost. Only the rows that actually
    /// changed are written, so the cost is proportional to what was learned
    /// rather than to the size of the glossary.
    fn flush_glossary(&mut self) -> Result<usize> {
        self.glossary.flush(self.store)
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

    use std::sync::Mutex;

    use crate::translator::reply::{BODY_MARK, TITLE_MARK};

    /// A scripted stand-in for the model.
    ///
    /// It answers by recognising which prompt it was handed, and records every
    /// request, so a test can assert not just the result but what was sent: the
    /// point of the line-scoped repair is that a chapter is *not* re-sent.
    #[derive(Default)]
    struct FakeModel {
        /// `(source title, translated title, translated body)` per chapter.
        bodies: Vec<(String, String, String)>,
        /// Replacement text the repair pass returns for any line it is given.
        repair_to: Option<String>,
        /// Source titles whose translation request must fail.
        fail_titles: Vec<String>,
        calls: Mutex<Vec<(String, String)>>,
    }

    impl FakeModel {
        fn prose_calls(&self) -> Vec<(String, String)> {
            self.calls.lock().unwrap().clone()
        }

        fn body_for(&self, user: &str) -> Option<(String, String, String)> {
            self.bodies
                .iter()
                .find(|(source_title, ..)| user.contains(source_title.as_str()))
                .cloned()
        }
    }

    impl crate::translator::Translate for FakeModel {
        async fn translate(&self, system: &str, user: &str) -> Result<String> {
            self.calls
                .lock()
                .unwrap()
                .push((system.to_string(), user.to_string()));

            if system.contains("running synopsis") {
                return Ok("Синопсис.".to_string());
            }
            if let Some(title) = self.fail_titles.iter().find(|t| user.contains(t.as_str())) {
                anyhow::bail!("scripted failure for {title}");
            }
            let (_, title, body) = self
                .body_for(user)
                .ok_or_else(|| anyhow!("fake has no body for this request"))?;
            Ok(format!("{TITLE_MARK}\n{title}\n{BODY_MARK}\n{body}"))
        }

        async fn translate_json(&self, system: &str, user: &str) -> Result<String> {
            self.calls
                .lock()
                .unwrap()
                .push((system.to_string(), user.to_string()));

            if system.contains("extract named entities") {
                return Ok(
                    r#"{"terms":[{"source":"王林","target":"Ван Линь","kind":"person"}]}"#
                        .to_string(),
                );
            }
            if system.contains("clean up") {
                let replacement = self.repair_to.clone().unwrap_or_default();
                // Echo back every line number the prompt listed.
                let lines: Vec<String> = user
                    .lines()
                    .filter_map(|l| l.strip_prefix('['))
                    .filter_map(|l| l.split_once(']'))
                    .map(|(n, _)| format!(r#"{{"n":{},"text":"{}"}}"#, n.trim(), replacement))
                    .collect();
                return Ok(format!(r#"{{"lines":[{}]}}"#, lines.join(",")));
            }
            anyhow::bail!("fake got an unexpected json prompt")
        }
    }

    fn chapters() -> Vec<Chapter> {
        (1..=3)
            .map(|i| Chapter {
                index: i,
                number: Some(i),
                title: format!("第{i}章"),
                body: format!("原文 {i}"),
            })
            .collect()
    }

    fn store_with_chapters() -> Store {
        let store = Store::open(":memory:").unwrap();
        store.init_chapters(&chapters()).unwrap();
        store
    }

    fn clean_model() -> FakeModel {
        FakeModel {
            bodies: (1..=3)
                .map(|i| {
                    (
                        format!("第{i}章"),
                        format!("Глава {i}"),
                        format!("Глава {i}. Перевод главы."),
                    )
                })
                .collect(),
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn run_translates_every_pending_chapter_in_order() {
        let store = store_with_chapters();
        let config = Config::default();
        let model = clean_model();
        let mut orch = Orchestrator::new(&model, &store, &config, None).unwrap();

        let mut seen: Vec<Option<usize>> = Vec::new();
        orch.run(None, &AtomicBool::new(false), |ev| {
            if ev.phase == "chapter_start" {
                seen.push(ev.current_number);
            }
        })
        .await
        .unwrap();

        assert_eq!(seen, vec![Some(1), Some(2), Some(3)]);
        assert_eq!(store.stats().unwrap().done, 3);
        assert!(store.pending_chapters().unwrap().is_empty());
        let chapter = store.chapter_full(2).unwrap().unwrap();
        let (title, body) = (
            chapter.translated_title.unwrap(),
            chapter.translated.unwrap(),
        );
        assert_eq!(title, "Глава 2");
        assert_eq!(body, "Глава 2. Перевод главы.");
    }

    #[tokio::test]
    async fn run_honours_the_limit_and_leaves_the_rest_pending() {
        let store = store_with_chapters();
        let config = Config::default();
        let model = clean_model();
        let mut orch = Orchestrator::new(&model, &store, &config, None).unwrap();

        orch.run(Some(2), &AtomicBool::new(false), |_| {})
            .await
            .unwrap();

        assert_eq!(store.stats().unwrap().done, 2);
        assert_eq!(store.pending_chapters().unwrap(), vec![3]);
    }

    #[tokio::test]
    async fn a_cancelled_run_stops_and_stays_resumable() {
        let store = store_with_chapters();
        let config = Config::default();
        let model = clean_model();
        let mut orch = Orchestrator::new(&model, &store, &config, None).unwrap();

        let cancel = AtomicBool::new(false);
        orch.run(None, &cancel, |ev| {
            if ev.phase == "chapter_done" {
                cancel.store(true, Ordering::Relaxed);
            }
        })
        .await
        .unwrap();

        assert_eq!(store.stats().unwrap().done, 1);
        assert_eq!(store.pending_chapters().unwrap(), vec![2, 3]);
        // Nothing is left claiming to be in flight.
        assert_eq!(store.stats().unwrap().in_progress, 0);
    }

    /// A chapter the model cannot translate is marked failed and the run goes on.
    #[tokio::test]
    async fn a_failed_chapter_does_not_sink_the_run() {
        let store = store_with_chapters();
        let config = Config::default();
        let model = FakeModel {
            fail_titles: vec!["第2章".to_string()],
            ..clean_model()
        };
        let mut orch = Orchestrator::new(&model, &store, &config, None).unwrap();

        orch.run(None, &AtomicBool::new(false), |_| {})
            .await
            .unwrap();

        let stats = store.stats().unwrap();
        assert_eq!(stats.done, 2);
        assert_eq!(stats.failed, 1);
        assert_eq!(store.chapter_full(2).unwrap().unwrap().status, "failed");
    }

    /// The terms a chapter teaches must be in the database before the next
    /// chapter is translated, not at the end of the run.
    #[tokio::test]
    async fn the_glossary_is_written_as_the_run_goes() {
        let store = store_with_chapters();
        let config = Config::default();
        let model = clean_model();
        let mut orch = Orchestrator::new(&model, &store, &config, None).unwrap();

        let mut after_first: Option<usize> = None;
        orch.run(None, &AtomicBool::new(false), |ev| {
            if ev.phase == "chapter_done" && after_first.is_none() {
                after_first = Some(store.load_glossary().unwrap().len());
            }
        })
        .await
        .unwrap();

        assert_eq!(
            after_first,
            Some(1),
            "the first chapter's term was not persisted"
        );
        let glossary = store.load_glossary().unwrap();
        assert_eq!(glossary.len(), 1);
        assert_eq!(glossary[0].target, "Ван Линь");
        // Seen once per chapter, so the count reflects all three.
        assert_eq!(glossary[0].frequency, 3);
    }

    /// The repair pass must send the offending lines, not the whole chapter.
    #[tokio::test]
    async fn repair_sends_only_the_offending_lines() {
        let store = Store::open(":memory:").unwrap();
        store
            .init_chapters(&[Chapter {
                index: 1,
                number: Some(1),
                title: "第1章".into(),
                body: "原文".into(),
            }])
            .unwrap();
        let config = Config::default();
        let model = FakeModel {
            bodies: vec![(
                "第1章".to_string(),
                "Глава 1".to_string(),
                "Первая строка.\nОн увидел cultivation.\nТретья строка.".to_string(),
            )],
            repair_to: Some("Он увидел культивацию.".to_string()),
            ..Default::default()
        };
        let mut orch = Orchestrator::new(&model, &store, &config, None).unwrap();

        orch.run(None, &AtomicBool::new(false), |_| {})
            .await
            .unwrap();

        let body = store.chapter_full(1).unwrap().unwrap().translated.unwrap();
        assert_eq!(
            body,
            "Первая строка.\nОн увидел культивацию.\nТретья строка."
        );

        let repair_prompts: Vec<String> = model
            .prose_calls()
            .into_iter()
            .filter(|(system, _)| system.contains("clean up"))
            .map(|(_, user)| user)
            .collect();
        assert_eq!(
            repair_prompts.len(),
            1,
            "expected exactly one repair request"
        );
        let sent = &repair_prompts[0];
        assert!(
            sent.contains("Он увидел cultivation."),
            "the bad line was not sent"
        );
        assert!(
            !sent.contains("Первая строка."),
            "a clean line was sent: {sent}"
        );
        assert!(
            !sent.contains("Третья строка."),
            "a clean line was sent: {sent}"
        );
    }

    /// The title is line 0 of the repair, so a title that kept a foreign word is
    /// fixed in the same request as the body rather than needing its own.
    #[tokio::test]
    async fn repair_covers_the_title() {
        let store = Store::open(":memory:").unwrap();
        store
            .init_chapters(&[Chapter {
                index: 1,
                number: Some(1),
                title: "第1章".into(),
                body: "原文".into(),
            }])
            .unwrap();
        let config = Config::default();
        let model = FakeModel {
            bodies: vec![(
                "第1章".to_string(),
                "Глава 1: cultivation".to_string(),
                "Чистая строка перевода.".to_string(),
            )],
            repair_to: Some("Глава 1: культивация".to_string()),
            ..Default::default()
        };
        let mut orch = Orchestrator::new(&model, &store, &config, None).unwrap();

        orch.run(None, &AtomicBool::new(false), |_| {})
            .await
            .unwrap();

        let row = store.chapter_full(1).unwrap().unwrap();
        assert_eq!(row.translated_title.unwrap(), "Глава 1: культивация");
        assert_eq!(row.translated.unwrap(), "Чистая строка перевода.");
    }

    /// A clean translation costs nothing extra: no repair request at all.
    #[tokio::test]
    async fn a_clean_translation_triggers_no_repair() {
        let store = store_with_chapters();
        let config = Config::default();
        let model = clean_model();
        let mut orch = Orchestrator::new(&model, &store, &config, None).unwrap();

        orch.run(Some(1), &AtomicBool::new(false), |_| {})
            .await
            .unwrap();

        assert!(
            !model
                .prose_calls()
                .iter()
                .any(|(system, _)| system.contains("clean up")),
            "a clean chapter should not be repaired"
        );
    }

    #[tokio::test]
    async fn book_and_chapter_prompts_are_injected() {
        let store = store_with_chapters();
        store
            .set_book_prompt("Write chapter titles as Глава N. Title, with Arabic numerals.")
            .unwrap();
        store
            .set_chapter_user_prompt(1, "Keep this chapter's title short.")
            .unwrap();
        let config = Config::default();
        let model = clean_model();
        let mut orch = Orchestrator::new(&model, &store, &config, None).unwrap();

        orch.run(Some(1), &AtomicBool::new(false), |_| {})
            .await
            .unwrap();

        let user = model
            .prose_calls()
            .into_iter()
            .find(|(system, _)| !system.contains("running synopsis"))
            .map(|(_, user)| user)
            .expect("translation request");
        assert!(user.contains("Arabic numerals"));
        assert!(user.contains("Keep this chapter's title short."));
        assert!(user.find("THIS BOOK").unwrap() < user.find("THIS chapter only").unwrap());
    }

    #[test]
    fn non_empty_filters_blank() {
        assert_eq!(non_empty("  "), None);
        assert_eq!(non_empty("x"), Some("x"));
    }
}
