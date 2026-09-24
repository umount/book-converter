//! Full reference translations seed untranslated chapters, preserving legacy origin semantics.
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
    number: Option<usize>,
    title: String,
    text: String,
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
    // Reject in-flight responses to the previous reference. Existing authored
    // translations remain finished; reference import never replaces them.
    tx.execute("UPDATE book_chapters SET revision=revision+1", [])
        .map_err(storage_error)?;
    tx.execute(
        "UPDATE book_translations SET source_revision=source_revision+1 WHERE status IN ('ready','needs_review')",
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
                number: chapter.number,
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
        auto_map(&tx, &chapters)?;
        adopt(&tx)?;
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
        adopt(&tx)?;
    }
    tx.commit().map_err(storage_error)?;
    Ok(view)
}

fn auto_map(tx: &Transaction<'_>, references: &[ImportedChapter]) -> Result<(), AppError> {
    let mut q = tx
        .prepare("SELECT id,display_number,source_title FROM book_chapters ORDER BY position")
        .map_err(storage_error)?;
    let sources = q
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, Option<usize>>(1)?,
                r.get::<_, String>(2)?,
            ))
        })
        .map_err(storage_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(storage_error)?;
    let numbered: std::collections::HashMap<_, _> = sources
        .iter()
        .filter_map(|(id, number, title)| {
            number
                .or_else(|| crate::book::fb2::heading_number(title).map(|v| v.0))
                .map(|n| (n, id))
        })
        .collect();
    let matches: Vec<_> = references
        .iter()
        .filter_map(|r| r.number.and_then(|n| numbered.get(&n)).map(|id| (*id, r)))
        .collect();
    let pairs = if matches.is_empty() {
        sources
            .iter()
            .zip(references)
            .map(|(s, r)| (&s.0, r))
            .collect()
    } else {
        matches
    };
    for (id, r) in pairs {
        tx.execute(
            "INSERT OR IGNORE INTO book_reference_mappings(chapter_id,reference_id) VALUES(?1,?2)",
            rusqlite::params![id, r.id],
        )
        .map_err(storage_error)?;
    }
    Ok(())
}

// A reference is a complete authored chapter, not a model prompt or an excerpt.
// Keep its body intact in the first text block; no invented paragraph alignment.
fn adopt(tx: &Transaction<'_>) -> Result<(), AppError> {
    use crate::{
        app::contracts::Revision,
        storage::{results, shared},
    };
    let settings = shared::settings(tx)?;
    let mut q = tx.prepare("SELECT c.id,c.revision,r.title,r.text FROM book_reference_mappings m JOIN book_chapters c ON c.id=m.chapter_id JOIN book_reference_chapters r ON r.id=m.reference_id WHERE NOT EXISTS(SELECT 1 FROM book_translations t WHERE t.chapter_id=c.id AND t.target_language=?1) ORDER BY c.position DESC").map_err(storage_error)?;
    let rows = q
        .query_map([&settings.choices.target_language], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
            ))
        })
        .map_err(storage_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(storage_error)?;
    let fingerprint = read(tx)?.fingerprint;
    for (chapter, revision, title, text) in rows {
        if text.trim().is_empty() {
            continue;
        }
        let mut q=tx.prepare("SELECT id FROM book_source_blocks WHERE chapter_id=?1 AND kind IN ('text','caption') ORDER BY position").map_err(storage_error)?;
        let ids = q
            .query_map([&chapter], |r| r.get::<_, String>(0))
            .map_err(storage_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(storage_error)?;
        if ids.is_empty() {
            continue;
        }
        let tail = text
            .chars()
            .rev()
            .take(1200)
            .collect::<String>()
            .chars()
            .rev()
            .collect::<String>();
        let blocks = ids
            .into_iter()
            .enumerate()
            .map(|(i, id)| (id, if i == 0 { text.clone() } else { String::new() }))
            .collect();
        let id = uuid::Uuid::new_v4().to_string();
        results::save_translation_in(
            tx,
            &results::BookTranslation {
                id: id.clone(),
                chapter_id: chapter,
                inputs: results::InputVersions {
                    source: Revision(revision.to_string()),
                    settings: settings.revision.clone(),
                    glossary: shared::glossary_revision(tx)?,
                },
                expected_translation: None,
                title,
                provenance: "reference".into(),
                context_fingerprint: fingerprint.clone(),
                blocks,
            },
        )?;
        results::save_context(
            tx,
            &results::BookContext {
                id: uuid::Uuid::new_v4().to_string(),
                translation_id: id,
                translation_revision: Revision("0".into()),
                summary: String::new(),
                previous_tail: tail,
                predecessor_id: None,
            },
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::{repository::ProjectRepository, results, shared, tests::database};
    #[test]
    fn seeds_full_reference_by_number_and_preserves_existing_translation() {
        let mut db = database(ProjectKind::Book);
        for (id, position, number) in [("a", 0, 3), ("b", 1, 7)] {
            db.execute("INSERT INTO book_chapters(id,position,display_number,source_title) VALUES(?1,?2,?3,'Chapter')",rusqlite::params![id,position,number]).unwrap();
            db.execute("INSERT INTO book_source_blocks(id,chapter_id,position,kind,text) VALUES(?1,?1,0,'text','Source')",[id]).unwrap();
        }
        let full = format!(
            "{}\n\nFINAL REFERENCE PARAGRAPH",
            "Полный перевод. ".repeat(1000)
        );
        let refs = vec![ImportedChapter {
            id: "r".into(),
            position: 0,
            number: Some(7),
            title: "Глава 7".into(),
            text: full.clone(),
        }];
        db.execute(
            "INSERT INTO book_reference_chapters VALUES('r',0,'Глава 7',?1)",
            [&full],
        )
        .unwrap();
        let tx = db.transaction().unwrap();
        auto_map(&tx, &refs).unwrap();
        adopt(&tx).unwrap();
        tx.commit().unwrap();
        let view = ProjectRepository::new(&mut db, ProjectKind::Book)
            .unwrap()
            .chapter("b")
            .unwrap();
        assert_eq!(view.status, "done");
        assert_eq!(view.translation.unwrap().origin, "reference");
        assert_eq!(
            view.blocks[0].translated_text.as_deref(),
            Some(full.as_str())
        );
        assert!(ProjectRepository::new(&mut db, ProjectKind::Book)
            .unwrap()
            .chapter("a")
            .unwrap()
            .translation
            .is_none());
        db.execute("UPDATE book_reference_chapters SET text='Replacement'", [])
            .unwrap();
        let tx = db.transaction().unwrap();
        invalidate(&tx).unwrap();
        adopt(&tx).unwrap();
        tx.commit().unwrap();
        let view = ProjectRepository::new(&mut db, ProjectKind::Book)
            .unwrap()
            .chapter("b")
            .unwrap();
        assert_eq!(
            view.blocks[0].translated_text.as_deref(),
            Some(full.as_str())
        );
        let options = crate::app::requests::TranslationOptions {
            max_chapters: 10,
            extract_glossary: false,
            force: false,
            instructions: None,
        };
        let target = shared::settings(&db).unwrap().choices.target_language;
        assert_eq!(
            crate::application::runtime::select_batch(
                &db,
                &crate::app::contracts::EntitySelection::All,
                &options,
                &target
            )
            .unwrap(),
            vec!["a"]
        );
        let mut choices = shared::settings(&db).unwrap().choices;
        choices.book_translation_profile = Some("changed".into());
        shared::update_settings(
            &mut db,
            &crate::app::contracts::Revision("0".into()),
            &choices,
        )
        .unwrap();
        let view = ProjectRepository::new(&mut db, ProjectKind::Book)
            .unwrap()
            .chapter("b")
            .unwrap();
        let t = view.translation.unwrap();
        assert_eq!(t.status, "ready");
        results::edit_translation_block(&mut db, &t.id, "b", &t.revision, "Исправленный перевод")
            .unwrap();
        let view = ProjectRepository::new(&mut db, ProjectKind::Book)
            .unwrap()
            .chapter("b")
            .unwrap();
        assert_eq!(view.translation.unwrap().origin, "manual");
    }
}
