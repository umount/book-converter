use super::repository::storage_error;
use crate::app::contracts::AppError;
use rusqlite::{params, Connection};

pub struct ReferenceChapter {
    pub id: String,
    pub position: u32,
    pub title: String,
    pub text: String,
}

/// Mappings are explicit stable-ID pairs supplied after matching, never numeric coincidence.
pub fn replace_reference(
    db: &mut Connection,
    chapters: &[ReferenceChapter],
    mappings: &[(String, String)],
) -> Result<(), AppError> {
    let tx = db.transaction().map_err(storage_error)?;
    tx.execute("DELETE FROM book_reference_mappings", [])
        .map_err(storage_error)?;
    tx.execute("DELETE FROM book_reference_chapters", [])
        .map_err(storage_error)?;
    for chapter in chapters {
        tx.execute(
            "INSERT INTO book_reference_chapters(id,position,title,text) VALUES(?1,?2,?3,?4)",
            params![chapter.id, chapter.position, chapter.title, chapter.text],
        )
        .map_err(storage_error)?;
    }
    for (chapter, reference) in mappings {
        tx.execute(
            "INSERT INTO book_reference_mappings(chapter_id,reference_id) VALUES(?1,?2)",
            params![chapter, reference],
        )
        .map_err(storage_error)?;
    }
    tx.execute(
        "UPDATE book_translations SET status='needs_review' WHERE status='ready'",
        [],
    )
    .map_err(storage_error)?;
    tx.commit().map_err(storage_error)
}
