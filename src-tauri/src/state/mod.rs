//! Персистентность прогресса перевода в SQLite.
//!
//! Критично для книги на ~990 глав: процесс можно прервать и продолжить
//! с места. Хранит статус каждой главы, её перевод и глоссарий.
//!
//! ## Схема (набросок)
//! - `chapters(index INTEGER PK, title TEXT, source TEXT, status TEXT,
//!    translated TEXT, updated_at)` — status: pending | in_progress | done | failed
//! - `glossary(source TEXT PK, target TEXT, kind TEXT, frequency INT, pinned INT)`
//! - `meta(key TEXT PK, value TEXT)` — путь к книге, настройки прогона и т.п.

use crate::book::Chapter;
use crate::glossary::Term;

/// Статус перевода главы.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Pending,
    InProgress,
    Done,
    Failed,
}

/// Хранилище прогресса поверх SQLite.
pub struct Store {
    _conn: rusqlite::Connection,
}

impl Store {
    /// Открыть/создать базу и применить схему.
    pub fn open(_path: &str) -> anyhow::Result<Self> {
        todo!("открытие SQLite и создание таблиц")
    }

    /// Загрузить главы из книги в базу (idempotent — не перетирает переводы).
    pub fn init_chapters(&self, _chapters: &[Chapter]) -> anyhow::Result<()> {
        todo!("вставка глав со статусом pending")
    }

    /// Вернуть индексы глав, ещё не переведённых (для возобновления).
    pub fn pending_chapters(&self) -> anyhow::Result<Vec<usize>> {
        todo!("выборка глав со статусом pending/failed")
    }

    /// Сохранить перевод главы и пометить как done.
    pub fn save_translation(&self, _index: usize, _translated: &str) -> anyhow::Result<()> {
        todo!("запись перевода и статуса done")
    }

    /// Обновить статус главы.
    pub fn set_status(&self, _index: usize, _status: Status) -> anyhow::Result<()> {
        todo!("обновление статуса главы")
    }

    /// Загрузить весь глоссарий.
    pub fn load_glossary(&self) -> anyhow::Result<Vec<Term>> {
        todo!("чтение глоссария")
    }

    /// Сохранить (upsert) глоссарий.
    pub fn save_glossary(&self, _terms: &[Term]) -> anyhow::Result<()> {
        todo!("upsert глоссария")
    }
}
