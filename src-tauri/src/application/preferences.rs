//! Shared processing profiles and glossary editing; project languages are immutable.
use crate::{
    app::{
        contracts::{AppError, Revision, TermId},
        requests::*,
    },
    storage::{
        repository::{conflict, storage_error},
        shared,
    },
};
use rusqlite::{Connection, OptionalExtension};

pub fn settings(db: &Connection) -> Result<ProjectSettingsView, AppError> {
    let value = shared::settings(db)?;
    let v = value.choices;
    Ok(ProjectSettingsView {
        languages: LanguagePair {
            source: v.source_language,
            target: v.target_language,
        },
        choices: ProcessingChoices {
            book_translation_profile: v.book_translation_profile,
            assistant_profile: v.assistant_profile,
        },
        revision: value.revision,
    })
}
pub fn update_settings(
    db: &mut Connection,
    args: &ProjectSettingsUpdateArgs,
) -> Result<Revision, AppError> {
    let mut choices = shared::settings(db)?.choices;
    choices.book_translation_profile = args.choices.book_translation_profile.clone();
    choices.assistant_profile = args.choices.assistant_profile.clone();
    shared::update_settings(db, &args.expected_revision, &choices)
}
pub fn glossary_page(
    db: &mut Connection,
    args: &GlossaryListArgs,
) -> Result<GlossaryPage, AppError> {
    if args.limit == 0 || args.limit > 500 || args.query.len() > 1024 {
        return Err(AppError::invalid("limit"));
    }
    let tx = db.transaction().map_err(storage_error)?;
    let settings = shared::settings(&tx)?;
    let after = if let Some(id) = &args.cursor {
        Some(
            tx.query_row(
                "SELECT source,id FROM glossary_terms WHERE id=?1",
                [id],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
            )
            .optional()
            .map_err(storage_error)?
            .ok_or_else(|| AppError::invalid("cursor"))?,
        )
    } else {
        None
    };
    let mut q=tx.prepare("SELECT id,source,target,kind,pinned,frequency,revision FROM glossary_terms WHERE (?1 IS NULL OR (source,id)>(?1,?2)) AND (instr(source,?4)>0 OR instr(target,?4)>0) AND (NOT ?5 OR pinned=1) ORDER BY source,id LIMIT ?3").map_err(storage_error)?;
    let mut items = q
        .query_map(
            rusqlite::params![
                after.as_ref().map(|v| &v.0),
                after.as_ref().map(|v| &v.1),
                args.limit + 1,
                args.query,
                args.pinned_only
            ],
            |r| {
                Ok(GlossaryTermView {
                    id: TermId(r.get(0)?),
                    source: r.get(1)?,
                    target: r.get(2)?,
                    kind: r.get(3)?,
                    pinned: r.get(4)?,
                    frequency: r.get(5)?,
                    revision: Revision(r.get::<_, i64>(6)?.to_string()),
                })
            },
        )
        .map_err(storage_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(storage_error)?;
    let next_cursor = if items.len() > args.limit as usize {
        items.pop();
        items.last().map(|v| v.id.0.clone())
    } else {
        None
    };
    let total = tx.query_row("SELECT COUNT(*) FROM glossary_terms WHERE (instr(source,?1)>0 OR instr(target,?1)>0) AND (NOT ?2 OR pinned=1)", rusqlite::params![args.query,args.pinned_only], |r| r.get(0)).map_err(storage_error)?;
    Ok(GlossaryPage {
        total,
        items,
        next_cursor,
        revision: shared::glossary_revision(&tx)?,
        settings_revision: settings.revision,
    })
}
pub fn put_term(db: &mut Connection, args: &GlossaryPutArgs) -> Result<Revision, AppError> {
    let tx = db.transaction().map_err(storage_error)?;
    if shared::settings(&tx)?.revision != args.expected_settings_revision {
        return Err(conflict());
    }
    let frequency = tx
        .query_row(
            "SELECT frequency FROM glossary_terms WHERE id=?1",
            [&args.term_id.0],
            |r| r.get::<_, u32>(0),
        )
        .optional()
        .map_err(storage_error)?
        .unwrap_or(0);
    let revision = shared::put_term_in(
        &tx,
        &shared::GlossaryTerm {
            id: args.term_id.0.clone(),
            source: args.source.clone(),
            target: args.target.clone(),
            kind: args.kind.clone(),
            pinned: args.pinned,
            frequency,
            revision: args
                .expected_revision
                .clone()
                .unwrap_or(Revision("0".into())),
        },
        args.expected_revision.as_ref(),
    )?;
    tx.commit().map_err(storage_error)?;
    Ok(revision)
}
pub fn delete_term(db: &mut Connection, args: &GlossaryDeleteArgs) -> Result<(), AppError> {
    let tx = db.transaction().map_err(storage_error)?;
    if shared::settings(&tx)?.revision != args.expected_settings_revision {
        return Err(conflict());
    }
    shared::delete_term_in(&tx, &args.term_id.0, &args.expected_revision)?;
    tx.commit().map_err(storage_error)
}
