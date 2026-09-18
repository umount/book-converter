//! Chapter rows: source text, translation status, and the per-chapter rolling
//! context that lets a run resume, or a single chapter be retranslated with the
//! continuity it originally had.

use anyhow::Result;
use rusqlite::{params, OptionalExtension};

use crate::book::Chapter;

use super::{ChapterListRow, ChapterRecord, Stats, Status, Store};

impl Store {
    /// Load the book's chapters into the database.
    ///
    /// Idempotent: an existing chapter row (with its status/translation) is kept,
    /// so re-opening the same book never discards work already done.
    pub fn init_chapters(&self, chapters: &[Chapter]) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        {
            let mut stmt = tx.prepare(
                "INSERT INTO chapters (idx, number, title, source)
                 VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(idx) DO NOTHING",
            )?;
            for c in chapters {
                stmt.execute(params![
                    c.index as i64,
                    c.number.map(|n| n as i64),
                    c.title,
                    c.body,
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Return the indices of chapters not yet translated (`pending`/`failed`),
    /// in reading order — the resumable queue.
    pub fn pending_chapters(&self) -> Result<Vec<usize>> {
        let mut stmt = self.conn.prepare(
            "SELECT idx FROM chapters
             WHERE status IN ('pending', 'failed')
             ORDER BY idx",
        )?;
        let rows = stmt
            .query_map([], |r| r.get::<_, i64>(0))?
            .collect::<rusqlite::Result<Vec<i64>>>()?;
        Ok(rows.into_iter().map(|n| n as usize).collect())
    }

    /// First pending/failed chapter: `(idx, number)` for resume hints.
    pub fn next_pending(&self) -> Result<Option<(usize, Option<usize>)>> {
        let row = self
            .conn
            .query_row(
                "SELECT idx, number FROM chapters
                 WHERE status IN ('pending', 'failed')
                 ORDER BY idx
                 LIMIT 1",
                [],
                |r| {
                    Ok((
                        r.get::<_, i64>(0)? as usize,
                        r.get::<_, Option<i64>>(1)?.map(|n| n as usize),
                    ))
                },
            )
            .optional()?;
        Ok(row)
    }

    /// Book chapter number stored for this reading-order index (`第N章` → N).
    pub fn chapter_number(&self, index: usize) -> Result<Option<usize>> {
        let row = self
            .conn
            .query_row(
                "SELECT number FROM chapters WHERE idx = ?1",
                params![index as i64],
                |r| r.get::<_, Option<i64>>(0),
            )
            .optional()?;
        Ok(row.flatten().map(|n| n as usize))
    }

    /// Reading-order index of the chapter with this book number, if any.
    pub fn index_for_number(&self, number: usize) -> Result<Option<usize>> {
        let row = self
            .conn
            .query_row(
                "SELECT idx FROM chapters WHERE number = ?1 ORDER BY idx LIMIT 1",
                params![number as i64],
                |r| r.get::<_, i64>(0),
            )
            .optional()?;
        Ok(row.map(|n| n as usize))
    }

    /// Highest book chapter number present (for UI inputs).
    pub fn max_chapter_number(&self) -> Result<Option<usize>> {
        let row: Option<i64> = self
            .conn
            .query_row(
                "SELECT MAX(number) FROM chapters WHERE number IS NOT NULL",
                [],
                |r| r.get(0),
            )
            .optional()?
            .flatten();
        Ok(row.map(|n| n as usize))
    }

    /// Read a chapter's source title and body (for building the request).
    pub fn chapter(&self, index: usize) -> Result<Option<(String, String)>> {
        let row = self
            .conn
            .query_row(
                "SELECT title, source FROM chapters WHERE idx = ?1",
                params![index as i64],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
            )
            .optional()?;
        Ok(row)
    }

    /// Save a chapter's translated title + body (produced by the model) and mark it
    /// `done`. Overwrites the `origin` (so re-translating a reference chapter with the
    /// model re-labels it accordingly).
    pub fn save_translation(
        &self,
        index: usize,
        translated_title: &str,
        translated_body: &str,
    ) -> Result<()> {
        self.save_translation_timed(index, translated_title, translated_body, None)
    }

    /// Save a model translation and optionally record how long it took (ms).
    pub fn save_translation_timed(
        &self,
        index: usize,
        translated_title: &str,
        translated_body: &str,
        translate_ms: Option<u64>,
    ) -> Result<()> {
        self.conn.execute(
            "UPDATE chapters
             SET translated = ?3, translated_title = ?2,
                 status = 'done', origin = 'model',
                 translate_ms = COALESCE(?4, translate_ms),
                 updated_at = datetime('now')
             WHERE idx = ?1",
            params![
                index as i64,
                translated_title,
                translated_body,
                translate_ms.map(|m| m as i64),
            ],
        )?;
        Ok(())
    }

    /// Record (or clear, with an empty list) the words a chapter kept in the wrong
    /// language, so the UI can flag it and a human can go fix it.
    pub fn set_language_issues(&self, index: usize, issues: &[String]) -> Result<()> {
        let value = (!issues.is_empty()).then(|| issues.join(", "));
        self.conn.execute(
            "UPDATE chapters SET lang_issues = ?2 WHERE idx = ?1",
            params![index as i64, value],
        )?;
        Ok(())
    }

    /// Average `translate_ms` over recent timed chapters (for ETA).
    pub fn avg_translate_ms(&self, limit: usize) -> Result<Option<u64>> {
        let lim = limit.max(1) as i64;
        let row: Option<(i64, i64)> = self
            .conn
            .query_row(
                "SELECT COUNT(*), COALESCE(SUM(translate_ms), 0) FROM (
                    SELECT translate_ms FROM chapters
                    WHERE translate_ms IS NOT NULL AND translate_ms > 0 AND status = 'done'
                    ORDER BY idx DESC
                    LIMIT ?1
                 )",
                params![lim],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        Ok(row.and_then(|(n, sum)| {
            if n == 0 {
                None
            } else {
                Some((sum / n) as u64)
            }
        }))
    }

    /// Persist the rolling continuity context *after* a chapter finished translating.
    /// `summary` is the story-so-far synopsis; `prev_tail` is the closing lines of
    /// this chapter's translation (fed into the next chapter's prompt).
    pub fn save_chapter_context(
        &self,
        index: usize,
        summary: &str,
        prev_tail: &str,
    ) -> Result<()> {
        self.conn.execute(
            "UPDATE chapters
             SET rolling_summary = ?2, prev_tail = ?3, updated_at = datetime('now')
             WHERE idx = ?1",
            params![index as i64, summary, prev_tail],
        )?;
        Ok(())
    }

    /// Continuity context to use when translating `index`: the rolling summary and
    /// previous-chapter tail from the nearest earlier `done` chapter that has them.
    /// Falls back to deriving a tail from that chapter's translated body.
    pub fn context_before(&self, index: usize) -> Result<(String, Option<String>)> {
        let row = self
            .conn
            .query_row(
                "SELECT rolling_summary, prev_tail, translated
                 FROM chapters
                 WHERE idx < ?1 AND status = 'done'
                 ORDER BY idx DESC
                 LIMIT 1",
                params![index as i64],
                |r| {
                    Ok((
                        r.get::<_, Option<String>>(0)?,
                        r.get::<_, Option<String>>(1)?,
                        r.get::<_, Option<String>>(2)?,
                    ))
                },
            )
            .optional()?;

        let Some((summary, prev_tail, translated)) = row else {
            let summary = self.get_meta("running_summary")?.unwrap_or_default();
            let prev_tail = self
                .get_meta("boot_prev_tail")?
                .filter(|s| !s.trim().is_empty());
            return Ok((summary, prev_tail));
        };

        let summary = match summary.filter(|s| !s.trim().is_empty()) {
            Some(summary) => summary,
            None => self.get_meta("running_summary")?.unwrap_or_default(),
        };

        let prev_tail = prev_tail
            .filter(|s| !s.trim().is_empty())
            .or_else(|| {
                translated
                    .as_deref()
                    .map(|b| crate::textutil::closing_excerpt(b, 400))
                    .filter(|s| !s.trim().is_empty())
            });

        Ok((summary, prev_tail))
    }

    /// Overwrite the continuity context that [`Self::context_before`] would return
    /// for `index` (so the next translate of this chapter uses the edited text).
    /// Writes onto the previous `done` chapter when one exists; always mirrors the
    /// summary into book-level `running_summary`. When there is no previous chapter,
    /// an optional `boot_prev_tail` meta key holds the tail.
    pub fn set_context_before(
        &self,
        index: usize,
        summary: &str,
        prev_tail: &str,
    ) -> Result<()> {
        let summary = summary.trim();
        let prev_tail = prev_tail.trim();

        let prev_idx: Option<i64> = self
            .conn
            .query_row(
                "SELECT idx FROM chapters
                 WHERE idx < ?1 AND status = 'done'
                 ORDER BY idx DESC
                 LIMIT 1",
                params![index as i64],
                |r| r.get(0),
            )
            .optional()?;

        let tx = self.conn.unchecked_transaction()?;
        if let Some(prev) = prev_idx {
            tx.execute(
                "UPDATE chapters
                 SET rolling_summary = ?2, prev_tail = ?3, updated_at = datetime('now')
                 WHERE idx = ?1",
                params![prev, summary, prev_tail],
            )?;
            // Clear a leftover boot tail so context_before prefers the chapter row.
            tx.execute(
                "INSERT INTO meta (key, value) VALUES ('boot_prev_tail', '')
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                [],
            )?;
        } else {
            tx.execute(
                "INSERT INTO meta (key, value) VALUES ('boot_prev_tail', ?1)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                params![prev_tail],
            )?;
        }
        tx.execute(
            "INSERT INTO meta (key, value) VALUES ('running_summary', ?1)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![summary],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Manually edit a chapter's translation (keeps/sets `origin = 'manual'`).
    pub fn save_manual_translation(
        &self,
        index: usize,
        translated_title: &str,
        translated_body: &str,
    ) -> Result<()> {
        self.conn.execute(
            "UPDATE chapters
             SET translated = ?3, translated_title = ?2,
                 status = 'done', origin = 'manual', updated_at = datetime('now')
             WHERE idx = ?1",
            params![index as i64, translated_title, translated_body],
        )?;
        Ok(())
    }

    /// Seed a chapter from a reference translation, but only if it is still
    /// `pending` (never overwrite work already done by the model or an earlier
    /// import). Marks it `done` with `origin = 'reference'`. Returns whether a row
    /// was filled.
    /// True when the chapter already holds a non-empty translation.
    pub fn has_translation(&self, index: usize) -> Result<bool> {
        let n: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM chapters
             WHERE idx = ?1 AND translated IS NOT NULL AND TRIM(translated) != ''",
            params![index as i64],
            |r| r.get(0),
        )?;
        Ok(n > 0)
    }

    pub fn save_reference_chapter(
        &self,
        index: usize,
        translated_title: &str,
        translated_body: &str,
    ) -> Result<bool> {
        let n = self.conn.execute(
            "UPDATE chapters
             SET translated = ?3, translated_title = ?2,
                 status = 'done', origin = 'reference', updated_at = datetime('now')
             WHERE idx = ?1 AND status = 'pending'",
            params![index as i64, translated_title, translated_body],
        )?;
        Ok(n > 0)
    }

    /// Reset chapters back to `pending` so a later run re-translates them (with the
    /// current glossary). `from_index` limits it to chapters at/after that reading-order
    /// index; `None` resets the whole book. Existing translated text is left in place
    /// until a re-run overwrites it. Returns the number of chapters reset.
    pub fn reset_from(&self, from_index: Option<usize>) -> Result<usize> {
        let n = match from_index {
            Some(idx) => self.conn.execute(
                "UPDATE chapters SET status = 'pending', updated_at = datetime('now')
                 WHERE idx >= ?1",
                params![idx as i64],
            )?,
            None => self.conn.execute(
                "UPDATE chapters SET status = 'pending', updated_at = datetime('now')",
                [],
            )?,
        };
        Ok(n)
    }

    /// Like [`Self::reset_from`], but `from_number` is the book chapter number
    /// (from the title, e.g. 523 for `第523章`), not the reading-order index.
    pub fn reset_from_number(&self, from_number: Option<usize>) -> Result<usize> {
        match from_number {
            None => self.reset_from(None),
            Some(n) => {
                let idx = self
                    .index_for_number(n)?
                    .ok_or_else(|| anyhow::anyhow!("no chapter with number {n}"))?;
                self.reset_from(Some(idx))
            }
        }
    }

    /// Update a chapter's status.
    pub fn set_status(&self, index: usize, status: Status) -> Result<()> {
        self.conn.execute(
            "UPDATE chapters
             SET status = ?2, updated_at = datetime('now')
             WHERE idx = ?1",
            params![index as i64, status.as_str()],
        )?;
        Ok(())
    }

    /// Aggregate progress counts.
    pub fn stats(&self) -> Result<Stats> {
        let mut stmt = self
            .conn
            .prepare("SELECT status, COUNT(*) FROM chapters GROUP BY status")?;
        let mut stats = Stats::default();
        let rows = stmt.query_map([], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
        })?;
        for row in rows {
            let (status, count) = row?;
            let count = count as usize;
            stats.total += count;
            match Status::from_str(&status) {
                Status::Pending => stats.pending += count,
                Status::InProgress => stats.in_progress += count,
                Status::Done => stats.done += count,
                Status::Failed => stats.failed += count,
            }
        }
        Ok(stats)
    }

    /// List chapters for the UI in reading order.
    pub fn list_chapters(&self) -> Result<Vec<ChapterListRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT idx, number, title, translated_title, status, origin, lang_issues
             FROM chapters ORDER BY idx",
        )?;
        let rows = stmt
            .query_map([], |r| {
                Ok(ChapterListRow {
                    idx: r.get::<_, i64>(0)? as usize,
                    number: r.get::<_, Option<i64>>(1)?.map(|n| n as usize),
                    title: r.get(2)?,
                    translated_title: r.get(3)?,
                    status: r.get(4)?,
                    origin: r.get(5)?,
                    lang_issues: r.get(6)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Full chapter view for the reader and translation coordinator.
    pub fn chapter_full(&self, index: usize) -> Result<Option<ChapterRecord>> {
        let row = self
            .conn
            .query_row(
                "SELECT number, title, source, status, translated_title, translated, origin, user_prompt
                 FROM chapters WHERE idx = ?1",
                params![index as i64],
                |r| {
                    Ok(ChapterRecord {
                        number: r.get::<_, Option<i64>>(0)?.map(|n| n as usize),
                        source_title: r.get(1)?,
                        source: r.get(2)?,
                        status: r.get(3)?,
                        translated_title: r.get(4)?,
                        translated: r.get(5)?,
                        origin: r.get(6)?,
                        user_prompt: r.get(7)?,
                    })
                },
            )
            .optional()?;
        Ok(row)
    }

    /// Read the optional user instruction for one chapter (empty → None).
    pub fn chapter_user_prompt(&self, index: usize) -> Result<Option<String>> {
        let v: Option<String> = self
            .conn
            .query_row(
                "SELECT user_prompt FROM chapters WHERE idx = ?1",
                params![index as i64],
                |r| r.get(0),
            )
            .optional()?
            .flatten();
        Ok(v.filter(|s| !s.trim().is_empty()))
    }

    /// Set or clear the per-chapter user instruction (empty string clears it).
    pub fn set_chapter_user_prompt(&self, index: usize, prompt: &str) -> Result<()> {
        let value: Option<&str> = {
            let t = prompt.trim();
            if t.is_empty() {
                None
            } else {
                Some(t)
            }
        };
        self.conn.execute(
            "UPDATE chapters SET user_prompt = ?2, updated_at = datetime('now') WHERE idx = ?1",
            params![index as i64, value],
        )?;
        Ok(())
    }

    /// All translated chapters in reading order (for export).
    /// Returns `(index, translated_title, translated_body)` — the translated
    /// title falls back to the source title if a run predates title translation.
    pub fn translated_chapters(&self) -> Result<Vec<(usize, String, String)>> {
        let mut stmt = self.conn.prepare(
            "SELECT idx, COALESCE(translated_title, title), translated FROM chapters
             WHERE status = 'done' AND translated IS NOT NULL
             ORDER BY idx",
        )?;
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, i64>(0)? as usize,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Chapters that have a stored rolling context: `(idx, rolling_summary, prev_tail)`.
    /// Empty strings are normalized to `""` (never null in the result).
    pub fn chapter_contexts(&self) -> Result<Vec<(usize, String, String)>> {
        let mut stmt = self.conn.prepare(
            "SELECT idx, COALESCE(rolling_summary, ''), COALESCE(prev_tail, '')
             FROM chapters
             WHERE (rolling_summary IS NOT NULL AND TRIM(rolling_summary) != '')
                OR (prev_tail IS NOT NULL AND TRIM(prev_tail) != '')
             ORDER BY idx",
        )?;
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, i64>(0)? as usize,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Done chapters with source + translation, in reading order:
    /// `(idx, source, translated)`.
    /// Chapters seeded from a reference translation: how many, and the highest
    /// book number among them.
    ///
    /// Derived from the chapters themselves rather than remembered separately,
    /// so it stays true after a reset (which keeps the text and only changes
    /// status) and needs no access to the reference file.
    pub fn reference_stats(&self) -> Result<(usize, Option<usize>)> {
        self.conn
            .query_row(
                "SELECT COUNT(*), MAX(number) FROM chapters
                 WHERE origin = 'reference'
                   AND translated IS NOT NULL AND TRIM(translated) != ''",
                [],
                |r| {
                    Ok((
                        r.get::<_, i64>(0)? as usize,
                        r.get::<_, Option<i64>>(1)?.map(|n| n as usize),
                    ))
                },
            )
            .map_err(Into::into)
    }

    /// Source and professional-translation pairs from reference-seeded chapters,
    /// in reading order. This is what a pinned glossary is mined from.
    pub fn reference_pairs(&self, limit: usize) -> Result<Vec<(usize, String, String)>> {
        let mut stmt = self.conn.prepare(
            "SELECT idx, source, translated FROM chapters
             WHERE origin = 'reference'
               AND translated IS NOT NULL AND TRIM(translated) != ''
               AND source IS NOT NULL AND TRIM(source) != ''
             ORDER BY idx
             LIMIT ?1",
        )?;
        let rows = stmt
            .query_map(params![limit as i64], |r| {
                Ok((
                    r.get::<_, i64>(0)? as usize,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// A style excerpt from the first substantial reference chapter, for the
    /// few-shot style guide in translation prompts.
    pub fn reference_style(&self, max_chars: usize) -> Result<Option<String>> {
        let text: Option<String> = self
            .conn
            .query_row(
                "SELECT translated FROM chapters
                 WHERE origin = 'reference' AND LENGTH(translated) > 200
                 ORDER BY idx LIMIT 1",
                [],
                |r| r.get(0),
            )
            .optional()?;
        Ok(text.map(|t| t.chars().take(max_chars).collect()))
    }

    /// Put reference-seeded chapters that were reset back to `done`.
    ///
    /// A reset only changes status, so the professional text is still there;
    /// re-seeding is restoring the status, not re-importing the text.
    pub fn restore_reference_chapters(&self) -> Result<usize> {
        let n = self.conn.execute(
            "UPDATE chapters SET status = 'done', updated_at = datetime('now')
             WHERE origin = 'reference' AND status = 'pending'
               AND translated IS NOT NULL AND TRIM(translated) != ''",
            [],
        )?;
        Ok(n)
    }

    pub fn done_chapter_pairs(&self) -> Result<Vec<(usize, String, String)>> {
        let mut stmt = self.conn.prepare(
            "SELECT idx, source, translated FROM chapters
             WHERE status = 'done'
               AND translated IS NOT NULL AND TRIM(translated) != ''
               AND source IS NOT NULL AND TRIM(source) != ''
             ORDER BY idx",
        )?;
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, i64>(0)? as usize,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }
}
