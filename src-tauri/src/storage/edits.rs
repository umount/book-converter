//! Manga region/mask edits and explicit book reference mappings.
use super::repository::{conflict, not_found, storage_error};
use crate::app::{
    contracts::{AppError, PixelBounds, Revision},
    requests::{RegionPatch, ReviewDecision},
};
use rusqlite::{params, Connection, OptionalExtension};

pub struct NewRegion {
    pub id: String,
    pub order: u32,
    pub category: String,
    pub bounds: PixelBounds,
    pub source_text: String,
}

/// Initial detection inserts the whole set or nothing; reruns require reconciliation in P09.
pub fn insert_regions(
    db: &mut Connection,
    page_id: &str,
    expected: &Revision,
    regions: &[NewRegion],
) -> Result<Revision, AppError> {
    let tx = db.transaction().map_err(storage_error)?;
    let (width, height, revision) = tx
        .query_row(
            "SELECT width,height,revision FROM manga_pages WHERE id=?1",
            [page_id],
            |r| {
                Ok((
                    r.get::<_, u32>(0)?,
                    r.get::<_, u32>(1)?,
                    r.get::<_, i64>(2)?,
                ))
            },
        )
        .optional()
        .map_err(storage_error)?
        .ok_or_else(not_found)?;
    if revision != expected.value()? {
        return Err(conflict());
    }
    let count: i64 = tx
        .query_row(
            "SELECT COUNT(*) FROM manga_regions WHERE page_id=?1",
            [page_id],
            |r| r.get(0),
        )
        .map_err(storage_error)?;
    if count != 0 {
        return Err(conflict());
    }
    for region in regions {
        if region.id.is_empty() {
            return Err(AppError::invalid("regionId"));
        }
        region.bounds.validate(width, height)?;
        let geometry =
            serde_json::to_string(&region.bounds).map_err(|_| AppError::invalid("bounds"))?;
        tx.execute("INSERT INTO manga_regions(id,page_id,reading_order,category,geometry_json,source_text) VALUES(?1,?2,?3,?4,?5,?6)",params![region.id,page_id,region.order,region.category,geometry,region.source_text]).map_err(storage_error)?;
    }
    invalidate_page(&tx, page_id)?;
    tx.commit().map_err(storage_error)?;
    Ok(Revision(super::shared::next(revision)?.to_string()))
}

fn invalidate_page(tx: &rusqlite::Transaction<'_>, page: &str) -> Result<(), AppError> {
    tx.execute(
        "UPDATE manga_pages SET revision=revision+1 WHERE id=?1",
        [page],
    )
    .map_err(storage_error)?;
    tx.execute(
        "UPDATE manga_results SET validity='stale' WHERE page_id=?1",
        [page],
    )
    .map_err(storage_error)?;
    Ok(())
}

pub fn update_region(
    db: &mut Connection,
    id: &str,
    expected: &Revision,
    patch: &RegionPatch,
) -> Result<Revision, AppError> {
    let tx = db.transaction().map_err(storage_error)?;
    let (page, revision): (String, i64) = tx
        .query_row(
            "SELECT page_id,revision FROM manga_regions WHERE id=?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(storage_error)?
        .ok_or_else(not_found)?;
    if revision != expected.value()? {
        return Err(conflict());
    }
    let revision = super::shared::next(revision)?;
    match patch {
        RegionPatch::SourceText { text } => {
            tx.execute("UPDATE manga_regions SET source_text=?1,source_manual=1,text_revision=text_revision+1,revision=?2 WHERE id=?3",params![text,revision,id]).map_err(storage_error)?;
        }
        RegionPatch::TranslatedText { text } => {
            tx.execute("UPDATE manga_regions SET translated_text=?1,translation_manual=1,text_revision=text_revision+1,revision=?2 WHERE id=?3",params![text,revision,id]).map_err(storage_error)?;
        }
        RegionPatch::Bounds { bounds } => {
            let (width, height) = tx
                .query_row(
                    "SELECT width,height FROM manga_pages WHERE id=?1",
                    [&page],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .map_err(storage_error)?;
            bounds.validate(width, height)?;
            tx.execute("UPDATE manga_regions SET geometry_json=?1,geometry_revision=geometry_revision+1,revision=?2 WHERE id=?3",params![serde_json::to_string(bounds).map_err(|_|AppError::invalid("bounds"))?,revision,id]).map_err(storage_error)?;
            tx.execute("DELETE FROM manga_masks WHERE page_id=?1", [&page])
                .map_err(storage_error)?;
        }
    }
    invalidate_page(&tx, &page)?;
    tx.commit().map_err(storage_error)?;
    Ok(Revision(revision.to_string()))
}

pub fn save_mask(
    db: &mut Connection,
    id: &str,
    page: &str,
    region: Option<&str>,
    asset: &str,
    expected_page: &Revision,
) -> Result<Revision, AppError> {
    let tx = db.transaction().map_err(storage_error)?;
    let current: Option<i64> = tx
        .query_row(
            "SELECT revision FROM manga_pages WHERE id=?1",
            [page],
            |r| r.get(0),
        )
        .optional()
        .map_err(storage_error)?;
    let revision = current.ok_or_else(not_found)?;
    if revision != expected_page.value()? {
        return Err(conflict());
    }
    let dimensions_match:i64=tx.query_row("SELECT COUNT(*) FROM assets JOIN manga_pages ON assets.width=manga_pages.width AND assets.height=manga_pages.height WHERE assets.id=?1 AND manga_pages.id=?2",params![asset,page],|r|r.get(0)).map_err(storage_error)?;
    if dimensions_match != 1 {
        return Err(AppError::invalid("maskAsset"));
    }
    let geometry_revision = if let Some(region) = region {
        tx.query_row(
            "SELECT geometry_revision FROM manga_regions WHERE id=?1 AND page_id=?2",
            params![region, page],
            |r| r.get::<_, i64>(0),
        )
        .optional()
        .map_err(storage_error)?
        .ok_or_else(not_found)?
    } else {
        revision
    };
    tx.execute("INSERT INTO manga_masks(id,page_id,region_id,asset_id,geometry_revision) VALUES(?1,?2,?3,?4,?5)",params![id,page,region,asset,geometry_revision]).map_err(storage_error)?;
    invalidate_page(&tx, page)?;
    tx.commit().map_err(storage_error)?;
    Ok(Revision(super::shared::next(revision)?.to_string()))
}

pub fn review_result(
    db: &Connection,
    id: &str,
    expected: &Revision,
    decision: ReviewDecision,
) -> Result<(), AppError> {
    let state = match decision {
        ReviewDecision::Unreviewed => "unreviewed",
        ReviewDecision::NeedsReview => "needs_review",
        ReviewDecision::Approved => "approved",
    };
    let changed=db.execute("UPDATE manga_reviews SET state=?1 WHERE result_id=?2 AND EXISTS(SELECT 1 FROM manga_results WHERE id=?2 AND revision=?3 AND validity='current' AND stage='lettering')",params![state,id,expected.value()?]).map_err(storage_error)?;
    if changed != 1 {
        return Err(conflict());
    }
    Ok(())
}

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
