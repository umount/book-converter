//! Persisting translation progress in SQLite.
//!
//! Critical for a ~1350-chapter book: the process can be interrupted and resumed
//! from where it left off. Stores each chapter's status, its translation, the
//! per-chapter rolling context (summary + previous-chapter tail), and the glossary.
//!
//! ## Schema
//! - `chapters(idx PK, number, title, source, status, translated, …,
//!   rolling_summary, prev_tail)` — status: pending | in_progress | done | failed.
//!   `rolling_summary` / `prev_tail` are the continuity context *after* this chapter
//!   finished, used to resume or translate a later chapter in isolation.
//! - `glossary(source PK, target, kind, frequency, pinned)`
//! - `meta(key PK, value)` — book path, run settings, book-level `running_summary`
//!
//! Self-contained apart from `rusqlite` (bundled SQLite, no system deps), so it
//! is testable without the Tauri crate.

use anyhow::Result;
use rusqlite::{params, Connection, OptionalExtension};

use crate::book::Chapter;
use crate::glossary::{Term, TermKind};

/// Chapter translation status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Pending,
    InProgress,
    Done,
    Failed,
}

impl Status {
    fn as_str(self) -> &'static str {
        match self {
            Status::Pending => "pending",
            Status::InProgress => "in_progress",
            Status::Done => "done",
            Status::Failed => "failed",
        }
    }

    fn from_str(s: &str) -> Status {
        match s {
            "in_progress" => Status::InProgress,
            "done" => Status::Done,
            "failed" => Status::Failed,
            _ => Status::Pending,
        }
    }
}

/// Aggregate progress counts, for the UI/progress event.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Stats {
    pub total: usize,
    pub done: usize,
    pub failed: usize,
    pub in_progress: usize,
    pub pending: usize,
}

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS meta (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS chapters (
    idx              INTEGER PRIMARY KEY,
    number           INTEGER,
    title            TEXT NOT NULL,
    source           TEXT NOT NULL,
    status           TEXT NOT NULL DEFAULT 'pending',
    translated       TEXT,
    translated_title TEXT,
    origin           TEXT,
    rolling_summary  TEXT,
    prev_tail        TEXT,
    user_prompt      TEXT,
    translate_ms     INTEGER,
    updated_at       TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE TABLE IF NOT EXISTS glossary (
    source    TEXT PRIMARY KEY,
    target    TEXT NOT NULL,
    kind      TEXT NOT NULL,
    frequency INTEGER NOT NULL DEFAULT 1,
    pinned    INTEGER NOT NULL DEFAULT 0
);
"#;

/// Progress store on top of SQLite.
pub struct Store {
    conn: Connection,
}

impl Store {
    /// Open/create the database, apply the schema, and recover from a crash by
    /// resetting any `in_progress` chapter back to `pending`.
    pub fn open(path: &str) -> Result<Self> {
        let conn = Connection::open(path)?;
        conn.execute_batch(SCHEMA)?;
        let store = Store { conn };
        store.migrate()?;
        store.reset_in_progress()?;
        Ok(store)
    }

    /// Additive migrations for databases created by an older version.
    fn migrate(&self) -> Result<()> {
        self.ensure_column("chapters", "origin", "TEXT")?;
        self.ensure_column("chapters", "rolling_summary", "TEXT")?;
        self.ensure_column("chapters", "prev_tail", "TEXT")?;
        self.ensure_column("chapters", "user_prompt", "TEXT")?;
        self.ensure_column("chapters", "translate_ms", "INTEGER")?;
        Ok(())
    }

    fn ensure_column(&self, table: &str, name: &str, ty: &str) -> Result<()> {
        let has: i64 = self.conn.query_row(
            &format!("SELECT COUNT(*) FROM pragma_table_info('{table}') WHERE name = '{name}'"),
            [],
            |r| r.get(0),
        )?;
        if has == 0 {
            self.conn
                .execute(&format!("ALTER TABLE {table} ADD COLUMN {name} {ty}"), [])?;
        }
        Ok(())
    }

    /// Reset chapters stuck `in_progress` (from a previous crash) to `pending`.
    fn reset_in_progress(&self) -> Result<usize> {
        let n = self.conn.execute(
            "UPDATE chapters SET status = 'pending' WHERE status = 'in_progress'",
            [],
        )?;
        Ok(n)
    }

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

        let summary = summary
            .filter(|s| !s.trim().is_empty())
            .or_else(|| self.get_meta("running_summary").ok().flatten())
            .unwrap_or_default();

        let prev_tail = prev_tail
            .filter(|s| !s.trim().is_empty())
            .or_else(|| {
                translated
                    .as_deref()
                    .map(|b| {
                        let chars: Vec<char> = b.chars().collect();
                        let start = chars.len().saturating_sub(400);
                        chars[start..].iter().collect::<String>()
                    })
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

        if let Some(prev) = prev_idx {
            self.save_chapter_context(prev as usize, summary, prev_tail)?;
            // Clear a leftover boot tail so context_before prefers the chapter row.
            let _ = self.set_meta("boot_prev_tail", "");
        } else {
            let _ = self.set_meta("boot_prev_tail", prev_tail);
        }
        self.set_meta("running_summary", summary)?;
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

    /// List chapters for the UI: `(idx, number, title, status, origin)` in order.
    #[allow(clippy::type_complexity)]
    pub fn list_chapters(
        &self,
    ) -> Result<Vec<(usize, Option<usize>, String, String, Option<String>)>> {
        let mut stmt = self
            .conn
            .prepare("SELECT idx, number, title, status, origin FROM chapters ORDER BY idx")?;
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, i64>(0)? as usize,
                    r.get::<_, Option<i64>>(1)?.map(|n| n as usize),
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, Option<String>>(4)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Full chapter view:
    /// `(number, source_title, source, status, translated_title, translated, origin, user_prompt)`.
    #[allow(clippy::type_complexity)]
    pub fn chapter_full(
        &self,
        index: usize,
    ) -> Result<
        Option<(
            Option<usize>,
            String,
            String,
            String,
            Option<String>,
            Option<String>,
            Option<String>,
            Option<String>,
        )>,
    > {
        let row = self
            .conn
            .query_row(
                "SELECT number, title, source, status, translated_title, translated, origin, user_prompt
                 FROM chapters WHERE idx = ?1",
                params![index as i64],
                |r| {
                    Ok((
                        r.get::<_, Option<i64>>(0)?.map(|n| n as usize),
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, String>(3)?,
                        r.get::<_, Option<String>>(4)?,
                        r.get::<_, Option<String>>(5)?,
                        r.get::<_, Option<String>>(6)?,
                        r.get::<_, Option<String>>(7)?,
                    ))
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

    /// Load the whole glossary.
    pub fn load_glossary(&self) -> Result<Vec<Term>> {
        let mut stmt = self
            .conn
            .prepare("SELECT source, target, kind, frequency, pinned FROM glossary")?;
        let rows = stmt
            .query_map([], |r| {
                Ok(Term {
                    source: r.get(0)?,
                    target: r.get(1)?,
                    kind: kind_from_str(&r.get::<_, String>(2)?),
                    frequency: r.get::<_, i64>(3)? as u32,
                    pinned: r.get::<_, i64>(4)? != 0,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Save (upsert) the glossary. Conflict policy (canon/pinned wins) is applied
    /// in `glossary::merge` before this call; here we just persist the result.
    pub fn save_glossary(&self, terms: &[Term]) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        {
            let mut stmt = tx.prepare(
                "INSERT INTO glossary (source, target, kind, frequency, pinned)
                 VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(source) DO UPDATE SET
                    target = excluded.target,
                    kind = excluded.kind,
                    frequency = excluded.frequency,
                    pinned = excluded.pinned",
            )?;
            for t in terms {
                stmt.execute(params![
                    t.source,
                    t.target,
                    kind_to_str(t.kind),
                    t.frequency as i64,
                    t.pinned as i64,
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Remove a single glossary entry by its source term.
    pub fn delete_term(&self, source: &str) -> Result<()> {
        self.conn
            .execute("DELETE FROM glossary WHERE source = ?1", params![source])?;
        Ok(())
    }

    /// Read a `meta` value.
    pub fn get_meta(&self, key: &str) -> Result<Option<String>> {
        let v = self
            .conn
            .query_row(
                "SELECT value FROM meta WHERE key = ?1",
                params![key],
                |r| r.get::<_, String>(0),
            )
            .optional()?;
        Ok(v)
    }

    /// Write a `meta` value.
    pub fn set_meta(&self, key: &str, value: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO meta (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }
}

/// Stable string form of a term category for the DB.
fn kind_to_str(kind: TermKind) -> &'static str {
    match kind {
        TermKind::Person => "person",
        TermKind::Location => "location",
        TermKind::Organization => "organization",
        TermKind::Term => "term",
    }
}

fn kind_from_str(s: &str) -> TermKind {
    match s {
        "location" => TermKind::Location,
        "organization" => TermKind::Organization,
        "term" => TermKind::Term,
        _ => TermKind::Person,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chapter(index: usize, title: &str, body: &str) -> Chapter {
        Chapter {
            index,
            number: Some(index),
            title: title.into(),
            body: body.into(),
        }
    }

    fn sample() -> Vec<Chapter> {
        vec![
            chapter(1, "第1章", "source one"),
            chapter(2, "第2章", "source two"),
            chapter(3, "第3章", "source three"),
        ]
    }

    #[test]
    fn init_and_pending() {
        let store = Store::open(":memory:").unwrap();
        store.init_chapters(&sample()).unwrap();
        assert_eq!(store.pending_chapters().unwrap(), vec![1, 2, 3]);
        assert_eq!(store.stats().unwrap().total, 3);
    }

    #[test]
    fn save_translation_advances_progress() {
        let store = Store::open(":memory:").unwrap();
        store.init_chapters(&sample()).unwrap();
        store.save_translation(2, "Глава 2", "перевод два").unwrap();
        assert_eq!(store.pending_chapters().unwrap(), vec![1, 3]);
        let stats = store.stats().unwrap();
        assert_eq!(stats.done, 1);
        assert_eq!(stats.pending, 2);
        assert_eq!(
            store.translated_chapters().unwrap(),
            vec![(2, "Глава 2".to_string(), "перевод два".to_string())]
        );
    }

    #[test]
    fn failed_chapters_are_requeued() {
        let store = Store::open(":memory:").unwrap();
        store.init_chapters(&sample()).unwrap();
        store.set_status(1, Status::Failed).unwrap();
        assert_eq!(store.pending_chapters().unwrap(), vec![1, 2, 3]);
        assert_eq!(store.stats().unwrap().failed, 1);
    }

    #[test]
    fn init_is_idempotent_and_keeps_translations() {
        let store = Store::open(":memory:").unwrap();
        store.init_chapters(&sample()).unwrap();
        store.save_translation(2, "T2", "done text").unwrap();
        store.init_chapters(&sample()).unwrap();
        assert_eq!(store.pending_chapters().unwrap(), vec![1, 3]);
        assert_eq!(store.translated_chapters().unwrap().len(), 1);
    }

    #[test]
    fn reopen_resets_in_progress() {
        let path = std::env::temp_dir().join(format!(
            "bc_state_test_{}.db",
            std::process::id()
        ));
        let path_str = path.to_str().unwrap();
        {
            let store = Store::open(path_str).unwrap();
            store.init_chapters(&sample()).unwrap();
            store.set_status(2, Status::InProgress).unwrap();
            assert_eq!(store.pending_chapters().unwrap(), vec![1, 3]);
        }
        {
            let store = Store::open(path_str).unwrap();
            assert_eq!(store.pending_chapters().unwrap(), vec![1, 2, 3]);
        }
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn glossary_roundtrip() {
        let store = Store::open(":memory:").unwrap();
        let terms = vec![
            Term {
                source: "王林".into(),
                target: "Ван Линь".into(),
                kind: TermKind::Person,
                frequency: 5,
                pinned: true,
            },
            Term {
                source: "南凰洲".into(),
                target: "Наньхуанчжоу".into(),
                kind: TermKind::Location,
                frequency: 2,
                pinned: false,
            },
        ];
        store.save_glossary(&terms).unwrap();
        let mut loaded = store.load_glossary().unwrap();
        loaded.sort_by(|a, b| a.source.cmp(&b.source));
        let mut expected = terms.clone();
        expected.sort_by(|a, b| a.source.cmp(&b.source));
        assert_eq!(loaded, expected);
    }

    #[test]
    fn meta_roundtrip() {
        let store = Store::open(":memory:").unwrap();
        assert_eq!(store.get_meta("book_path").unwrap(), None);
        store.set_meta("book_path", "/books/x.txt").unwrap();
        store.set_meta("book_path", "/books/y.txt").unwrap();
        assert_eq!(
            store.get_meta("book_path").unwrap(),
            Some("/books/y.txt".to_string())
        );
    }

    #[test]
    fn chapter_context_roundtrip_and_before() {
        let store = Store::open(":memory:").unwrap();
        store.init_chapters(&sample()).unwrap();
        store
            .save_translation(1, "Глава 1", "конец первой главы вот так")
            .unwrap();
        store
            .save_chapter_context(1, "Герой начал путь.", "вот так")
            .unwrap();

        let (sum, tail) = store.context_before(2).unwrap();
        assert_eq!(sum, "Герой начал путь.");
        assert_eq!(tail.as_deref(), Some("вот так"));

        let (sum0, tail0) = store.context_before(1).unwrap();
        assert!(sum0.is_empty());
        assert!(tail0.is_none());

        store
            .set_context_before(2, "Исправленный синопсис.", "новый хвост")
            .unwrap();
        let (sum2, tail2) = store.context_before(2).unwrap();
        assert_eq!(sum2, "Исправленный синопсис.");
        assert_eq!(tail2.as_deref(), Some("новый хвост"));
        assert_eq!(
            store.get_meta("running_summary").unwrap().as_deref(),
            Some("Исправленный синопсис.")
        );

        store
            .set_context_before(1, "Старт книги.", "пролог закончился так")
            .unwrap();
        let (sum1, tail1) = store.context_before(1).unwrap();
        assert_eq!(sum1, "Старт книги.");
        assert_eq!(tail1.as_deref(), Some("пролог закончился так"));
    }

    #[test]
    fn manual_translation_sets_origin() {
        let store = Store::open(":memory:").unwrap();
        store.init_chapters(&sample()).unwrap();
        store
            .save_manual_translation(1, "Заголовок", "ручной текст")
            .unwrap();
        let full = store.chapter_full(1).unwrap().unwrap();
        assert_eq!(full.3, "done");
        assert_eq!(full.5.as_deref(), Some("ручной текст"));
        assert_eq!(full.6.as_deref(), Some("manual"));
    }

    #[test]
    fn chapter_user_prompt_roundtrip() {
        let store = Store::open(":memory:").unwrap();
        store.init_chapters(&sample()).unwrap();
        assert_eq!(store.chapter_user_prompt(1).unwrap(), None);
        store
            .set_chapter_user_prompt(1, "  Translate 她 as господин, not госпожа.  ")
            .unwrap();
        assert_eq!(
            store.chapter_user_prompt(1).unwrap().as_deref(),
            Some("Translate 她 as господин, not госпожа.")
        );
        let full = store.chapter_full(1).unwrap().unwrap();
        assert_eq!(
            full.7.as_deref(),
            Some("Translate 她 as господин, not госпожа.")
        );
        store.set_chapter_user_prompt(1, "   ").unwrap();
        assert_eq!(store.chapter_user_prompt(1).unwrap(), None);
    }

    #[test]
    fn translate_ms_and_average() {
        let store = Store::open(":memory:").unwrap();
        store.init_chapters(&sample()).unwrap();
        store
            .save_translation_timed(1, "T1", "body1", Some(1000))
            .unwrap();
        store
            .save_translation_timed(2, "T2", "body2", Some(3000))
            .unwrap();
        assert_eq!(store.avg_translate_ms(10).unwrap(), Some(2000));
    }
}
