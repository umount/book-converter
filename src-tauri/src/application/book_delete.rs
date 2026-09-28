//! Remove one chapter atomically, preserving shared assets and the original file.
use crate::{
    app::{
        contracts::{AppError, ProjectKind},
        requests::DeleteChapterArgs,
    },
    storage::repository::{conflict, storage_error, ProjectRepository},
};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior};

pub fn delete(db: &mut Connection, args: &DeleteChapterArgs) -> Result<(), AppError> {
    ProjectRepository::new(db, ProjectKind::Book)?;
    let tx = db
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(storage_error)?;
    let active: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM job_runs WHERE state IN ('queued','running','cancelling'))", [], |r| r.get(0)).map_err(storage_error)?;
    if active {
        return Err(AppError::invalid("chapterDeleteBusy"));
    }
    let row: Option<(i64, i64)> = tx
        .query_row(
            "SELECT revision,position FROM book_chapters WHERE id=?1",
            [&args.chapter_id.0],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(storage_error)?;
    let (revision, position) = row.ok_or_else(|| AppError::invalid("chapterDeleted"))?;
    let translation: Option<i64> = tx.query_row("SELECT MAX(revision) FROM book_translations WHERE chapter_id=?1 AND target_language=(SELECT target_language FROM project_settings WHERE singleton=1)", [&args.chapter_id.0], |r| r.get(0)).map_err(storage_error)?;
    if revision != args.expected_revision.value()?
        || translation
            != args
                .expected_translation_revision
                .as_ref()
                .map(|r| r.value())
                .transpose()?
    {
        return Err(conflict());
    }
    let had_context: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM book_contexts c JOIN book_translations t ON t.id=c.translation_id WHERE t.chapter_id=?1)", [&args.chapter_id.0], |r|r.get(0)).map_err(storage_error)?;
    if had_context {
        // Later rolling summaries may still contain events from the removed chapter.
        // Keep translations, but stop passing those summaries to future requests.
        tx.execute("DELETE FROM book_contexts WHERE translation_id IN (SELECT t.id FROM book_translations t JOIN book_chapters c ON c.id=t.chapter_id WHERE c.position>?1)", [position]).map_err(storage_error)?;
    }
    tx.execute(
        "DELETE FROM book_glossary_results WHERE chapter_id=?1",
        [&args.chapter_id.0],
    )
    .map_err(storage_error)?;
    tx.execute(
        "DELETE FROM book_chapters WHERE id=?1",
        [&args.chapter_id.0],
    )
    .map_err(storage_error)?;
    // Foreign keys remove source blocks, all translation revisions, their contexts,
    // reference mappings and occurrences. Shared reference texts/assets remain.
    let changed = tx.execute("UPDATE glossary_terms SET frequency=(SELECT COALESCE(SUM(frequency),0) FROM book_term_occurrences o WHERE o.source=glossary_terms.source),revision=revision+1 WHERE frequency!=(SELECT COALESCE(SUM(frequency),0) FROM book_term_occurrences o WHERE o.source=glossary_terms.source)", []).map_err(storage_error)?;
    if changed > 0 {
        tx.execute(
            "UPDATE glossary_state SET revision=revision+1 WHERE singleton=1",
            [],
        )
        .map_err(storage_error)?;
    }
    let ids = {
        let mut q = tx
            .prepare("SELECT id FROM book_chapters ORDER BY position")
            .map_err(storage_error)?;
        let rows = q
            .query_map([], |r| r.get::<_, String>(0))
            .map_err(storage_error)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(storage_error)?
    };
    for (index, id) in ids.iter().enumerate() {
        tx.execute(
            "UPDATE book_chapters SET position=?1 WHERE id=?2",
            rusqlite::params![index as i64, id],
        )
        .map_err(storage_error)?;
    }
    tx.commit().map_err(storage_error)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::contracts::{ChapterId, ErrorCode, ProjectId, Revision};
    fn fixture() -> Connection {
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch("PRAGMA foreign_keys=ON;").unwrap();
        db.execute_batch(include_str!("../storage/schema.sql"))
            .unwrap();
        db.execute_batch(include_str!("../storage/schema_extensions.sql"))
            .unwrap();
        db.execute_batch("INSERT INTO project_settings(singleton,kind,source_language,target_language) VALUES(1,'book','zh','ru');
            INSERT INTO book_chapters(id,position,display_number,source_title) VALUES('a',0,1,'Cover'),('b',1,2,'Story');
            INSERT INTO book_source_blocks(id,chapter_id,position,kind,text) VALUES('sa','a',0,'text','名'),('sb','b',0,'text','名');
            INSERT INTO book_translations(id,chapter_id,source_revision,provenance,target_language,translated_title,context_fingerprint,glossary_revision,revision,status) VALUES('ta','a',0,'test','ru','Cover','ctx',0,0,'ready'),('tb','b',0,'test','ru','Story','ctx',0,0,'ready');
            INSERT INTO book_translation_blocks VALUES('ta','a','sa','Имя'),('tb','b','sb','История');
            INSERT INTO book_contexts VALUES('ca','ta','Old summary','tail',0,NULL),('cb','tb','Summary with cover','tail',0,'ca');
            INSERT INTO book_reference_chapters VALUES('r',0,'Cover','Reference');
            INSERT INTO book_reference_mappings VALUES('a','r');
            INSERT INTO book_glossary_results VALUES('ga','a',0,0,'[]');
            INSERT INTO glossary_terms(id,source,target,kind,frequency) VALUES('term','名','Имя','name',5);
            INSERT INTO book_term_occurrences VALUES('a','名',3),('b','名',2);").unwrap();
        let asset = "a".repeat(64);
        db.execute("INSERT INTO assets(id,relative_path,mime,byte_length) VALUES(?1,'shared.png','image/png',1)",[&asset]).unwrap();
        for (id, ch) in [("ia", "a"), ("ib", "b")] {
            db.execute("INSERT INTO book_source_blocks(id,chapter_id,position,kind,asset_id) VALUES(?1,?2,1,'image',?3)",rusqlite::params![id,ch,asset]).unwrap();
        }
        db
    }
    fn args(id: &str) -> DeleteChapterArgs {
        DeleteChapterArgs {
            project_id: ProjectId::new(),
            chapter_id: ChapterId(id.into()),
            expected_revision: Revision("0".into()),
            expected_translation_revision: Some(Revision("0".into())),
        }
    }
    fn count(db: &Connection, table: &str) -> i64 {
        db.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
            .unwrap()
    }
    #[test]
    fn deletion_cleans_dependencies_without_deleting_shared_assets_or_other_translations() {
        let mut db = fixture();
        delete(&mut db, &args("a")).unwrap();
        assert_eq!(count(&db, "book_chapters"), 1);
        assert_eq!(count(&db, "book_source_blocks"), 2);
        assert_eq!(count(&db, "book_translations"), 1);
        assert_eq!(count(&db, "book_translation_blocks"), 1);
        assert_eq!(count(&db, "book_contexts"), 0);
        assert_eq!(count(&db, "book_reference_mappings"), 0);
        assert_eq!(count(&db, "book_reference_chapters"), 1);
        assert_eq!(count(&db, "book_glossary_results"), 0);
        assert_eq!(count(&db, "assets"), 1);
        assert_eq!(
            db.query_row("SELECT frequency FROM glossary_terms", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            2
        );
        assert_eq!(
            db.query_row(
                "SELECT position,display_number FROM book_chapters",
                [],
                |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?))
            )
            .unwrap(),
            (0, 2)
        );
        assert_eq!(
            db.query_row(
                "SELECT translated_text FROM book_translation_blocks",
                [],
                |r| r.get::<_, String>(0)
            )
            .unwrap(),
            "История"
        );
        assert!(!db
            .prepare("PRAGMA foreign_key_check")
            .unwrap()
            .exists([])
            .unwrap());
        delete(&mut db, &args("b")).unwrap();
        assert_eq!(count(&db, "book_chapters"), 0);
        assert_eq!(count(&db, "assets"), 1);
    }
    #[test]
    fn stale_confirmation_and_active_jobs_cannot_delete_anything() {
        let mut db = fixture();
        let mut stale = args("a");
        stale.expected_revision = Revision("1".into());
        assert_eq!(
            delete(&mut db, &stale).unwrap_err().code,
            ErrorCode::RevisionConflict
        );
        stale = args("a");
        stale.expected_translation_revision = None;
        assert_eq!(
            delete(&mut db, &stale).unwrap_err().code,
            ErrorCode::RevisionConflict
        );
        for state in ["queued", "running", "cancelling"] {
            db.execute("INSERT INTO job_runs(id,kind,state,settings_snapshot,created_at,updated_at) VALUES('job','book_translation',?1,'{}','0','0')",[state]).unwrap();
            assert!(delete(&mut db, &args("a")).is_err());
            assert_eq!(count(&db, "book_chapters"), 2);
            assert_eq!(count(&db, "book_contexts"), 2);
            db.execute("DELETE FROM job_runs", []).unwrap();
        }
    }
    #[test]
    fn deleting_an_untranslated_cover_keeps_unrelated_contexts() {
        let mut db = fixture();
        db.execute("DELETE FROM book_translations WHERE chapter_id='a'", [])
            .unwrap();
        let mut request = args("a");
        request.expected_translation_revision = None;
        delete(&mut db, &request).unwrap();
        assert_eq!(count(&db, "book_contexts"), 1);
    }
}
