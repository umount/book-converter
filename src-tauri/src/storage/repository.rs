//! Domain-scoped persistence for version-1 projects. No legacy chapter indices.
use crate::app::contracts::{
    AppError, BookBlockContent, BookBlockView, ErrorCode, ProjectKind, Revision,
};
use crate::app::requests::{BookChapterView, ChapterSummary, TranslationSummary};
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
        let mut chapter = tx
            .query_row(
                "SELECT id,position,source_title,revision,status,origin,needs_review,(SELECT NULLIF(trim(translated_title),'') FROM book_translations WHERE chapter_id=book_chapter_states.id AND target_language=(SELECT target_language FROM project_settings WHERE singleton=1) ORDER BY revision DESC LIMIT 1) FROM book_chapter_states WHERE id=?1",
                [id],
                |r| {
                    Ok(ChapterSummary {
                translated_volume: None,
                        volume: crate::book::parser::chapter_volume(&r.get::<_,String>(2)?),
                        translated_title:r.get(7)?,
                        status:r.get(4)?,origin:r.get(5)?,needs_review:r.get(6)?,
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
        if let Some(source) = &chapter.volume {
            let title = crate::application::book_volume::read(&tx, source)?.title;
            chapter.translated_volume = (!title.is_empty()).then_some(title);
        }
        let translation=tx.query_row("SELECT id,revision,translated_title,status,CASE WHEN provenance='reference' THEN 'reference' WHEN provenance IN ('manual','manual-replace') THEN 'manual' ELSE 'model' END FROM book_translations WHERE chapter_id=?1 AND target_language=(SELECT target_language FROM project_settings WHERE singleton=1) ORDER BY revision DESC LIMIT 1",[id],|r|Ok(TranslationSummary{id:r.get(0)?,revision:Revision(r.get::<_,i64>(1)?.to_string()),title:r.get(2)?,status:r.get(3)?,origin:r.get(4)?})).optional().map_err(storage_error)?;
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
        let target: String = tx
            .query_row(
                "SELECT target_language FROM project_settings WHERE singleton=1",
                [],
                |r| r.get(0),
            )
            .map_err(storage_error)?;
        let source = blocks
            .iter()
            .filter_map(|b| match &b.content {
                BookBlockContent::Text { text } | BookBlockContent::Caption { text } => {
                    Some(text.as_str())
                }
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n\n");
        let body = blocks
            .iter()
            .filter_map(|b| b.translated_text.as_deref())
            .collect::<Vec<_>>()
            .join("\n\n");
        let lang_issues = translation
            .as_ref()
            .map(|t| crate::textutil::leftover_foreign(&target, &t.title, &body, &source))
            .unwrap_or_default();
        let error: Option<String> = tx
            .query_row(
                "SELECT translation_error FROM book_chapter_states WHERE id=?1",
                [id],
                |r| r.get(0),
            )
            .map_err(storage_error)?;
        let translation_error = error
            .map(|error| {
                serde_json::from_str::<AppError>(&error).map_err(|_| AppError::invalid("jobError"))
            })
            .transpose()?;
        let status = chapter.status.clone();
        tx.commit().map_err(storage_error)?;
        Ok(BookChapterView {
            status,
            lang_issues,
            translation_error,
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
        tx.execute("UPDATE book_translations SET status='needs_review' WHERE status='ready' AND provenance!='reference' AND chapter_id IN (SELECT id FROM book_chapters WHERE position >= (SELECT position FROM book_chapters WHERE id=?1))",[id]).map_err(storage_error)?;
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
        tx.execute("UPDATE book_translations SET status='needs_review' WHERE status='ready' AND provenance!='reference' AND chapter_id IN (SELECT id FROM book_chapters WHERE position > (SELECT position FROM book_chapters WHERE id=?1))",[&chapter]).map_err(storage_error)?;
        tx.commit().map_err(storage_error)?;
        Ok(Revision(next.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::contracts::ChapterId;

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
            translated_volume: None,
            volume: None,
            translated_title: None,
            status: "pending".into(),
            origin: None,
            needs_review: false,
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
    }
}
