//! Typed chapter content: the block rows a chapter is made of, and the images
//! they point at.
//!
//! Only chapters that are *not* plain prose get rows here. A prose chapter's
//! single block is its own `chapters.source`, and storing that twice would
//! double the database on a book-length text — so an empty block list means
//! "this chapter is exactly its text".

use anyhow::Result;
use rusqlite::{params, OptionalExtension};

use crate::book::{ChapterBlocks, ChapterKind};

use super::{AssetRow, BlockRow, Store};

impl Store {
    /// Record the images a project owns. Idempotent: re-importing the same book
    /// re-points the same content ids at the same files.
    pub fn save_assets(&self, assets: &[AssetRow]) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        {
            let mut stmt = tx.prepare(
                "INSERT INTO assets (id, rel_path, content_type, bytes, width, height)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT(id) DO UPDATE SET
                     rel_path = excluded.rel_path,
                     content_type = excluded.content_type,
                     bytes = excluded.bytes,
                     width = COALESCE(excluded.width, width),
                     height = COALESCE(excluded.height, height)",
            )?;
            for asset in assets {
                stmt.execute(params![
                    asset.id,
                    asset.rel_path,
                    asset.content_type,
                    asset.bytes as i64,
                    asset.width,
                    asset.height,
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Load the blocks of chapters that have typed content, plus each chapter's
    /// kind.
    ///
    /// Idempotent in the same way as [`Store::init_chapters`]: an existing block
    /// row is kept, because it may already carry a translation (a caption, and
    /// later the text lifted off a manga page).
    pub fn init_chapter_blocks(&self, chapters: &[ChapterBlocks]) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        {
            let mut block = tx.prepare(
                "INSERT INTO chapter_blocks (chapter_idx, ord, kind, text, asset_id)
                 VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(chapter_idx, ord) DO NOTHING",
            )?;
            let mut kind = tx.prepare("UPDATE chapters SET kind = ?2 WHERE idx = ?1")?;
            // A page with no words is taken out of the queue right here, so it
            // never costs an API call and never reads as unfinished work.
            let mut skip = tx.prepare(
                "UPDATE chapters SET status = 'skipped', updated_at = datetime('now')
                 WHERE idx = ?1 AND status = 'pending'",
            )?;
            for chapter in chapters {
                kind.execute(params![chapter.chapter_index as i64, chapter.kind.as_str()])?;
                if !chapter.kind.is_translatable() {
                    skip.execute(params![chapter.chapter_index as i64])?;
                }
                for (ord, b) in chapter.blocks.iter().enumerate() {
                    block.execute(params![
                        chapter.chapter_index as i64,
                        ord as i64,
                        b.kind.as_str(),
                        (!b.text.is_empty()).then_some(&b.text),
                        b.asset_id,
                    ])?;
                }
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Blocks of one chapter in document order, with their images resolved.
    /// Empty for a prose chapter.
    pub fn chapter_blocks(&self, index: usize) -> Result<Vec<BlockRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT b.ord, b.kind, b.text, b.translated,
                    a.rel_path, a.width, a.height
             FROM chapter_blocks b
             LEFT JOIN assets a ON a.id = b.asset_id
             WHERE b.chapter_idx = ?1
             ORDER BY b.ord",
        )?;
        let rows = stmt
            .query_map(params![index as i64], |r| {
                Ok(BlockRow {
                    ord: r.get::<_, i64>(0)? as usize,
                    kind: r.get(1)?,
                    text: r.get(2)?,
                    translated: r.get(3)?,
                    rel_path: r.get(4)?,
                    width: r.get::<_, Option<i64>>(5)?.map(|n| n as u32),
                    height: r.get::<_, Option<i64>>(6)?.map(|n| n as u32),
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Every image the project owns.
    pub fn assets(&self) -> Result<Vec<AssetRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, rel_path, content_type, bytes, width, height FROM assets ORDER BY id",
        )?;
        let rows = stmt
            .query_map([], |r| {
                Ok(AssetRow {
                    id: r.get(0)?,
                    rel_path: r.get(1)?,
                    content_type: r.get(2)?,
                    bytes: r.get::<_, i64>(3)? as u64,
                    width: r.get::<_, Option<i64>>(4)?.map(|n| n as u32),
                    height: r.get::<_, Option<i64>>(5)?.map(|n| n as u32),
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Chapters an export should contain: everything translated, plus pages of
    /// pictures that have no words to translate — they are part of the book, and
    /// leaving them out would silently drop a manga volume's actual content.
    ///
    /// Returns `(idx, title, body)` like [`Store::translated_chapters`]; the body
    /// of a picture page is its marker text.
    pub fn chapters_for_export(&self) -> Result<Vec<(usize, String, String)>> {
        let mut stmt = self.conn.prepare(
            "SELECT idx,
                    COALESCE(NULLIF(TRIM(translated_title), ''), title),
                    CASE
                        WHEN translated IS NOT NULL AND TRIM(translated) != '' THEN translated
                        ELSE source
                    END
             FROM chapters
             WHERE (status = 'done' AND translated IS NOT NULL AND TRIM(translated) != '')
                OR COALESCE(kind, 'text') = 'image'
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

    /// What one chapter is made of (prose when nothing was recorded).
    pub fn chapter_kind(&self, index: usize) -> Result<ChapterKind> {
        let kind: Option<String> = self
            .conn
            .query_row(
                "SELECT kind FROM chapters WHERE idx = ?1",
                params![index as i64],
                |r| r.get(0),
            )
            .optional()?
            .flatten();
        Ok(kind
            .as_deref()
            .map(ChapterKind::from_str)
            .unwrap_or(ChapterKind::Text))
    }

    /// Whether any chapter has typed content, i.e. whether this project was
    /// imported after blocks existed. Drives the one-off backfill on open.
    pub fn has_chapter_blocks(&self) -> Result<bool> {
        let n: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM chapter_blocks", [], |r| r.get(0))?;
        Ok(n > 0)
    }

}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::book::blocks::Block;
    use crate::book::Chapter;

    fn chapters() -> Vec<Chapter> {
        vec![
            Chapter {
                index: 1,
                number: Some(1),
                title: "Chapter 1".into(),
                body: "prose".into(),
            },
            Chapter {
                index: 2,
                number: Some(2),
                title: "Chapter 2".into(),
                body: "[[img:ab12]]".into(),
            },
        ]
    }

    fn store() -> Store {
        let store = Store::open(":memory:").unwrap();
        store.init_chapters(&chapters()).unwrap();
        store
            .save_assets(&[AssetRow {
                id: "ab12".into(),
                rel_path: "assets/ab12.jpg".into(),
                content_type: "image/jpeg".into(),
                bytes: 1024,
                width: Some(800),
                height: Some(1200),
            }])
            .unwrap();
        store
            .init_chapter_blocks(&[ChapterBlocks {
                chapter_index: 2,
                kind: ChapterKind::Image,
                blocks: vec![Block::image("ab12")],
            }])
            .unwrap();
        store
    }

    #[test]
    fn blocks_roundtrip_with_their_asset() {
        let store = store();
        let rows = store.chapter_blocks(2).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].kind, "image");
        assert_eq!(rows[0].rel_path.as_deref(), Some("assets/ab12.jpg"));
        assert_eq!(rows[0].width, Some(800));
        assert_eq!(store.chapter_kind(2).unwrap(), ChapterKind::Image);
        assert!(store.has_chapter_blocks().unwrap());
    }

    /// A prose chapter stores no rows: reading it back is reading its text.
    #[test]
    fn prose_chapters_store_nothing() {
        let store = store();
        assert!(store.chapter_blocks(1).unwrap().is_empty());
        assert_eq!(store.chapter_kind(1).unwrap(), ChapterKind::Text);
    }

    /// Re-importing a book must not wipe a block translation. Nothing writes
    /// one yet (a caption is translated as part of the chapter's text), but the
    /// column is what reading words off a page will fill in, and an import must
    /// not be able to throw that away.
    #[test]
    fn re_init_keeps_block_translations() {
        let store = store();
        store
            .conn
            .execute(
                "UPDATE chapter_blocks SET translated = 'Подпись'
                 WHERE chapter_idx = 2 AND ord = 0",
                [],
            )
            .unwrap();
        store
            .init_chapter_blocks(&[ChapterBlocks {
                chapter_index: 2,
                kind: ChapterKind::Image,
                blocks: vec![Block::image("ab12")],
            }])
            .unwrap();
        assert_eq!(
            store.chapter_blocks(2).unwrap()[0].translated.as_deref(),
            Some("Подпись")
        );
    }

    /// A page of pictures leaves the queue, and progress is measured against
    /// what can actually be translated.
    #[test]
    fn an_image_chapter_is_skipped_not_pending() {
        let store = store();
        assert_eq!(store.pending_chapters().unwrap(), vec![1]);
        let stats = store.stats().unwrap();
        assert_eq!((stats.total, stats.pending, stats.skipped), (2, 1, 1));
    }

    /// Re-translating the book must not queue the pictures back up.
    #[test]
    fn a_reset_leaves_skipped_chapters_alone() {
        let store = store();
        store.save_translation(1, "Chapter 1", "перевод").unwrap();
        assert_eq!(store.reset_from(None).unwrap(), 1);
        assert_eq!(store.pending_chapters().unwrap(), vec![1]);
        assert_eq!(store.stats().unwrap().skipped, 1);
    }

    /// Two names for the same bytes are one asset row.
    #[test]
    fn saving_the_same_asset_twice_updates_it() {
        let store = store();
        store
            .save_assets(&[AssetRow {
                id: "ab12".into(),
                rel_path: "assets/ab12.png".into(),
                content_type: "image/png".into(),
                bytes: 2048,
                width: None,
                height: None,
            }])
            .unwrap();
        let rows = store.chapter_blocks(2).unwrap();
        assert_eq!(rows[0].rel_path.as_deref(), Some("assets/ab12.png"));
        // Dimensions already known are not lost to a later write without them.
        assert_eq!(rows[0].width, Some(800));
    }
}
