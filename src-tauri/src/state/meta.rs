//! Book-level key/value metadata: title, author, cover, format, encoding, and
//! the book-level mirror of the running summary.

use anyhow::Result;
use rusqlite::{params, OptionalExtension};

use super::Store;

#[derive(Debug, Clone, Default)]
pub(crate) struct ProjectMetadata {
    pub(crate) title: Option<String>,
    pub(crate) author: Option<String>,
    pub(crate) title_translated: Option<String>,
    pub(crate) author_translated: Option<String>,
    pub(crate) summary: Option<String>,
    pub(crate) format: Option<String>,
    pub(crate) encoding: Option<String>,
    pub(crate) cover_content_type: Option<String>,
    pub(crate) cover_base64: Option<String>,
    pub(crate) reference_style: Option<String>,
}

impl Store {
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

    /// Read the durable project metadata in one typed operation.
    pub(crate) fn project_metadata(&self) -> Result<ProjectMetadata> {
        let get = |key: &str| {
            self.get_meta(key)
                .map(|value| value.filter(|v| !v.trim().is_empty()))
        };
        Ok(ProjectMetadata {
            title: get("title")?,
            author: get("author")?,
            title_translated: get("title_translated")?,
            author_translated: get("author_translated")?,
            summary: get("summary")?,
            format: get("format")?,
            encoding: get("encoding")?,
            cover_content_type: get("cover_ct")?,
            cover_base64: get("cover_b64")?,
            reference_style: get("ref_style")?,
        })
    }

    /// Persist the source-level metadata atomically.
    pub(crate) fn set_source_metadata(
        &self,
        title: &str,
        author: &str,
        format: &str,
        encoding: &str,
    ) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        for (key, value) in [
            ("title", title),
            ("author", author),
            ("format", format),
            ("encoding", encoding),
        ] {
            tx.execute(
                "INSERT INTO meta (key, value) VALUES (?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                params![key, value],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Persist or clear cover metadata atomically.
    pub(crate) fn set_cover_meta(
        &self,
        content_type: Option<&str>,
        base64: Option<&str>,
    ) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        match (content_type, base64) {
            (Some(content_type), Some(base64)) => {
                for (key, value) in [("cover_ct", content_type), ("cover_b64", base64)] {
                    tx.execute(
                        "INSERT INTO meta (key, value) VALUES (?1, ?2)
                         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                        params![key, value],
                    )?;
                }
            }
            _ => {
                tx.execute("DELETE FROM meta WHERE key IN ('cover_ct', 'cover_b64')", [])?;
            }
        }
        tx.commit()?;
        Ok(())
    }
}
