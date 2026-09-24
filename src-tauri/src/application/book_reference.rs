//! Explicit reference correspondence, independent of legacy numeric chapter matching.
use crate::{
    app::{
        contracts::{AppError, ChapterId, ProjectKind},
        requests::*,
    },
    project::lifecycle::ProjectManager,
    storage::repository::{conflict, storage_error, ProjectRepository},
};
use rusqlite::{Connection, Transaction};
use sha2::{Digest, Sha256};
use std::path::Path;

struct ImportedChapter {
    id: String,
    position: u32,
    title: String,
    text: String,
}

pub fn excerpt(db: &mut Connection, id: &str) -> Result<BookReferenceExcerpt, AppError> {
    ProjectRepository::new(db, ProjectKind::Book)?;
    use rusqlite::OptionalExtension;
    let text: String = db
        .query_row(
            "SELECT substr(text,1,1501) FROM book_reference_chapters WHERE id=?1",
            [id],
            |r| r.get(0),
        )
        .optional()
        .map_err(storage_error)?
        .ok_or_else(crate::storage::repository::not_found)?;
    Ok(BookReferenceExcerpt {
        truncated: text.chars().count() > 1500,
        text: text.chars().take(1500).collect(),
    })
}

pub fn read(db: &Connection) -> Result<BookReferenceView, AppError> {
    let mut fingerprint = Sha256::new();
    let chapters = {
        let mut q = db
            .prepare("SELECT id,position,title,text FROM book_reference_chapters ORDER BY position")
            .map_err(storage_error)?;
        let mut rows = q.query([]).map_err(storage_error)?;
        let mut chapters = Vec::new();
        while let Some(row) = rows.next().map_err(storage_error)? {
            let chapter = ReferenceChapterView {
                id: row.get(0).map_err(storage_error)?,
                position: row.get(1).map_err(storage_error)?,
                title: row.get(2).map_err(storage_error)?,
            };
            // Hash one row at a time; chapter bodies never enter the list response.
            let text: String = row.get(3).map_err(storage_error)?;
            let bytes = serde_json::to_vec(&(&chapter, &text))
                .map_err(|_| AppError::invalid("reference"))?;
            fingerprint.update((bytes.len() as u64).to_le_bytes());
            fingerprint.update(bytes);
            chapters.push(chapter);
        }
        chapters
    };
    let mappings = {
        let mut q = db
            .prepare(
                "SELECT chapter_id,reference_id FROM book_reference_mappings ORDER BY chapter_id",
            )
            .map_err(storage_error)?;
        let rows = q
            .query_map([], |r| {
                Ok(ReferenceMapping {
                    chapter_id: ChapterId(r.get(0)?),
                    reference_id: r.get(1)?,
                })
            })
            .map_err(storage_error)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(storage_error)?
    };
    let bytes = serde_json::to_vec(&mappings).map_err(|_| AppError::invalid("reference"))?;
    fingerprint.update(bytes);
    Ok(BookReferenceView {
        fingerprint: format!("{:x}", fingerprint.finalize()),
        chapters,
        mappings,
    })
}
fn invalidate(tx: &Transaction<'_>) -> Result<(), AppError> {
    // Reference data is an input to every chapter translation. Incrementing chapter
    // revisions also rejects a response already in flight at the time of the edit.
    tx.execute("UPDATE book_chapters SET revision=revision+1", [])
        .map_err(storage_error)?;
    tx.execute(
        "UPDATE book_translations SET status='needs_review' WHERE status='ready'",
        [],
    )
    .map_err(storage_error)?;
    Ok(())
}
pub fn import(
    manager: &ProjectManager,
    args: &BookReferenceImportArgs,
) -> Result<BookReferenceView, AppError> {
    let lease = manager.lease(&args.project_id)?;
    let expected = lease.with_connection(|db, _| {
        ProjectRepository::new(db, ProjectKind::Book)?;
        Ok(read(db)?.fingerprint)
    })?;
    let path = Path::new(&args.path);
    let mut loaded =
        crate::book::load_book(path).map_err(|_| AppError::invalid("referenceSource"))?;
    if loaded.chapters.is_empty() {
        let decoded =
            crate::book::read_book_file(path).map_err(|_| AppError::invalid("referenceSource"))?;
        if decoded.text.trim().is_empty() {
            return Err(AppError::invalid("emptyReference"));
        }
        loaded.chapters.push(crate::book::Chapter {
            index: 0,
            number: None,
            title: loaded.meta.title.clone().unwrap_or_default(),
            body: decoded.text,
        });
    }
    let blocks_by_chapter: std::collections::HashMap<_, _> = loaded
        .blocks
        .iter()
        .map(|blocks| (blocks.chapter_index, blocks))
        .collect();
    let chapters = loaded
        .chapters
        .iter()
        .enumerate()
        .map(|(position, chapter)| {
            let text = blocks_by_chapter
                .get(&chapter.index)
                .map(|blocks| {
                    blocks
                        .blocks
                        .iter()
                        .filter(|b| b.kind != crate::book::blocks::BlockKind::Image)
                        .map(|b| b.text.as_str())
                        .collect::<Vec<_>>()
                        .join("\n\n")
                })
                .unwrap_or_else(|| chapter.body.clone());
            ImportedChapter {
                id: uuid::Uuid::new_v4().to_string(),
                position: position as u32,
                title: chapter.title.clone(),
                text,
            }
        })
        .collect::<Vec<_>>();
    lease.with_connection(|db, _| {
        let tx = db.transaction().map_err(storage_error)?;
        if read(&tx)?.fingerprint != expected {
            return Err(conflict());
        }
        tx.execute("DELETE FROM book_reference_mappings", [])
            .map_err(storage_error)?;
        tx.execute("DELETE FROM book_reference_chapters", [])
            .map_err(storage_error)?;
        for chapter in &chapters {
            tx.execute(
                "INSERT INTO book_reference_chapters(id,position,title,text) VALUES(?1,?2,?3,?4)",
                rusqlite::params![chapter.id, chapter.position, chapter.title, chapter.text],
            )
            .map_err(storage_error)?;
        }
        invalidate(&tx)?;
        let view = read(&tx)?;
        tx.commit().map_err(storage_error)?;
        Ok(view)
    })
}
pub fn map(
    db: &mut Connection,
    args: &BookReferenceMapArgs,
) -> Result<BookReferenceView, AppError> {
    ProjectRepository::new(db, ProjectKind::Book)?;
    let tx = db.transaction().map_err(storage_error)?;
    if read(&tx)?.fingerprint != args.expected_fingerprint {
        return Err(conflict());
    }
    tx.execute("DELETE FROM book_reference_mappings", [])
        .map_err(storage_error)?;
    for mapping in &args.mappings {
        tx.execute(
            "INSERT INTO book_reference_mappings(chapter_id,reference_id) VALUES(?1,?2)",
            rusqlite::params![mapping.chapter_id.0, mapping.reference_id],
        )
        .map_err(storage_error)?;
    }
    let view = read(&tx)?;
    if view.fingerprint != args.expected_fingerprint {
        invalidate(&tx)?;
    }
    tx.commit().map_err(storage_error)?;
    Ok(view)
}
