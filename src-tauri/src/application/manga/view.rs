//! Read the page and its recognition state from one consistent database snapshot.
use super::regions;
use crate::{
    app::{
        contracts::{AppError, AssetId, PageId, ProjectKind, RegionId, Revision, VolumeId},
        requests::{MangaPageView, MangaRecognitionSummary, MangaRegionView, PageSummary},
    },
    storage::repository::{not_found, storage_error, ProjectRepository},
};
use rusqlite::{Connection, OptionalExtension};
pub fn page(db: &mut Connection, id: &str) -> Result<MangaPageView, AppError> {
    ProjectRepository::new(db, ProjectKind::Manga)?;
    let tx = db.transaction().map_err(storage_error)?;
    let page=tx.query_row("SELECT id,volume_id,position,original_asset_id,width,height,revision,(SELECT asset_id FROM manga_page_previews WHERE page_id=p.id) FROM manga_pages p WHERE id=?1",[id],|r|Ok(PageSummary{id:PageId(r.get(0)?),volume_id:VolumeId(r.get(1)?),position:r.get(2)?,original_asset_id:AssetId(r.get(3)?),thumbnail_asset_id:r.get::<_,Option<String>>(7)?.map(AssetId),width:r.get(4)?,height:r.get(5)?,revision:Revision(r.get::<_,i64>(6)?.to_string())})).optional().map_err(storage_error)?.ok_or_else(not_found)?;
    let recognition=tx.query_row("SELECT r.revision,(r.validity='current' AND r.page_revision=p.revision AND r.settings_revision=s.revision AND r.glossary_revision=g.revision),v.state='needs_review' FROM manga_results r JOIN manga_pages p ON p.id=r.page_id JOIN project_settings s ON s.singleton=1 JOIN glossary_state g ON g.singleton=1 JOIN manga_reviews v ON v.result_id=r.id WHERE r.page_id=?1 AND r.stage='recognition' ORDER BY r.revision DESC LIMIT 1",[id],|r|Ok(MangaRecognitionSummary{revision:Revision(r.get::<_,i64>(0)?.to_string()),current:r.get(1)?,needs_review:r.get(2)?})).optional().map_err(storage_error)?;
    let regions = regions::read(&tx, id)?
        .into_iter()
        .map(|r| MangaRegionView {
            vertical: r.vertical,
            id: RegionId(r.id),
            page_id: PageId(id.into()),
            reading_order: r.reading_order,
            bounds: r.bounds,
            source_text: r.source_text,
            translated_text: r.translated_text,
            source_manual: r.source_manual,
            translation_manual: r.translation_manual,
            revision: Revision(r.revision.to_string()),
        })
        .collect();
    let rendered_asset_id=tx.query_row("SELECT r.output_asset_id FROM manga_results r JOIN manga_pages p ON p.id=r.page_id JOIN project_settings s ON s.singleton=1 JOIN glossary_state g ON g.singleton=1 WHERE r.page_id=?1 AND r.stage='lettering' AND r.validity='current' AND r.page_revision=p.revision AND r.settings_revision=s.revision AND r.glossary_revision=g.revision ORDER BY r.revision DESC LIMIT 1",[id],|r|r.get::<_,String>(0)).optional().map_err(storage_error)?.map(AssetId);
    Ok(MangaPageView {
        rendered_asset_id,
        page,
        regions,
        recognition,
    })
}
