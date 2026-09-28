//! Previewed structural edits. Publication is atomic and guarded by captured revisions.
use crate::{
    app::{
        contracts::{AppError, BlockId, ChapterId, ProjectId, ProjectKind},
        requests::*,
    },
    storage::{
        repository::{storage_error, ProjectRepository},
        results::{self, BookTranslation, InputVersions},
        shared,
    },
};
use rusqlite::Connection;
use std::{
    collections::{HashMap, HashSet},
    sync::Mutex,
    time::{Duration, Instant},
};

/// Shared chapter-instruction write path for the editor and assistant proposals.
pub fn update_instructions(
    db: &mut Connection,
    args: &UpdateChapterInstructionsArgs,
) -> Result<crate::app::contracts::Revision, AppError> {
    ProjectRepository::new(db, ProjectKind::Book)?.update_chapter_instructions(
        &args.chapter_id.0,
        &args.expected_revision,
        &args.instructions,
    )
}

pub struct PreparedReplacement {
    project: ProjectId,
    created: Instant,
    translations: Vec<BookTranslation>,
    needs_review: HashSet<String>,
    pub view: BookReplacePreview,
}
#[derive(Default)]
pub struct EditPreviews(Mutex<HashMap<String, PreparedReplacement>>);
impl EditPreviews {
    pub fn insert(&self, preview: PreparedReplacement) -> Result<BookReplacePreview, AppError> {
        let mut entries = self.0.lock().unwrap_or_else(|p| p.into_inner());
        entries.retain(|_, p| p.created.elapsed() < Duration::from_secs(1800));
        if entries.len() >= 8 {
            return Err(AppError::invalid("tooManyPreviews"));
        }
        let view = preview.view.clone();
        entries.insert(view.preview_id.clone(), preview);
        Ok(view)
    }
    pub fn take(&self, project: &ProjectId, id: &str) -> Result<PreparedReplacement, AppError> {
        let mut entries = self.0.lock().unwrap_or_else(|p| p.into_inner());
        let value = entries
            .get(id)
            .ok_or_else(|| AppError::invalid("previewExpired"))?;
        if &value.project != project {
            return Err(AppError::invalid("previewProject"));
        }
        let value = entries.remove(id).expect("preview checked under lock");
        if value.created.elapsed() >= Duration::from_secs(1800) {
            return Err(AppError::invalid("previewExpired"));
        }
        Ok(value)
    }
}

pub fn preview(
    db: &mut Connection,
    args: &BookReplacePreviewArgs,
) -> Result<PreparedReplacement, AppError> {
    preview_replacements(db, args, &[(args.search.clone(), args.replacement.clone())])
}

/// One snapshot and one publication for all literal corrections in an assistant reply.
pub fn preview_replacements(
    db: &mut Connection,
    args: &BookReplacePreviewArgs,
    replacements: &[(String, String)],
) -> Result<PreparedReplacement, AppError> {
    if replacements.is_empty() {
        return Err(AppError::invalid("search"));
    }
    let patterns = replacements.iter().map(|(search, replacement)| {
        if search.is_empty() || search.len() > 8192 || replacement.len() > 65536 {
            return Err(AppError::invalid("search"));
        }
        let pattern = regex::RegexBuilder::new(&regex::escape(search))
            .case_insensitive(!args.case_sensitive).build()
            .map_err(|_| AppError::invalid("search"))?;
        Ok((pattern, replacement))
    }).collect::<Result<Vec<_>, AppError>>()?;
    ProjectRepository::new(db, ProjectKind::Book)?;
    // One read transaction captures settings, chapter and translation revisions together.
    let tx = db.transaction().map_err(storage_error)?;
    let settings = shared::settings(&tx)?;
    let glossary = shared::glossary_revision(&tx)?;
    let ordered = {
        let mut q = tx
            .prepare("SELECT id FROM book_chapters ORDER BY position")
            .map_err(storage_error)?;
        let rows = q
            .query_map([], |r| r.get::<_, String>(0))
            .map_err(storage_error)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(storage_error)?
    };
    let mut translations = Vec::new();
    let mut needs_review = HashSet::new();
    let mut changes = Vec::new();
    let mut bytes = 0usize;
    for chapter in args.selection.resolve(&ordered)? {
        use rusqlite::OptionalExtension;
        let row = tx.query_row("SELECT id,revision,translated_title,source_revision,settings_revision,glossary_revision,context_fingerprint,status,provenance FROM book_translations WHERE chapter_id=?1 AND target_language=?2 ORDER BY revision DESC LIMIT 1", rusqlite::params![chapter,settings.choices.target_language], |r| Ok((r.get::<_,String>(0)?,r.get::<_,i64>(1)?,r.get::<_,String>(2)?,r.get::<_,i64>(3)?,r.get::<_,i64>(4)?,r.get::<_,i64>(5)?,r.get::<_,String>(6)?,r.get::<_,String>(7)?,r.get::<_,String>(8)?))).optional().map_err(storage_error)?;
        let Some((
            id,
            revision,
            title,
            source,
            setting_rev,
            _glossary_rev,
            context,
            status,
            provenance,
        )) = row
        else {
            continue;
        };
        let mut q = tx.prepare("SELECT source_block_id,translated_text FROM book_translation_blocks JOIN book_source_blocks ON book_source_blocks.id=source_block_id WHERE translation_id=?1 ORDER BY position").map_err(storage_error)?;
        let mut blocks = q
            .query_map([&id], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
            })
            .map_err(storage_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(storage_error)?;
        let before_count = changes.len();
        for (block, text) in &mut blocks {
            bytes = bytes.saturating_add(text.len());
            let mut replaced = text.clone();
            for (pattern, replacement) in &patterns {
                replaced = pattern.replace_all(&replaced, regex::NoExpand(replacement)).into_owned();
            }
            if replaced != *text {
                bytes = bytes.saturating_add(replaced.len());
                changes.push(BookReplaceChange {
                    chapter_id: ChapterId(chapter.clone()),
                    block_id: BlockId(block.clone()),
                    before: text.clone(),
                    after: replaced,
                });
                *text = changes.last().unwrap().after.clone();
            }
            if bytes > 32 * 1024 * 1024 || changes.len() > 5000 {
                return Err(AppError::invalid("previewTooLarge"));
            }
        }
        if changes.len() == before_count {
            continue;
        }
        let current_source: i64 = tx
            .query_row(
                "SELECT revision FROM book_chapters WHERE id=?1",
                [&chapter],
                |r| r.get(0),
            )
            .map_err(storage_error)?;
        // A reviewed literal edit is allowed on an older translation. It does not
        // establish that the rest of the chapter meets the updated book inputs.
        if status != "ready"
            || (provenance != "reference"
                && (source != current_source
                    || setting_rev.to_string() != settings.revision.0))
        {
            needs_review.insert(chapter.clone());
        }
        translations.push(BookTranslation {
            id: uuid::Uuid::new_v4().to_string(),
            chapter_id: chapter,
            inputs: InputVersions {
                source: crate::app::contracts::Revision(current_source.to_string()),
                settings: settings.revision.clone(),
                glossary: glossary.clone(),
            },
            expected_translation: Some(crate::app::contracts::Revision(revision.to_string())),
            title,
            provenance: "manual-replace".into(),
            context_fingerprint: context,
            blocks,
        });
    }
    Ok(PreparedReplacement {
        project: args.project_id.clone(),
        created: Instant::now(),
        translations,
        needs_review,
        view: BookReplacePreview {
            preview_id: uuid::Uuid::new_v4().to_string(),
            changes,
        },
    })
}

pub fn apply(db: &mut Connection, preview: PreparedReplacement) -> Result<u32, AppError> {
    let tx = db.transaction().map_err(storage_error)?;
    for translation in &preview.translations {
        results::save_translation_in(&tx, translation)?;
        if preview.needs_review.contains(&translation.chapter_id) {
            tx.execute(
                "UPDATE book_translations SET status='needs_review' WHERE id=?1",
                [&translation.id],
            )
            .map_err(storage_error)?;
        }
    }
    tx.commit().map_err(storage_error)?;
    Ok(preview.view.changes.len() as u32)
}
