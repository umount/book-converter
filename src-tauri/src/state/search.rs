//! Reading and rewriting chapter text in bulk: what book-wide search and
//! find/replace across the whole book need.

use anyhow::Result;
use rusqlite::params;

use super::{SearchableChapter, Store};

impl Store {
    /// Chapters with their searchable text, in reading order. `in_source` picks
    /// the original instead of the translation; chapters with no text are skipped.
    pub fn searchable_chapters(&self, in_source: bool) -> Result<Vec<SearchableChapter>> {
        let sql = if in_source {
            "SELECT idx, number, title, source FROM chapters ORDER BY idx"
        } else {
            "SELECT idx, number, COALESCE(translated_title, title), translated
             FROM chapters WHERE translated IS NOT NULL ORDER BY idx"
        };
        let mut stmt = self.conn.prepare(sql)?;
        let rows = stmt
            .query_map([], |r| {
                Ok(SearchableChapter {
                    idx: r.get::<_, i64>(0)? as usize,
                    number: r.get::<_, Option<i64>>(1)?.map(|n| n as usize),
                    title: r.get(2)?,
                    text: r.get(3)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Find/replace across every stored translation (title + body). Returns the
    /// number of chapters actually changed. Status and origin are left untouched
    /// (this is a text edit, not a re-translation).
    ///
    /// `expand` mirrors the find bar's regex mode: with it, `$1` in `replacement`
    /// refers to a capture group; without it the replacement is inserted verbatim,
    /// so a literal `$` in the text stays a `$`.
    pub fn replace_in_translations(
        &self,
        re: &regex::Regex,
        replacement: &str,
        expand: bool,
    ) -> Result<usize> {
        let mut stmt = self.conn.prepare(
            "SELECT idx, translated_title, translated FROM chapters WHERE translated IS NOT NULL",
        )?;
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, i64>(0)? as usize,
                    r.get::<_, Option<String>>(1)?,
                    r.get::<_, String>(2)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        let apply = |s: &str| {
            if expand {
                re.replace_all(s, replacement).into_owned()
            } else {
                re.replace_all(s, regex::NoExpand(replacement)).into_owned()
            }
        };

        let mut changed = 0usize;
        for (idx, title, body) in rows {
            let new_body = apply(&body);
            let new_title = title.as_ref().map(|t| apply(t));
            if new_body != body || new_title.as_deref() != title.as_deref() {
                self.conn.execute(
                    "UPDATE chapters
                     SET translated = ?2,
                         translated_title = COALESCE(?3, translated_title),
                         updated_at = datetime('now')
                     WHERE idx = ?1",
                    params![idx as i64, new_body, new_title],
                )?;
                changed += 1;
            }
        }
        Ok(changed)
    }
}
