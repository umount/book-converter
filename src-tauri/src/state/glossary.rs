//! The glossary table: canonical renderings of names, places and terminology.
//!
//! Filtering, ordering and windowing happen here rather than in the UI: a long
//! book's glossary reaches tens of thousands of terms, and shipping all of them
//! to the frontend to filter is what made the glossary view unusable.

use anyhow::Result;
use rusqlite::params;

use crate::glossary::{Term, TermKind};

use super::Store;

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

/// Escape the wildcards SQL `LIKE` would otherwise interpret in user input.
fn escape_like(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

impl Store {
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

    /// One page of the glossary, filtered and ordered in SQL.
    ///
    /// `query` matches either side of a term, `kind` narrows to one category
    /// (both are ignored when empty). Returns `(total_matching, page)` so the UI
    /// can show the real size of a filter it is only rendering a window of.
    ///
    /// Matching is case-insensitive by lowercasing both sides. SQLite's own
    /// `LIKE` folds case for ASCII only, which would make a filter useless on
    /// exactly the scripts this tool works in.
    pub fn glossary_page(
        &self,
        query: &str,
        kind: Option<&str>,
        offset: usize,
        limit: usize,
    ) -> Result<(usize, Vec<Term>)> {
        let q = query.trim().to_lowercase();
        let pattern = format!("%{}%", escape_like(&q));
        let has_query = !q.is_empty();
        let kind = kind.filter(|k| !k.trim().is_empty());

        let where_sql = "WHERE (?1 = 0 OR ulower(source) LIKE ?2 ESCAPE '\\' \
                                     OR ulower(target) LIKE ?2 ESCAPE '\\')
                           AND (?3 IS NULL OR kind = ?3)";

        let total: i64 = self.conn.query_row(
            &format!("SELECT COUNT(*) FROM glossary {where_sql}"),
            params![has_query as i64, pattern, kind],
            |r| r.get(0),
        )?;

        let mut stmt = self.conn.prepare(&format!(
            "SELECT source, target, kind, frequency, pinned FROM glossary {where_sql}
             ORDER BY frequency DESC, source ASC
             LIMIT ?4 OFFSET ?5"
        ))?;
        let rows = stmt
            .query_map(
                params![has_query as i64, pattern, kind, limit as i64, offset as i64],
                |r| {
                    Ok(Term {
                        source: r.get(0)?,
                        target: r.get(1)?,
                        kind: kind_from_str(&r.get::<_, String>(2)?),
                        frequency: r.get::<_, i64>(3)? as u32,
                        pinned: r.get::<_, i64>(4)? != 0,
                    })
                },
            )?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok((total as usize, rows))
    }

    /// Upsert one glossary entry.
    ///
    /// The single-term counterpart of [`Store::save_glossary`]. Editing one term
    /// from the UI used to load every term and write every term back, which on a
    /// 10K-entry glossary is 10K statements per keystroke-driven save.
    pub fn upsert_term(&self, term: &Term) -> Result<()> {
        self.conn.execute(
            "INSERT INTO glossary (source, target, kind, frequency, pinned)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(source) DO UPDATE SET
                target = excluded.target,
                kind = excluded.kind,
                frequency = excluded.frequency,
                pinned = excluded.pinned",
            params![
                term.source,
                term.target,
                kind_to_str(term.kind),
                term.frequency as i64,
                term.pinned as i64,
            ],
        )?;
        Ok(())
    }

    /// Upsert several glossary entries in one transaction.
    ///
    /// Used to flush what a translation run learned: only the rows that changed,
    /// rather than the whole term list.
    pub fn upsert_terms(&self, terms: &[&Term]) -> Result<()> {
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

    /// Read one glossary entry by its source form.
    pub fn term(&self, source: &str) -> Result<Option<Term>> {
        let mut stmt = self.conn.prepare(
            "SELECT source, target, kind, frequency, pinned FROM glossary WHERE source = ?1",
        )?;
        let mut rows = stmt.query_map(params![source], |r| {
            Ok(Term {
                source: r.get(0)?,
                target: r.get(1)?,
                kind: kind_from_str(&r.get::<_, String>(2)?),
                frequency: r.get::<_, i64>(3)? as u32,
                pinned: r.get::<_, i64>(4)? != 0,
            })
        })?;
        Ok(rows.next().transpose()?)
    }

    /// Remove a single glossary entry by its source term.
    pub fn delete_term(&self, source: &str) -> Result<()> {
        self.conn
            .execute("DELETE FROM glossary WHERE source = ?1", params![source])?;
        Ok(())
    }
}
