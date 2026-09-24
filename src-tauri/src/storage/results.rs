//! Atomic domain results. A late model response cannot replace a newer revision.
use super::{
    repository::{conflict, not_found, storage_error},
    shared::{glossary_revision, next, settings},
};
use crate::app::contracts::{AppError, MangaStage, Revision};
use rusqlite::{params, Connection, OptionalExtension};

#[derive(Debug, Clone)]
pub struct InputVersions {
    pub source: Revision,
    pub settings: Revision,
    pub glossary: Revision,
}

fn check_inputs(db: &Connection, versions: &InputVersions) -> Result<(), AppError> {
    if settings(db)?.revision != versions.settings || glossary_revision(db)? != versions.glossary {
        return Err(conflict());
    }
    Ok(())
}

#[derive(Debug, Clone)]
pub struct BookTranslation {
    pub id: String,
    pub chapter_id: String,
    pub inputs: InputVersions,
    pub expected_translation: Option<Revision>,
    pub title: String,
    pub provenance: String,
    pub context_fingerprint: String,
    pub blocks: Vec<(String, String)>,
}

pub fn save_translation(
    db: &mut Connection,
    value: &BookTranslation,
) -> Result<Revision, AppError> {
    let tx = db.transaction().map_err(storage_error)?;
    let revision = save_translation_in(&tx, value)?;
    tx.commit().map_err(storage_error)?;
    Ok(revision)
}

pub fn save_translation_in(
    tx: &rusqlite::Transaction<'_>,
    value: &BookTranslation,
) -> Result<Revision, AppError> {
    if value.id.is_empty() || value.provenance.is_empty() || value.context_fingerprint.is_empty() {
        return Err(AppError::invalid("translation"));
    }
    check_inputs(tx, &value.inputs)?;
    let source = tx
        .query_row(
            "SELECT revision FROM book_chapters WHERE id=?1",
            [&value.chapter_id],
            |r| r.get::<_, i64>(0),
        )
        .optional()
        .map_err(storage_error)?
        .ok_or_else(not_found)?;
    if source != value.inputs.source.value()? {
        return Err(conflict());
    }
    let target = settings(tx)?.choices.target_language;
    let latest: Option<i64>=tx.query_row("SELECT MAX(revision) FROM book_translations WHERE chapter_id=?1 AND target_language=?2",params![value.chapter_id,target],|r|r.get(0)).map_err(storage_error)?;
    if latest
        != value
            .expected_translation
            .as_ref()
            .map(Revision::value)
            .transpose()?
    {
        return Err(conflict());
    }
    let revision = match latest {
        Some(v) => next(v)?,
        None => 0,
    };
    let ids = {
        let mut query=tx.prepare("SELECT id FROM book_source_blocks WHERE chapter_id=?1 AND kind IN ('text','caption') ORDER BY position").map_err(storage_error)?;
        let rows = query
            .query_map([&value.chapter_id], |r| r.get::<_, String>(0))
            .map_err(storage_error)?;
        rows.collect::<Result<std::collections::HashSet<_>, _>>()
            .map_err(storage_error)?
    };
    let supplied: std::collections::HashSet<_> =
        value.blocks.iter().map(|(id, _)| id.clone()).collect();
    if ids.is_empty() || supplied != ids || supplied.len() != value.blocks.len() {
        return Err(AppError::invalid("translationBlocks"));
    }
    tx.execute(
        "UPDATE book_translations SET status='stale' WHERE chapter_id=?1 AND target_language=?2",
        rusqlite::params![value.chapter_id, target],
    )
    .map_err(storage_error)?;
    tx.execute("INSERT INTO book_translations(id,chapter_id,source_revision,settings_revision,status,provenance,target_language,translated_title,context_fingerprint,glossary_revision,revision) VALUES(?1,?2,?3,?4,'ready',?5,?6,?7,?8,?9,?10)",params![value.id,value.chapter_id,source,value.inputs.settings.value()?,value.provenance,target,value.title,value.context_fingerprint,value.inputs.glossary.value()?,revision]).map_err(storage_error)?;
    for (id, text) in &value.blocks {
        tx.execute("INSERT INTO book_translation_blocks(translation_id,chapter_id,source_block_id,translated_text) VALUES(?1,?2,?3,?4)",params![value.id,value.chapter_id,id,text]).map_err(storage_error)?;
    }
    tx.execute("UPDATE book_translations SET status='needs_review' WHERE status='ready' AND provenance!='reference' AND chapter_id IN (SELECT id FROM book_chapters WHERE position > (SELECT position FROM book_chapters WHERE id=?1))",[&value.chapter_id]).map_err(storage_error)?;
    Ok(Revision(revision.to_string()))
}

#[derive(Debug, Clone)]
pub struct BookContext {
    pub id: String,
    pub translation_id: String,
    pub translation_revision: Revision,
    pub summary: String,
    pub previous_tail: String,
    pub predecessor_id: Option<String>,
}

pub fn save_context(db: &Connection, value: &BookContext) -> Result<(), AppError> {
    let changed=db.execute("INSERT INTO book_contexts(id,translation_id,translation_revision,summary,previous_tail,predecessor_id) SELECT ?1,id,revision,?4,?5,?6 FROM book_translations WHERE id=?2 AND revision=?3 AND status='ready' AND source_revision=(SELECT revision FROM book_chapters WHERE id=book_translations.chapter_id) AND settings_revision=(SELECT revision FROM project_settings WHERE singleton=1) AND glossary_revision=(SELECT revision FROM glossary_state WHERE singleton=1)",params![value.id,value.translation_id,value.translation_revision.value()?,value.summary,value.previous_tail,value.predecessor_id]).map_err(storage_error)?;
    if changed != 1 {
        return Err(conflict());
    }
    Ok(())
}

#[derive(Debug, Clone)]
pub enum MangaOutput {
    Structured(serde_json::Value),
    Image(String),
}
#[derive(Debug, Clone)]
pub struct MangaResult {
    pub id: String,
    pub page_id: String,
    pub stage: MangaStage,
    pub inputs: InputVersions,
    pub expected_result: Option<Revision>,
    pub fingerprint: String,
    pub provider_version: String,
    pub output: MangaOutput,
}

pub fn save_manga_result(db: &mut Connection, value: &MangaResult) -> Result<Revision, AppError> {
    let tx = db.transaction().map_err(storage_error)?;
    let revision = save_manga_result_in(&tx, value)?;
    tx.commit().map_err(storage_error)?;
    Ok(revision)
}

pub fn save_manga_result_in(
    tx: &rusqlite::Transaction<'_>,
    value: &MangaResult,
) -> Result<Revision, AppError> {
    if value.id.is_empty() || value.fingerprint.is_empty() || value.provider_version.is_empty() {
        return Err(AppError::invalid("result"));
    }
    let needs_image = matches!(
        value.stage,
        MangaStage::Inpainting | MangaStage::Lettering | MangaStage::Masks
    );
    if needs_image != matches!(&value.output, MangaOutput::Image(_)) {
        return Err(AppError::invalid("stageOutput"));
    }
    check_inputs(tx, &value.inputs)?;
    let source = tx
        .query_row(
            "SELECT revision FROM manga_pages WHERE id=?1",
            [&value.page_id],
            |r| r.get::<_, i64>(0),
        )
        .optional()
        .map_err(storage_error)?
        .ok_or_else(not_found)?;
    if source != value.inputs.source.value()? {
        return Err(conflict());
    }
    let stage = serde_json::to_value(&value.stage).map_err(|_| AppError::invalid("stage"))?;
    let stage = stage.as_str().ok_or_else(|| AppError::invalid("stage"))?;
    let latest: Option<i64> = tx
        .query_row(
            "SELECT MAX(revision) FROM manga_results WHERE page_id=?1 AND stage=?2",
            params![value.page_id, stage],
            |r| r.get(0),
        )
        .map_err(storage_error)?;
    if latest
        != value
            .expected_result
            .as_ref()
            .map(Revision::value)
            .transpose()?
    {
        return Err(conflict());
    }
    let revision = match latest {
        Some(v) => next(v)?,
        None => 0,
    };
    let (asset, payload) = match &value.output {
        MangaOutput::Image(id) => (Some(id.clone()), None),
        MangaOutput::Structured(json) => {
            if !json.is_object() && !json.is_array() {
                return Err(AppError::invalid("stagePayload"));
            }
            (
                None,
                Some(serde_json::to_string(json).map_err(|_| AppError::invalid("stagePayload"))?),
            )
        }
    };
    if let Some(asset_id) = &asset {
        let valid:i64=tx.query_row("SELECT COUNT(*) FROM assets JOIN manga_pages ON assets.width=manga_pages.width AND assets.height=manga_pages.height WHERE assets.id=?1 AND manga_pages.id=?2",params![asset_id,value.page_id],|r|r.get(0)).map_err(storage_error)?;
        if valid != 1 {
            return Err(AppError::invalid("resultDimensions"));
        }
    }
    let affected: &[&str] = match value.stage {
        MangaStage::Detection => &[
            "detection",
            "recognition",
            "translation",
            "masks",
            "inpainting",
            "lettering",
        ],
        MangaStage::Recognition => &["recognition", "translation", "lettering"],
        MangaStage::Translation => &["translation", "lettering"],
        MangaStage::Masks => &["masks", "inpainting", "lettering"],
        MangaStage::Inpainting => &["inpainting", "lettering"],
        MangaStage::Lettering => &["lettering"],
    };
    for affected_stage in affected {
        tx.execute(
            "UPDATE manga_results SET validity='stale' WHERE page_id=?1 AND stage=?2",
            params![value.page_id, affected_stage],
        )
        .map_err(storage_error)?;
    }
    tx.execute("INSERT INTO manga_results(id,page_id,stage,input_fingerprint,revision,page_revision,settings_revision,glossary_revision,output_asset_id,payload_json,provider_version,validity) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,'current')",params![value.id,value.page_id,stage,value.fingerprint,revision,source,value.inputs.settings.value()?,value.inputs.glossary.value()?,asset,payload,value.provider_version]).map_err(storage_error)?;
    tx.execute(
        "INSERT INTO manga_reviews(result_id,state) VALUES(?1,'unreviewed')",
        [&value.id],
    )
    .map_err(storage_error)?;
    Ok(Revision(revision.to_string()))
}

/// Manual editing publishes a new chapter translation revision, preserving historical contexts.
pub fn edit_translation_block(
    db: &mut Connection,
    id: &str,
    block: &str,
    expected: &Revision,
    text: &str,
) -> Result<Revision, AppError> {
    let tx = db.transaction().map_err(storage_error)?;
    let (chapter,title,source,settings_rev,glossary,revision,context,status,provenance):(String,String,i64,i64,i64,i64,String,String,String)=tx.query_row("SELECT chapter_id,translated_title,source_revision,settings_revision,glossary_revision,revision,context_fingerprint,status,provenance FROM book_translations WHERE id=?1",[id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?,r.get(7)?,r.get(8)?))).optional().map_err(storage_error)?.ok_or_else(not_found)?;
    if revision != expected.value()? {
        return Err(conflict());
    }
    let mut blocks = {
        let mut query=tx.prepare("SELECT source_block_id,translated_text FROM book_translation_blocks WHERE translation_id=?1").map_err(storage_error)?;
        let rows = query
            .query_map([id], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
            })
            .map_err(storage_error)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(storage_error)?
    };
    let Some(target) = blocks.iter_mut().find(|(id, _)| id == block) else {
        return Err(AppError::invalid("translationBlock"));
    };
    target.1 = text.into();
    let current_settings = settings(&tx)?;
    let current_glossary = glossary_revision(&tx)?;
    let current_source: i64 = tx
        .query_row(
            "SELECT revision FROM book_chapters WHERE id=?1",
            [&chapter],
            |r| r.get(0),
        )
        .map_err(storage_error)?;
    let needs_review = status != "ready"
        || (provenance != "reference"
            && (source != current_source
                || settings_rev != current_settings.revision.value()?
                || glossary != current_glossary.value()?));
    let new_id = uuid::Uuid::new_v4().to_string();
    let result = save_translation_in(
        &tx,
        &BookTranslation {
            id: new_id.clone(),
            chapter_id: chapter,
            inputs: InputVersions {
                source: Revision(current_source.to_string()),
                settings: current_settings.revision,
                glossary: current_glossary,
            },
            expected_translation: Some(expected.clone()),
            title,
            provenance: "manual".into(),
            context_fingerprint: context,
            blocks,
        },
    )?;
    if needs_review {
        tx.execute(
            "UPDATE book_translations SET status='needs_review' WHERE id=?1",
            [&new_id],
        )
        .map_err(storage_error)?;
    }
    tx.commit().map_err(storage_error)?;
    Ok(result)
}

/// Title-only revisions preserve the body's origin, review state and continuity.
pub fn edit_translation_title(db: &mut Connection, id: &str, expected: &Revision, title: &str) -> Result<Revision, AppError> {
    let tx=db.transaction().map_err(storage_error)?;
    let (_,revision)=edit_translation_title_in(&tx,id,expected,title,None)?;
    tx.commit().map_err(storage_error)?;
    Ok(revision)
}

pub fn edit_translation_title_in(tx: &rusqlite::Transaction<'_>, id: &str, expected: &Revision, title: &str, inputs:Option<&InputVersions>) -> Result<(String,Revision),AppError> {
    if let Some(inputs)=inputs {
        check_inputs(tx,inputs)?;
        let source:i64=tx.query_row("SELECT c.revision FROM book_chapters c JOIN book_translations t ON t.chapter_id=c.id WHERE t.id=?1",[id],|r|r.get(0)).map_err(storage_error)?;
        if source!=inputs.source.value()? {return Err(conflict())}
    }
    let (revision,status):(i64,String)=tx.query_row("SELECT revision,status FROM book_translations t WHERE id=?1 AND revision=(SELECT MAX(revision) FROM book_translations WHERE chapter_id=t.chapter_id AND target_language=t.target_language)",[id],|r|Ok((r.get(0)?,r.get(1)?))).optional().map_err(storage_error)?.ok_or_else(conflict)?;
    if revision!=expected.value()? {return Err(conflict())}
    let revision=next(revision)?;
    let new_id=uuid::Uuid::new_v4().to_string();
    tx.execute("UPDATE book_translations SET status='stale' WHERE id=?1",[id]).map_err(storage_error)?;
    tx.execute("INSERT INTO book_translations(id,chapter_id,source_revision,settings_revision,status,provenance,target_language,translated_title,context_fingerprint,glossary_revision,revision) SELECT ?2,chapter_id,source_revision,settings_revision,?3,provenance,target_language,?4,context_fingerprint,glossary_revision,?5 FROM book_translations WHERE id=?1",params![id,new_id,status,title,revision]).map_err(storage_error)?;
    tx.execute("INSERT INTO book_translation_blocks(translation_id,chapter_id,source_block_id,translated_text) SELECT ?2,chapter_id,source_block_id,translated_text FROM book_translation_blocks WHERE translation_id=?1",params![id,new_id]).map_err(storage_error)?;
    tx.execute("INSERT INTO book_contexts(id,translation_id,summary,previous_tail,translation_revision,predecessor_id) SELECT ?3,?2,summary,previous_tail,?4,predecessor_id FROM book_contexts WHERE translation_id=?1",params![id,new_id,uuid::Uuid::new_v4().to_string(),revision]).map_err(storage_error)?;
    Ok((new_id,Revision(revision.to_string())))
}
