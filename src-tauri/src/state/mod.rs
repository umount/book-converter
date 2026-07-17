//! Persisting translation progress in SQLite.
//!
//! Critical for a ~990-chapter book: the process can be interrupted and resumed
//! from where it left off. Stores each chapter's status, its translation, and
//! the glossary.
//!
//! ## Schema (sketch)
//! - `chapters(index INTEGER PK, title TEXT, source TEXT, status TEXT,
//!    translated TEXT, updated_at)` — status: pending | in_progress | done | failed
//! - `glossary(source TEXT PK, target TEXT, kind TEXT, frequency INT, pinned INT)`
//! - `meta(key TEXT PK, value TEXT)` — book path, run settings, etc.

use crate::book::Chapter;
use crate::glossary::Term;

/// Chapter translation status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Pending,
    InProgress,
    Done,
    Failed,
}

/// Progress store on top of SQLite.
pub struct Store {
    _conn: rusqlite::Connection,
}

impl Store {
    /// Open/create the database and apply the schema.
    pub fn open(_path: &str) -> anyhow::Result<Self> {
        todo!("open SQLite and create the tables")
    }

    /// Load the book's chapters into the database (idempotent — never clobbers translations).
    pub fn init_chapters(&self, _chapters: &[Chapter]) -> anyhow::Result<()> {
        todo!("insert chapters with status pending")
    }

    /// Return the indices of chapters not yet translated (for resumption).
    pub fn pending_chapters(&self) -> anyhow::Result<Vec<usize>> {
        todo!("select chapters with status pending/failed")
    }

    /// Save a chapter's translation and mark it done.
    pub fn save_translation(&self, _index: usize, _translated: &str) -> anyhow::Result<()> {
        todo!("write the translation and status done")
    }

    /// Update a chapter's status.
    pub fn set_status(&self, _index: usize, _status: Status) -> anyhow::Result<()> {
        todo!("update the chapter status")
    }

    /// Load the whole glossary.
    pub fn load_glossary(&self) -> anyhow::Result<Vec<Term>> {
        todo!("read the glossary")
    }

    /// Save (upsert) the glossary.
    pub fn save_glossary(&self, _terms: &[Term]) -> anyhow::Result<()> {
        todo!("upsert the glossary")
    }
}
