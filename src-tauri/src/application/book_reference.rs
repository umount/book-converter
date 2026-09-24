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

pub fn read(db: &Connection) -> Result<BookReferenceView, AppError> {
    let chapters = {
        let mut q = db
            .prepare("SELECT id,position,title,text FROM book_reference_chapters ORDER BY position")
            .map_err(storage_error)?;
        let rows = q
            .query_map([], |r| {
                Ok(ReferenceChapterView {
                    id: r.get(0)?,
                    position: r.get(1)?,
                    title: r.get(2)?,
                    text: r.get(3)?,
                })
            })
            .map_err(storage_error)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(storage_error)?
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
    let bytes =
        serde_json::to_vec(&(&chapters, &mappings)).map_err(|_| AppError::invalid("reference"))?;
    Ok(BookReferenceView {
        fingerprint: format!("{:x}", Sha256::digest(bytes)),
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
    let chapters = loaded
        .chapters
        .iter()
        .enumerate()
        .map(|(position, chapter)| {
            let text = loaded
                .blocks
                .iter()
                .find(|blocks| blocks.chapter_index == chapter.index)
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
            ReferenceChapterView {
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
