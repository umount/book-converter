//! Domain-scoped persistence for version-1 projects. No legacy chapter indices.
use crate::app::contracts::{
    AppError, BookBlockContent, BookBlockView, ErrorCode, ProjectKind, Revision,
};
use crate::app::requests::{BookChapterView, ChapterSummary, PageSummary, TranslationSummary};
use rusqlite::{params, Connection, OptionalExtension};

pub(super) fn failure(code: ErrorCode, key: &str) -> AppError {
    AppError {
        code,
        message_key: key.into(),
        params: Default::default(),
        retryable: false,
    }
}

pub(crate) fn storage_error(_: rusqlite::Error) -> AppError {
    failure(ErrorCode::Storage, "errors.storage")
}

pub(crate) fn not_found() -> AppError {
    failure(ErrorCode::NotFound, "errors.entityNotFound")
}
pub(crate) fn conflict() -> AppError {
    failure(ErrorCode::RevisionConflict, "errors.revisionConflict")
}

pub struct ProjectRepository<'a> {
    connection: &'a mut Connection,
    kind: ProjectKind,
}

impl<'a> ProjectRepository<'a> {
    pub fn new(
        connection: &'a mut Connection,
        expected_kind: ProjectKind,
    ) -> Result<Self, AppError> {
        let actual: String = connection
            .query_row(
                "SELECT kind FROM project_settings WHERE singleton=1",
                [],
                |r| r.get(0),
            )
            .map_err(storage_error)?;
        let expected = match expected_kind {
            ProjectKind::Book => "book",
            ProjectKind::Manga => "manga",
        };
        if actual != expected {
            return Err(failure(
                ErrorCode::WrongProjectKind,
                "errors.wrongProjectKind",
            ));
        }
        Ok(Self {
            connection,
            kind: expected_kind,
        })
    }

    fn require(&self, kind: ProjectKind) -> Result<(), AppError> {
        if self.kind != kind {
            return Err(failure(
                ErrorCode::WrongProjectKind,
                "errors.wrongProjectKind",
            ));
        }
        Ok(())
    }

    /// Import a chapter and all its structural blocks atomically.
    pub fn insert_chapter(
        &mut self,
        chapter: &ChapterSummary,
        blocks: &[BookBlockView],
    ) -> Result<(), AppError> {
        self.require(ProjectKind::Book)?;
        if chapter.revision.value()? != 0
            || blocks.iter().any(|b| {
                b.chapter_id != chapter.id.0 || b.revision.0 != "0" || b.translated_text.is_some()
            })
        {
            return Err(AppError::invalid("chapter"));
        }
        let tx = self.connection.transaction().map_err(storage_error)?;
        tx.execute(
            "INSERT INTO book_chapters(id,position,source_title) VALUES(?1,?2,?3)",
            params![chapter.id.0, chapter.position, chapter.title],
        )
        .map_err(storage_error)?;
        for block in blocks {
            let (kind, text, asset, alt) = match &block.content {
                BookBlockContent::Text { text } => ("text", Some(text.as_str()), None, ""),
                BookBlockContent::Caption { text } => ("caption", Some(text.as_str()), None, ""),
                BookBlockContent::Image { asset_id, alt } => {
                    ("image", None, Some(asset_id.as_str()), alt.as_str())
                }
            };
            tx.execute("INSERT INTO book_source_blocks(id,chapter_id,position,kind,text,asset_id,alt) VALUES(?1,?2,?3,?4,?5,?6,?7)",params![block.id,chapter.id.0,block.position,kind,text,asset,alt]).map_err(storage_error)?;
        }
        tx.commit().map_err(storage_error)
    }

    /// Read chapter metadata and its blocks from the same database snapshot.
    pub fn chapter(&mut self, id: &str) -> Result<BookChapterView, AppError> {
        self.require(ProjectKind::Book)?;
        let tx = self.connection.transaction().map_err(storage_error)?;
        let chapter = tx
            .query_row(
                "SELECT id,position,source_title,revision FROM book_chapters WHERE id=?1",
                [id],
                |r| {
                    Ok(ChapterSummary {
                        id: crate::app::contracts::ChapterId(r.get(0)?),
                        position: r.get(1)?,
                        title: r.get(2)?,
                        revision: Revision(r.get::<_, i64>(3)?.to_string()),
                    })
                },
            )
            .optional()
            .map_err(storage_error)?
            .ok_or_else(not_found)?;
        let translation=tx.query_row("SELECT id,revision,translated_title,status FROM book_translations WHERE chapter_id=?1 AND target_language=(SELECT target_language FROM project_settings WHERE singleton=1) ORDER BY revision DESC LIMIT 1",[id],|r|Ok(TranslationSummary{id:r.get(0)?,revision:Revision(r.get::<_,i64>(1)?.to_string()),title:r.get(2)?,status:r.get(3)?})).optional().map_err(storage_error)?;
        let translated: std::collections::HashMap<String, String> = if let Some(translation) =
            &translation
        {
            let mut query=tx.prepare("SELECT source_block_id,translated_text FROM book_translation_blocks WHERE translation_id=?1").map_err(storage_error)?;
            let rows = query
                .query_map([&translation.id], |r| Ok((r.get(0)?, r.get(1)?)))
                .map_err(storage_error)?;
            rows.collect::<Result<_, _>>().map_err(storage_error)?
        } else {
            Default::default()
        };
        let blocks = {
            let mut query = tx.prepare("SELECT id,position,kind,text,asset_id,alt,revision FROM book_source_blocks WHERE chapter_id=?1 ORDER BY position").map_err(storage_error)?;
            let rows = query
                .query_map([id], |r| {
                    let kind: String = r.get(2)?;
                    let content = match kind.as_str() {
                        "text" => BookBlockContent::Text { text: r.get(3)? },
                        "caption" => BookBlockContent::Caption { text: r.get(3)? },
                        "image" => BookBlockContent::Image {
                            asset_id: r.get(4)?,
                            alt: r.get(5)?,
                        },
                        _ => return Err(rusqlite::Error::InvalidQuery),
                    };
                    Ok(BookBlockView {
                        id: r.get(0)?,
                        chapter_id: id.into(),
                        position: r.get(1)?,
                        revision: Revision(r.get::<_, i64>(6)?.to_string()),
                        content,
                        translated_text: translated.get(&r.get::<_, String>(0)?).cloned(),
                    })
                })
                .map_err(storage_error)?;
            rows.collect::<Result<Vec<_>, _>>().map_err(storage_error)?
        };
        let instructions = tx
            .query_row(
                "SELECT instructions FROM book_chapters WHERE id=?1",
                [id],
                |r| r.get::<_, String>(0),
            )
            .map_err(storage_error)?;
        tx.commit().map_err(storage_error)?;
        Ok(BookChapterView {
            instructions,
            chapter,
            blocks,
            translation,
        })
    }

    pub fn update_chapter_instructions(
        &mut self,
        id: &str,
        expected: &Revision,
        instructions: &str,
    ) -> Result<Revision, AppError> {
        self.require(ProjectKind::Book)?;
        if instructions.len() > 32768 {
            return Err(AppError::invalid("instructions"));
        }
        let tx = self.connection.transaction().map_err(storage_error)?;
        let revision = super::shared::next(expected.value()?)?;
        let changed = tx
            .execute(
                "UPDATE book_chapters SET instructions=?1,revision=?2 WHERE id=?3 AND revision=?4",
                params![instructions, revision, id, expected.value()?],
            )
            .map_err(storage_error)?;
        if changed != 1 {
            return Err(super::shared::missing_or_conflict(
                &tx,
                "SELECT 1 FROM book_chapters WHERE id=?1",
                id,
            )?);
        }
        tx.execute("UPDATE book_translations SET status='needs_review' WHERE status='ready' AND chapter_id IN (SELECT id FROM book_chapters WHERE position >= (SELECT position FROM book_chapters WHERE id=?1))",[id]).map_err(storage_error)?;
        tx.commit().map_err(storage_error)?;
        Ok(Revision(revision.to_string()))
    }

    pub fn update_book_text(
        &mut self,
        id: &str,
        expected: &Revision,
        text: &str,
    ) -> Result<Revision, AppError> {
        self.require(ProjectKind::Book)?;
        let expected = expected.value()?;
        let next = expected
            .checked_add(1)
            .ok_or_else(|| AppError::invalid("revision"))?;
        let tx = self.connection.transaction().map_err(storage_error)?;
        let chapter: Option<String> = tx.query_row("UPDATE book_source_blocks SET text=?1,revision=revision+1 WHERE id=?2 AND revision=?3 AND kind IN ('text','caption') RETURNING chapter_id",params![text,id,expected],|r|r.get(0)).optional().map_err(storage_error)?;
        let Some(chapter) = chapter else {
            let kind: Option<String> = tx
                .query_row(
                    "SELECT kind FROM book_source_blocks WHERE id=?1",
                    [id],
                    |r| r.get(0),
                )
                .optional()
                .map_err(storage_error)?;
            return Err(match kind {
                None => not_found(),
                Some(kind) if kind == "image" => AppError::invalid("blockKind"),
                _ => conflict(),
            });
        };
        tx.execute(
            "UPDATE book_chapters SET revision=revision+1 WHERE id=?1",
            [&chapter],
        )
        .map_err(storage_error)?;
        tx.execute(
            "UPDATE book_translations SET status='stale' WHERE chapter_id=?1",
            [&chapter],
        )
        .map_err(storage_error)?;
        tx.execute("UPDATE book_translations SET status='needs_review' WHERE status='ready' AND chapter_id IN (SELECT id FROM book_chapters WHERE position > (SELECT position FROM book_chapters WHERE id=?1))",[&chapter]).map_err(storage_error)?;
        tx.commit().map_err(storage_error)?;
        Ok(Revision(next.to_string()))
    }

    pub fn insert_volume(
        &mut self,
        id: &str,
        position: u32,
        title: &str,
        rtl: bool,
    ) -> Result<(), AppError> {
        self.require(ProjectKind::Manga)?;
        self.connection.execute("INSERT INTO manga_volumes(id,position,title,reading_direction) VALUES(?1,?2,?3,?4)",params![id,position,title,if rtl { "rtl" } else { "ltr" }]).map_err(storage_error)?;
        Ok(())
    }

    pub fn insert_page(&mut self, page: &PageSummary) -> Result<(), AppError> {
        self.require(ProjectKind::Manga)?;
        if page.revision.value()? != 0 {
            return Err(AppError::invalid("revision"));
        }
        let tx = self.connection.transaction().map_err(storage_error)?;
        // Dimension equality is checked inside the insert, not with a racy preflight read.
        let count = tx.execute("INSERT INTO manga_pages(id,volume_id,position,original_asset_id,width,height) SELECT ?1,?2,?3,id,?5,?6 FROM assets WHERE id=?4 AND width=?5 AND height=?6",params![page.id.0,page.volume_id.0,page.position,page.original_asset_id.0,page.width,page.height]).map_err(storage_error)?;
        if count != 1 {
            return Err(AppError::invalid("originalAsset"));
        }
        if let Some(thumbnail) = &page.thumbnail_asset_id {
            tx.execute(
                "INSERT INTO manga_page_previews(page_id,asset_id) VALUES(?1,?2)",
                params![page.id.0, thumbnail.0],
            )
            .map_err(storage_error)?;
        }
        tx.commit().map_err(storage_error)
    }

    pub fn pages(&self, volume_id: &str) -> Result<Vec<PageSummary>, AppError> {
        self.require(ProjectKind::Manga)?;
        let mut query = self.connection.prepare("SELECT id,volume_id,position,original_asset_id,width,height,revision,(SELECT asset_id FROM manga_page_previews WHERE page_id=manga_pages.id) FROM manga_pages WHERE volume_id=?1 ORDER BY position").map_err(storage_error)?;
        let rows = query
            .query_map([volume_id], |r| {
                Ok(PageSummary {
                    id: crate::app::contracts::PageId(r.get(0)?),
                    volume_id: crate::app::contracts::VolumeId(r.get(1)?),
                    position: r.get(2)?,
                    original_asset_id: crate::app::contracts::AssetId(r.get(3)?),
                    thumbnail_asset_id: r
                        .get::<_, Option<String>>(7)?
                        .map(crate::app::contracts::AssetId),
                    width: r.get(4)?,
                    height: r.get(5)?,
                    revision: Revision(r.get::<_, i64>(6)?.to_string()),
                })
            })
            .map_err(storage_error)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(storage_error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::contracts::{AssetId, ChapterId, PageId, VolumeId};

    fn fixture(kind: ProjectKind) -> Connection {
        super::super::tests::database(kind)
    }

    fn block(id: &str, position: u32, content: BookBlockContent) -> BookBlockView {
        BookBlockView {
            id: id.into(),
            chapter_id: "chapter".into(),
            position,
            revision: Revision("0".into()),
            content,
            translated_text: None,
        }
    }

    #[test]
    fn chapter_import_rolls_back_on_dangling_asset_then_reads_ordered_blocks() {
        let mut db = fixture(ProjectKind::Book);
        let mut repo = ProjectRepository::new(&mut db, ProjectKind::Book).unwrap();
        let chapter = ChapterSummary {
            id: ChapterId("chapter".into()),
            position: 0,
            title: "Example".into(),
            revision: Revision("0".into()),
        };
        let text = block(
            "text",
            0,
            BookBlockContent::Text {
                text: "Before".into(),
            },
        );
        let image = block(
            "image",
            1,
            BookBlockContent::Image {
                asset_id: "a".repeat(64),
                alt: "Picture".into(),
            },
        );
        assert!(repo
            .insert_chapter(&chapter, &[text.clone(), image])
            .is_err());
        assert_eq!(
            repo.chapter("chapter").unwrap_err().code,
            ErrorCode::NotFound
        );
        let caption = block(
            "caption",
            1,
            BookBlockContent::Caption {
                text: "Caption".into(),
            },
        );
        repo.insert_chapter(&chapter, &[caption.clone(), text.clone()])
            .unwrap();
        assert_eq!(repo.chapter("chapter").unwrap().blocks, vec![text, caption]);
        repo.update_book_text("text", &Revision("0".into()), "After")
            .unwrap();
        let view = repo.chapter("chapter").unwrap();
        assert_eq!(view.chapter.revision.0, "1");
        assert_eq!(
            view.blocks[0].content,
            BookBlockContent::Text {
                text: "After".into()
            }
        );
        assert_eq!(
            repo.insert_volume("v", 0, "Volume", true).unwrap_err().code,
            ErrorCode::WrongProjectKind
        );
    }

    #[test]
    fn manga_pages_reference_validated_dimensions_and_keep_occurrence_identity() {
        let mut db = fixture(ProjectKind::Manga);
        db.execute("INSERT INTO assets(id,relative_path,mime,byte_length,width,height) VALUES(?1,'assets/test.png','image/png',10,32,48)",["a".repeat(64)]).unwrap();
        assert!(ProjectRepository::new(&mut db, ProjectKind::Book).is_err());
        let mut repo = ProjectRepository::new(&mut db, ProjectKind::Manga).unwrap();
        repo.insert_volume("volume", 0, "Volume", true).unwrap();
        let mut page = PageSummary {
            id: PageId("p1".into()),
            volume_id: VolumeId("volume".into()),
            position: 0,
            thumbnail_asset_id: None,
            original_asset_id: AssetId("a".repeat(64)),
            width: 32,
            height: 49,
            revision: Revision("0".into()),
        };
        assert_eq!(
            repo.insert_page(&page).unwrap_err().code,
            ErrorCode::InvalidInput
        );
        page.height = 48;
        repo.insert_page(&page).unwrap();
        page.id = PageId("p2".into());
        page.position = 1;
        repo.insert_page(&page).unwrap();
        let pages = repo.pages("volume").unwrap();
        assert_eq!(pages.len(), 2);
        assert_ne!(pages[0].id, pages[1].id);
        assert_eq!(pages[0].original_asset_id, pages[1].original_asset_id);
        assert_eq!(
            repo.chapter("chapter").unwrap_err().code,
            ErrorCode::WrongProjectKind
        );
    }
}
