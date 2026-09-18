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
use rusqlite::Connection;

mod chapters;
mod glossary;
mod meta;
mod search;

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

/// Connection pragmas, applied before the schema on every connection.
///
/// A translation run holds its own connection on a background thread while the
/// UI keeps opening short-lived ones to read progress, chapters and the
/// glossary. In the default rollback-journal mode a writer locks the whole
/// database, and rusqlite installs no busy handler, so those reads would fail
/// outright with SQLITE_BUSY. WAL lets readers run alongside the writer, and the
/// busy timeout absorbs the remaining contention on the write lock itself.
const PRAGMAS: &str = r#"
PRAGMA journal_mode = WAL;
PRAGMA busy_timeout = 5000;
PRAGMA synchronous = NORMAL;
"#;

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
    -- Words left in the wrong language after the repair passes, comma-separated.
    lang_issues      TEXT,
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

/// Lightweight chapter row for list views.
pub(crate) struct ChapterListRow {
    pub(crate) idx: usize,
    pub(crate) number: Option<usize>,
    pub(crate) title: String,
    pub(crate) translated_title: Option<String>,
    pub(crate) status: String,
    pub(crate) origin: Option<String>,
    pub(crate) lang_issues: Option<String>,
}

/// Complete chapter data used by the reader and translation coordinator.
pub(crate) struct ChapterRecord {
    pub(crate) number: Option<usize>,
    pub(crate) source_title: String,
    pub(crate) source: String,
    pub(crate) status: String,
    pub(crate) translated_title: Option<String>,
    pub(crate) translated: Option<String>,
    pub(crate) origin: Option<String>,
    pub(crate) user_prompt: Option<String>,
}

/// A chapter's searchable text.
pub(crate) struct SearchableChapter {
    pub(crate) idx: usize,
    pub(crate) number: Option<usize>,
    pub(crate) title: String,
    pub(crate) text: String,
}

/// Progress store on top of SQLite.
pub struct Store {
    conn: Connection,
}

impl Store {
    /// Open/create the database and apply the schema and migrations.
    ///
    /// Opening is **read-only in effect**: it never changes chapter data. Crash
    /// recovery is a separate, explicit step ([`Store::recover`]) precisely
    /// because commands open the database constantly to read it, and a read that
    /// rewrites statuses would erase the state of a run that is in flight.
    pub fn open(path: &str) -> Result<Self> {
        let conn = Connection::open(path)?;
        conn.execute_batch(PRAGMAS)?;
        register_ulower(&conn)?;
        conn.execute_batch(SCHEMA)?;
        let store = Store { conn };
        store.migrate()?;
        Ok(store)
    }

    /// Recover from a crash: chapters left `in_progress` by a process that died
    /// mid-translation go back to `pending`. Returns how many were reset.
    ///
    /// Called once when a project is opened and once when a job starts, never on
    /// a plain read. A chapter that is genuinely being translated right now is
    /// `in_progress`, and that status is what tells the UI not to let the user
    /// edit it under the translator's feet.
    pub fn recover(&self) -> Result<usize> {
        let n = self.conn.execute(
            "UPDATE chapters SET status = 'pending' WHERE status = 'in_progress'",
            [],
        )?;
        if n > 0 {
            tracing::info!(chapters = n, "recovered chapters left in_progress by a crash");
        }
        Ok(n)
    }

    /// Additive migrations for databases created by an older version.
    fn migrate(&self) -> Result<()> {
        // The glossary is read paged and ordered by frequency; on a book that
        // grows tens of thousands of terms, that ordering must not be a scan.
        self.conn.execute_batch(
            "CREATE INDEX IF NOT EXISTS glossary_by_frequency
             ON glossary (frequency DESC, source ASC);",
        )?;
        self.ensure_column("chapters", "origin", "TEXT")?;
        self.ensure_column("chapters", "rolling_summary", "TEXT")?;
        self.ensure_column("chapters", "prev_tail", "TEXT")?;
        self.ensure_column("chapters", "user_prompt", "TEXT")?;
        self.ensure_column("chapters", "translate_ms", "INTEGER")?;
        self.ensure_column("chapters", "lang_issues", "TEXT")?;
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


}


/// Register `ulower(x)`, a Unicode-aware lowercase for use in queries.
///
/// SQLite's built-in `lower()` and its `LIKE` both fold case for ASCII only, so
/// neither can match "ВАН" against "Ван" and a glossary filter would be useless
/// on exactly the scripts this tool translates between.
fn register_ulower(conn: &Connection) -> Result<()> {
    use rusqlite::functions::FunctionFlags;
    conn.create_scalar_function(
        "ulower",
        1,
        FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC,
        |ctx| {
            let s = ctx.get_raw(0).as_str_or_null()?;
            Ok(s.map(|s| s.to_lowercase()))
        },
    )?;
    Ok(())
}


#[cfg(test)]
mod tests {
    use super::*;

    use crate::book::Chapter;
    use crate::glossary::{Term, TermKind};

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

    /// A temp DB path unique to this test process and name.
    fn temp_db(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("bc_state_{}_{}.db", name, std::process::id()))
    }

    #[test]
    fn recover_requeues_in_progress() {
        let path = temp_db("recover");
        let path_str = path.to_str().unwrap();
        {
            let store = Store::open(path_str).unwrap();
            store.init_chapters(&sample()).unwrap();
            store.set_status(2, Status::InProgress).unwrap();
            assert_eq!(store.pending_chapters().unwrap(), vec![1, 3]);
        }
        {
            let store = Store::open(path_str).unwrap();
            assert_eq!(store.recover().unwrap(), 1);
            assert_eq!(store.pending_chapters().unwrap(), vec![1, 2, 3]);
        }
        let _ = std::fs::remove_file(path);
    }

    /// Reading the database must never rewrite it. A UI command opening the DB
    /// while a chapter is being translated used to reset that chapter to
    /// `pending`, which erased the status the editor lock is built on.
    #[test]
    fn open_leaves_in_progress_alone() {
        let path = temp_db("open_pure");
        let path_str = path.to_str().unwrap();
        let running = Store::open(path_str).unwrap();
        running.init_chapters(&sample()).unwrap();
        running.set_status(2, Status::InProgress).unwrap();

        // A concurrent reader, exactly as every Tauri command does it.
        let reader = Store::open(path_str).unwrap();
        assert_eq!(reader.stats().unwrap().in_progress, 1);
        assert_eq!(
            reader.chapter_full(2).unwrap().unwrap().status,
            "in_progress"
        );

        // The translating connection still sees its own chapter as in flight.
        assert_eq!(running.stats().unwrap().in_progress, 1);
        assert_eq!(running.pending_chapters().unwrap(), vec![1, 3]);

        drop(reader);
        drop(running);
        let _ = std::fs::remove_file(path);
    }

    /// WAL is what lets the UI read while the background job writes.
    #[test]
    fn open_enables_wal_and_a_busy_timeout() {
        let path = temp_db("pragmas");
        let path_str = path.to_str().unwrap();
        let store = Store::open(path_str).unwrap();
        let mode: String = store
            .conn
            .query_row("PRAGMA journal_mode", [], |r| r.get(0))
            .unwrap();
        assert_eq!(mode.to_lowercase(), "wal");
        let timeout: i64 = store
            .conn
            .query_row("PRAGMA busy_timeout", [], |r| r.get(0))
            .unwrap();
        assert!(timeout >= 5000, "busy_timeout was {timeout}");
        drop(store);
        for suffix in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{path_str}{suffix}"));
        }
    }

    /// The editor autosaves, so an empty save must be recognisable as one and
    /// never allowed to stand in for a translation.
    #[test]
    fn has_translation_distinguishes_empty_from_written() {
        let store = Store::open(":memory:").unwrap();
        store.init_chapters(&sample()).unwrap();
        assert!(!store.has_translation(1).unwrap());
        store.save_translation(1, "Глава 1", "Текст главы.").unwrap();
        assert!(store.has_translation(1).unwrap());
        store.save_manual_translation(2, "", "   ").unwrap();
        assert!(!store.has_translation(2).unwrap());
    }

    /// Everything the app needs from a reference after import is answerable from
    /// the chapters themselves, with no access to the reference file.
    #[test]
    fn reference_facts_come_from_the_chapters() {
        let store = Store::open(":memory:").unwrap();
        store.init_chapters(&sample()).unwrap();
        assert_eq!(store.reference_stats().unwrap(), (0, None));
        assert!(store.reference_pairs(10).unwrap().is_empty());

        let long = "Профессиональный перевод. ".repeat(20);
        assert!(store.save_reference_chapter(1, "Глава 1", &long).unwrap());
        assert!(store.save_reference_chapter(2, "Глава 2", "Короткая.").unwrap());

        assert_eq!(store.reference_stats().unwrap(), (2, Some(2)));
        let pairs = store.reference_pairs(10).unwrap();
        assert_eq!(pairs.len(), 2);
        assert_eq!(pairs[0].0, 1);
        assert_eq!(pairs[0].2, long);
        assert_eq!(store.reference_pairs(1).unwrap().len(), 1);
        // The style excerpt skips the chapter that is too short to be one.
        assert!(store.reference_style(50).unwrap().unwrap().starts_with("Профессиональный"));
    }

    /// A reset keeps the professional text, so re-seeding is restoring status.
    #[test]
    fn reference_chapters_survive_a_reset_and_can_be_restored() {
        let store = Store::open(":memory:").unwrap();
        store.init_chapters(&sample()).unwrap();
        store.save_reference_chapter(1, "Глава 1", "Текст.").unwrap();
        assert_eq!(store.stats().unwrap().done, 1);

        store.reset_from(None).unwrap();
        assert_eq!(store.stats().unwrap().done, 0);
        // The text is still there, which is why nothing needs re-importing.
        assert_eq!(store.reference_stats().unwrap().0, 1);

        assert_eq!(store.restore_reference_chapters().unwrap(), 1);
        assert_eq!(store.stats().unwrap().done, 1);
        assert_eq!(
            store.chapter_full(1).unwrap().unwrap().origin.unwrap(),
            "reference"
        );
    }

    fn glossary_sample() -> Vec<Term> {
        vec![
            Term { source: "王林".into(), target: "Ван Линь".into(), kind: TermKind::Person, frequency: 90, pinned: true },
            Term { source: "血湖".into(), target: "Кровавое озеро".into(), kind: TermKind::Location, frequency: 40, pinned: false },
            Term { source: "剑宗".into(), target: "Секта Меча".into(), kind: TermKind::Organization, frequency: 20, pinned: false },
            Term { source: "李慕婉".into(), target: "Ли Мувань".into(), kind: TermKind::Person, frequency: 10, pinned: false },
        ]
    }

    #[test]
    fn glossary_page_orders_by_frequency_and_windows() {
        let store = Store::open(":memory:").unwrap();
        store.save_glossary(&glossary_sample()).unwrap();

        let (total, first) = store.glossary_page("", None, 0, 2).unwrap();
        assert_eq!(total, 4);
        assert_eq!(first.len(), 2);
        assert_eq!(first[0].source, "王林");
        assert_eq!(first[1].source, "血湖");

        let (total, second) = store.glossary_page("", None, 2, 2).unwrap();
        assert_eq!(total, 4);
        assert_eq!(second[0].source, "剑宗");
        assert_eq!(second[1].source, "李慕婉");

        // Past the end is empty, not an error.
        let (_, past) = store.glossary_page("", None, 10, 2).unwrap();
        assert!(past.is_empty());
    }

    #[test]
    fn glossary_page_filters_both_sides_case_insensitively() {
        let store = Store::open(":memory:").unwrap();
        store.save_glossary(&glossary_sample()).unwrap();

        // Target side, and in a script SQLite's own LIKE would not fold.
        let (total, hits) = store.glossary_page("ВАН ЛИНЬ", None, 0, 50).unwrap();
        assert_eq!(total, 1);
        assert_eq!(hits[0].source, "王林");

        // Source side.
        let (total, _) = store.glossary_page("剑宗", None, 0, 50).unwrap();
        assert_eq!(total, 1);

        // Substring of a target.
        let (total, _) = store.glossary_page("озеро", None, 0, 50).unwrap();
        assert_eq!(total, 1);
    }

    #[test]
    fn glossary_page_filters_by_kind() {
        let store = Store::open(":memory:").unwrap();
        store.save_glossary(&glossary_sample()).unwrap();
        let (total, hits) = store.glossary_page("", Some("person"), 0, 50).unwrap();
        assert_eq!(total, 2);
        assert!(hits.iter().all(|t| t.kind == TermKind::Person));

        // Filter and kind compose: "Мувань" is one of the two persons.
        let (total, hits) = store.glossary_page("МУВАНЬ", Some("person"), 0, 50).unwrap();
        assert_eq!(total, 1);
        assert_eq!(hits[0].source, "李慕婉");

        // A term matching the query but not the kind is excluded.
        let (total, _) = store.glossary_page("Кровавое", Some("person"), 0, 50).unwrap();
        assert_eq!(total, 0);
    }

    /// A `%` typed into the filter box is a literal, not a wildcard.
    #[test]
    fn glossary_page_escapes_like_wildcards() {
        let store = Store::open(":memory:").unwrap();
        let mut terms = glossary_sample();
        terms.push(Term { source: "100%".into(), target: "сто процентов".into(), kind: TermKind::Term, frequency: 1, pinned: false });
        store.save_glossary(&terms).unwrap();

        let (total, hits) = store.glossary_page("100%", None, 0, 50).unwrap();
        assert_eq!(total, 1);
        assert_eq!(hits[0].source, "100%");

        // A lone "%" must not match everything.
        let (total, _) = store.glossary_page("%", None, 0, 50).unwrap();
        assert_eq!(total, 1);
    }

    #[test]
    fn upsert_term_touches_one_row() {
        let store = Store::open(":memory:").unwrap();
        let a = Term { source: "王林".into(), target: "Ван Линь".into(), kind: TermKind::Person, frequency: 3, pinned: false };
        let b = Term { source: "血湖".into(), target: "Кровавое озеро".into(), kind: TermKind::Location, frequency: 1, pinned: false };
        store.save_glossary(&[a.clone(), b.clone()]).unwrap();

        store
            .upsert_term(&Term { target: "Ван Линь (канон)".into(), pinned: true, ..a })
            .unwrap();

        let loaded = store.load_glossary().unwrap();
        assert_eq!(loaded.len(), 2);
        let wang = loaded.iter().find(|t| t.source == "王林").unwrap();
        assert_eq!(wang.target, "Ван Линь (канон)");
        assert!(wang.pinned);
        // The untouched term is exactly as it was.
        let lake = loaded.iter().find(|t| t.source == "血湖").unwrap();
        assert_eq!(lake.target, "Кровавое озеро");
        assert!(!lake.pinned);
    }

    #[test]
    fn term_reads_one_entry() {
        let store = Store::open(":memory:").unwrap();
        let t = Term { source: "王林".into(), target: "Ван Линь".into(), kind: TermKind::Person, frequency: 2, pinned: true };
        store.upsert_term(&t).unwrap();
        let got = store.term("王林").unwrap().unwrap();
        assert_eq!(got.target, "Ван Линь");
        assert_eq!(got.frequency, 2);
        assert!(got.pinned);
        assert!(store.term("нет такого").unwrap().is_none());
    }

    #[test]
    fn upsert_terms_writes_only_what_it_is_given() {
        let store = Store::open(":memory:").unwrap();
        let keep = Term { source: "血湖".into(), target: "Кровавое озеро".into(), kind: TermKind::Location, frequency: 1, pinned: true };
        store.save_glossary(std::slice::from_ref(&keep)).unwrap();

        let new_a = Term { source: "王林".into(), target: "Ван Линь".into(), kind: TermKind::Person, frequency: 4, pinned: false };
        let new_b = Term { source: "剑宗".into(), target: "Секта Меча".into(), kind: TermKind::Organization, frequency: 2, pinned: false };
        store.upsert_terms(&[&new_a, &new_b]).unwrap();

        let loaded = store.load_glossary().unwrap();
        assert_eq!(loaded.len(), 3);
        assert!(loaded.iter().find(|t| t.source == "血湖").unwrap().pinned);
        assert_eq!(loaded.iter().find(|t| t.source == "王林").unwrap().frequency, 4);
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
        assert_eq!(full.status, "done");
        assert_eq!(full.translated.as_deref(), Some("ручной текст"));
        assert_eq!(full.origin.as_deref(), Some("manual"));
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
            full.user_prompt.as_deref(),
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
