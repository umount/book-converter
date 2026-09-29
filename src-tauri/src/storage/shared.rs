//! Revision-checked project settings, glossary and assistant history persistence.
use super::repository::{conflict, not_found, storage_error};
use crate::app::contracts::{AppError, Revision};
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessingSettings {
    pub source_language: Option<String>,
    pub target_language: String,
    /// Profile IDs only. Credentials belong to app settings, never a project archive.
    pub book_translation_profile: Option<String>,
    pub assistant_profile: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsSnapshot {
    pub choices: ProcessingSettings,
    pub revision: Revision,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GlossaryTerm {
    pub id: String,
    pub source: String,
    pub target: String,
    pub kind: String,
    pub pinned: bool,
    pub frequency: u32,
    pub revision: Revision,
}

pub fn settings(db: &Connection) -> Result<SettingsSnapshot, AppError> {
    let (source, target, profiles, revision): (Option<String>, String, String, i64) = db.query_row(
        "SELECT source_language,target_language,profiles_json,revision FROM project_settings WHERE singleton=1", [],
        |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).map_err(storage_error)?;
    let profiles: std::collections::BTreeMap<String, String> =
        serde_json::from_str(&profiles).map_err(|_| AppError::invalid("profiles"))?;
    Ok(SettingsSnapshot {
        choices: ProcessingSettings {
            source_language: source,
            target_language: target,
            book_translation_profile: profiles.get("book_translation").cloned(),
            assistant_profile: profiles.get("assistant").cloned(),
        },
        revision: Revision(revision.to_string()),
    })
}

pub fn update_settings(
    db: &mut Connection,
    expected: &Revision,
    choices: &ProcessingSettings,
) -> Result<Revision, AppError> {
    if choices.target_language.trim().is_empty()
        || choices
            .source_language
            .as_ref()
            .is_some_and(|s| s.trim().is_empty())
    {
        return Err(AppError::invalid("languages"));
    }
    let expected = expected.value()?;
    let revision = next(expected)?;
    let profiles: std::collections::BTreeMap<_, _> = [
        ("book_translation", &choices.book_translation_profile),
        ("assistant", &choices.assistant_profile),
    ]
    .into_iter()
    .filter_map(|(role, profile)| profile.as_ref().map(|p| (role, p)))
    .collect();
    let json = serde_json::to_string(&profiles).map_err(|_| AppError::invalid("profiles"))?;
    let tx = db.transaction().map_err(storage_error)?;
    let current = settings(&tx)?;
    if current.revision.value()? != expected {
        return Err(conflict());
    }
    if current.choices.source_language != choices.source_language
        || current.choices.target_language != choices.target_language
    {
        return Err(AppError::invalid("projectLanguagesImmutable"));
    }
    if current.choices == *choices {
        return Ok(current.revision);
    }
    let changed = tx.execute("UPDATE project_settings SET source_language=?1,target_language=?2,profiles_json=?3,revision=?4 WHERE singleton=1 AND revision=?5",params![choices.source_language,choices.target_language,json,revision,expected]).map_err(storage_error)?;
    if changed != 1 {
        return Err(conflict());
    }
    invalidate(&tx)?;
    tx.commit().map_err(storage_error)?;
    Ok(Revision(revision.to_string()))
}

/// Only staged imports may set languages. Published projects never unlock them.
pub(crate) fn finalize_import_settings(
    db: &mut Connection,
    expected: &Revision,
    choices: &ProcessingSettings,
) -> Result<Revision, AppError> {
    if choices
        .source_language
        .as_deref()
        .is_none_or(|s| s.trim().is_empty())
        || choices.target_language.trim().is_empty()
    {
        return Err(AppError::invalid("languages"));
    }
    let tx = db.transaction().map_err(storage_error)?;
    let current = settings(&tx)?;
    if current.revision != *expected {
        return Err(conflict());
    }
    let locked: bool = tx
        .query_row(
            "SELECT languages_locked FROM project_settings WHERE singleton=1",
            [],
            |r| r.get(0),
        )
        .map_err(storage_error)?;
    if locked {
        return if current.choices == *choices {
            Ok(current.revision)
        } else {
            Err(AppError::invalid("projectLanguagesImmutable"))
        };
    }
    let profiles: std::collections::BTreeMap<_, _> = [
        ("book_translation", &choices.book_translation_profile),
        ("assistant", &choices.assistant_profile),
    ]
    .into_iter()
    .filter_map(|(k, v)| v.as_ref().map(|v| (k, v)))
    .collect();
    let revision = next(expected.value()?)?;
    tx.execute("UPDATE project_settings SET source_language=?1,target_language=?2,profiles_json=?3,revision=?4,languages_locked=1 WHERE singleton=1",params![choices.source_language,choices.target_language,serde_json::to_string(&profiles).map_err(|_|AppError::invalid("profiles"))?,revision]).map_err(storage_error)?;
    tx.commit().map_err(storage_error)?;
    Ok(Revision(revision.to_string()))
}

pub(crate) fn validate_fixed_languages(db: &Connection) -> Result<(), AppError> {
    let valid:bool=db.query_row("SELECT languages_locked=1 AND source_language IS NOT NULL AND length(trim(source_language))>0 AND length(trim(target_language))>0 FROM project_settings WHERE singleton=1",[],|r|r.get(0)).map_err(storage_error)?;
    if !valid {
        return Err(AppError::invalid("projectLanguagesUnconfirmed"));
    }
    Ok(())
}

pub(super) fn next(value: i64) -> Result<i64, AppError> {
    value
        .checked_add(1)
        .ok_or_else(|| AppError::invalid("revision"))
}

/// Conservative invalidation; stage-specific dependency narrowing belongs to P04/P05.
pub(super) fn invalidate(tx: &Transaction<'_>) -> Result<(), AppError> {
    tx.execute(
        "UPDATE book_translations SET status='needs_review' WHERE status='ready' AND provenance!='reference'",
        [],
    )
    .map_err(storage_error)?;
    Ok(())
}

pub fn glossary_revision(db: &Connection) -> Result<Revision, AppError> {
    db.query_row(
        "SELECT revision FROM glossary_state WHERE singleton=1",
        [],
        |r| r.get::<_, i64>(0),
    )
    .map(|v| Revision(v.to_string()))
    .map_err(storage_error)
}

/// `expected=None` creates a term; existing entries always require a revision.
pub fn put_term(
    db: &mut Connection,
    term: &GlossaryTerm,
    expected: Option<&Revision>,
) -> Result<Revision, AppError> {
    let tx = db.transaction().map_err(storage_error)?;
    let revision = put_term_in(&tx, term, expected)?;
    tx.commit().map_err(storage_error)?;
    Ok(revision)
}

pub fn put_term_in(
    tx: &Transaction<'_>,
    term: &GlossaryTerm,
    expected: Option<&Revision>,
) -> Result<Revision, AppError> {
    if term.id.is_empty() || term.source.trim().is_empty() || term.target.trim().is_empty() {
        return Err(AppError::invalid("term"));
    }
    let previous: Option<(String, String)> = tx
        .query_row(
            "SELECT source,target FROM glossary_terms WHERE id=?1",
            [&term.id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(storage_error)?;
    let revision = if let Some(expected) = expected {
        let expected = expected.value()?;
        let revision = next(expected)?;
        let changed = tx.execute("UPDATE glossary_terms SET source=?1,target=?2,kind=?3,pinned=?4,frequency=?5,revision=?6 WHERE id=?7 AND revision=?8",params![term.source,term.target,term.kind,term.pinned,term.frequency,revision,term.id,expected]).map_err(storage_error)?;
        if changed != 1 {
            return Err(missing_or_conflict(
                tx,
                "SELECT 1 FROM glossary_terms WHERE id=?1",
                &term.id,
            )?);
        }
        revision
    } else {
        tx.execute("INSERT INTO glossary_terms(id,source,target,kind,pinned,frequency) VALUES(?1,?2,?3,?4,?5,?6)",params![term.id,term.source,term.target,term.kind,term.pinned,term.frequency]).map_err(storage_error)?;
        0
    };
    bump_glossary(tx)?;
    if previous
        .as_ref()
        .is_none_or(|(source, target)| source != &term.source || target != &term.target)
    {
        invalidate_term(tx, &term.source)?;
        if let Some((source, _)) = previous {
            invalidate_term(tx, &source)?;
        }
    }
    Ok(Revision(revision.to_string()))
}

pub fn delete_term(db: &mut Connection, id: &str, expected: &Revision) -> Result<(), AppError> {
    let tx = db.transaction().map_err(storage_error)?;
    delete_term_in(&tx, id, expected)?;
    tx.commit().map_err(storage_error)
}

pub fn delete_term_in(tx: &Transaction<'_>, id: &str, expected: &Revision) -> Result<(), AppError> {
    let source: Option<String> = tx
        .query_row("SELECT source FROM glossary_terms WHERE id=?1", [id], |r| {
            r.get(0)
        })
        .optional()
        .map_err(storage_error)?;
    if tx
        .execute(
            "DELETE FROM glossary_terms WHERE id=?1 AND revision=?2",
            params![id, expected.value()?],
        )
        .map_err(storage_error)?
        != 1
    {
        return Err(missing_or_conflict(
            tx,
            "SELECT 1 FROM glossary_terms WHERE id=?1",
            id,
        )?);
    }
    bump_glossary(tx)?;
    if let Some(source) = source {
        invalidate_term(tx, &source)?;
    }
    Ok(())
}

fn invalidate_term(tx: &Transaction<'_>, source: &str) -> Result<(), AppError> {
    tx.execute("UPDATE book_translations SET status='needs_review' WHERE status='ready' AND provenance!='reference' AND chapter_id IN (SELECT id FROM book_chapters WHERE instr(source_title,?1)>0 UNION SELECT chapter_id FROM book_source_blocks WHERE instr(text,?1)>0)", [source]).map_err(storage_error)?;
    Ok(())
}

pub(crate) fn bump_glossary(tx: &Transaction<'_>) -> Result<(), AppError> {
    tx.execute(
        "UPDATE glossary_state SET revision=revision+1 WHERE singleton=1",
        [],
    )
    .map_err(storage_error)?;

    Ok(())
}

pub(super) fn missing_or_conflict(
    db: &Connection,
    query: &str,
    id: &str,
) -> Result<AppError, AppError> {
    let exists = db
        .query_row(query, [id], |_| Ok(()))
        .optional()
        .map_err(storage_error)?
        .is_some();
    Ok(if exists { conflict() } else { not_found() })
}

pub fn glossary(db: &Connection) -> Result<Vec<GlossaryTerm>, AppError> {
    let mut query = db.prepare("SELECT id,source,target,kind,pinned,frequency,revision FROM glossary_terms ORDER BY source,id").map_err(storage_error)?;
    let terms = query
        .query_map([], |r| {
            Ok(GlossaryTerm {
                id: r.get(0)?,
                source: r.get(1)?,
                target: r.get(2)?,
                kind: r.get(3)?,
                pinned: r.get(4)?,
                frequency: r.get(5)?,
                revision: Revision(r.get::<_, i64>(6)?.to_string()),
            })
        })
        .map_err(storage_error)?;
    terms.collect::<Result<Vec<_>, _>>().map_err(storage_error)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryMessage {
    pub id: String,
    pub role: String,
    pub content: String,
    pub created_at: String,
}

/// Store the application's sanitized textual transcript, never provider request payloads.
pub fn append_message(db: &Connection, message: &HistoryMessage) -> Result<(), AppError> {
    if message.id.is_empty()
        || !["user", "assistant", "tool"].contains(&message.role.as_str())
        || message.content.len() > 1024 * 1024
    {
        return Err(AppError::invalid("message"));
    }
    db.execute("INSERT INTO assistant_messages(id,position,role,content,created_at) SELECT ?1,COALESCE(MAX(position)+1,0),?2,?3,?4 FROM assistant_messages",params![message.id,message.role,message.content,message.created_at]).map_err(storage_error)?;
    Ok(())
}

pub fn history(db: &Connection, limit: u32) -> Result<Vec<HistoryMessage>, AppError> {
    if limit == 0 || limit > 1000 {
        return Err(AppError::invalid("limit"));
    }
    let mut query=db.prepare("SELECT id,role,content,created_at FROM (SELECT * FROM assistant_messages ORDER BY position DESC LIMIT ?1) ORDER BY position").map_err(storage_error)?;
    let rows = query
        .query_map([limit], |r| {
            Ok(HistoryMessage {
                id: r.get(0)?,
                role: r.get(1)?,
                content: r.get(2)?,
                created_at: r.get(3)?,
            })
        })
        .map_err(storage_error)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(storage_error)
}
