use super::{edits, results, runs, shared, tests::database};
use crate::app::{
    contracts::{ErrorCode, JobState, MangaStage, PixelBounds, ProjectKind, Revision},
    requests::{RegionPatch, ReviewDecision},
};
fn rev(value: u32) -> Revision {
    Revision(value.to_string())
}
fn book() -> rusqlite::Connection {
    let db = database(ProjectKind::Book);
    db.execute(
        "INSERT INTO book_chapters(id,position,source_title) VALUES('chapter',0,'Title')",
        [],
    )
    .unwrap();
    db.execute("INSERT INTO book_source_blocks(id,chapter_id,position,kind,text) VALUES('text','chapter',0,'text','Source')",[]).unwrap();
    db
}
fn manga() -> rusqlite::Connection {
    let db = database(ProjectKind::Manga);
    db.execute("INSERT INTO assets(id,relative_path,mime,byte_length,width,height) VALUES(?1,'assets/a.png','image/png',10,100,100)",["a".repeat(64)]).unwrap();
    db.execute(
        "INSERT INTO manga_volumes VALUES('volume',0,'Volume','rtl')",
        [],
    )
    .unwrap();
    db.execute("INSERT INTO manga_pages(id,volume_id,position,original_asset_id,width,height) VALUES('page','volume',0,?1,100,100)",["a".repeat(64)]).unwrap();
    db
}
fn inputs(source: u32, settings: u32, glossary: u32) -> results::InputVersions {
    results::InputVersions {
        source: rev(source),
        settings: rev(settings),
        glossary: rev(glossary),
    }
}
fn translation() -> results::BookTranslation {
    results::BookTranslation {
        id: "translation".into(),
        chapter_id: "chapter".into(),
        inputs: inputs(0, 0, 0),
        expected_translation: None,
        title: "Title".into(),
        provenance: "fake-v1".into(),
        context_fingerprint: "ctx-v1".into(),
        blocks: vec![("text".into(), "Translated".into())],
    }
}
fn count(db: &rusqlite::Connection, table: &str) -> i64 {
    db.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
        .unwrap()
}

#[test]
fn glossary_and_settings_are_atomic_versioned_and_invalidate_results() {
    let mut db = book();
    results::save_translation(&mut db, &translation()).unwrap();
    let mut choices = shared::settings(&db).unwrap().choices;
    choices.target_language = "en".into();
    assert_eq!(
        shared::update_settings(&mut db, &rev(0), &choices).unwrap(),
        rev(1)
    );
    assert_eq!(
        shared::update_settings(&mut db, &rev(0), &choices)
            .unwrap_err()
            .code,
        ErrorCode::RevisionConflict
    );
    assert_eq!(
        db.query_row("SELECT status FROM book_translations", [], |r| r
            .get::<_, String>(0))
            .unwrap(),
        "needs_review"
    );
    let mut term = shared::GlossaryTerm {
        id: "term".into(),
        source: "source".into(),
        target: "target".into(),
        kind: "name".into(),
        pinned: true,
        frequency: 1,
        revision: rev(0),
    };
    shared::put_term(&mut db, &term, None).unwrap();
    assert_eq!(shared::glossary_revision(&db).unwrap(), rev(1));
    term.target = "new".into();
    assert_eq!(
        shared::put_term(&mut db, &term, Some(&rev(0))).unwrap(),
        rev(1)
    );
    assert_eq!(
        shared::put_term(&mut db, &term, Some(&rev(0)))
            .unwrap_err()
            .code,
        ErrorCode::RevisionConflict
    );
    assert_eq!(shared::glossary_revision(&db).unwrap(), rev(2));
    assert_eq!(shared::glossary(&db).unwrap()[0].target, "new");
    shared::delete_term(&mut db, "term", &rev(1)).unwrap();
    assert_eq!(shared::glossary_revision(&db).unwrap(), rev(3));
    assert!(shared::glossary(&db).unwrap().is_empty());
    assert_eq!(
        shared::delete_term(&mut db, "term", &rev(1))
            .unwrap_err()
            .code,
        ErrorCode::NotFound
    );
}

#[test]
fn translation_rejects_partial_duplicate_unknown_and_late_results() {
    let mut db = book();
    let mut value = translation();
    for blocks in [
        vec![],
        vec![("other".into(), "bad".into())],
        vec![("text".into(), "a".into()), ("text".into(), "b".into())],
    ] {
        value.blocks = blocks;
        assert!(results::save_translation(&mut db, &value).is_err());
        assert_eq!(count(&db, "book_translations"), 0);
    }
    value = translation();
    assert_eq!(results::save_translation(&mut db, &value).unwrap(), rev(0));
    value.id = "late".into();
    assert_eq!(
        results::save_translation(&mut db, &value).unwrap_err().code,
        ErrorCode::RevisionConflict
    );
    value.expected_translation = Some(rev(0));
    db.execute("UPDATE book_chapters SET revision=1", [])
        .unwrap();
    assert_eq!(
        results::save_translation(&mut db, &value).unwrap_err().code,
        ErrorCode::RevisionConflict
    );
    assert_eq!(count(&db, "book_translations"), 1);
    let context = results::BookContext {
        id: "context".into(),
        translation_id: "translation".into(),
        translation_revision: rev(0),
        summary: "Summary".into(),
        previous_tail: "Tail".into(),
        predecessor_id: None,
    };
    assert!(results::save_context(&db, &context).is_err());
    db.execute("UPDATE book_chapters SET revision=0", [])
        .unwrap();
    results::save_context(&db, &context).unwrap();
    assert_eq!(count(&db, "book_contexts"), 1);
}

#[test]
fn source_revision_overflow_rolls_back_text_and_parent_together() {
    let mut db = book();
    db.execute("UPDATE book_chapters SET revision=9223372036854775807", [])
        .unwrap();
    let mut repo = super::repository::ProjectRepository::new(&mut db, ProjectKind::Book).unwrap();
    assert!(repo
        .update_book_text("text", &rev(0), "Must roll back")
        .is_err());
    assert_eq!(
        db.query_row("SELECT text FROM book_source_blocks", [], |r| r
            .get::<_, String>(0))
            .unwrap(),
        "Source"
    );
}

#[test]
fn manga_results_and_edits_preserve_revisions_and_reject_dangling_assets() {
    let mut db = manga();
    let mut value = results::MangaResult {
        id: "result".into(),
        page_id: "page".into(),
        stage: MangaStage::Lettering,
        inputs: inputs(0, 0, 0),
        expected_result: None,
        fingerprint: "input-hash".into(),
        provider_version: "fake-v1".into(),
        output: results::MangaOutput::Image("b".repeat(64)),
    };
    assert!(results::save_manga_result(&mut db, &value).is_err());
    assert_eq!(count(&db, "manga_results"), 0);
    value.output = results::MangaOutput::Image("a".repeat(64));
    results::save_manga_result(&mut db, &value).unwrap();
    edits::review_result(&db, "result", &rev(0), ReviewDecision::Approved).unwrap();
    let region = edits::NewRegion {
        id: "region".into(),
        order: 0,
        category: "dialogue".into(),
        bounds: PixelBounds {
            x: 0.,
            y: 0.,
            width: 50.,
            height: 50.,
        },
        source_text: "Source".into(),
    };
    assert_eq!(
        edits::insert_regions(&mut db, "page", &rev(0), &[region]).unwrap(),
        rev(1)
    );
    assert!(edits::review_result(&db, "result", &rev(0), ReviewDecision::Approved).is_err());
    assert_eq!(
        edits::update_region(
            &mut db,
            "region",
            &rev(0),
            &RegionPatch::TranslatedText {
                text: "Manual correction".into()
            }
        )
        .unwrap(),
        rev(1)
    );
    assert!(edits::update_region(
        &mut db,
        "region",
        &rev(0),
        &RegionPatch::SourceText {
            text: "Late OCR".into()
        }
    )
    .is_err());
    assert!(db
        .query_row("SELECT translation_manual FROM manga_regions", [], |r| r
            .get::<_, bool>(
            0
        ))
        .unwrap());

    value.id = "late".into();
    value.expected_result = Some(rev(0));
    assert_eq!(
        results::save_manga_result(&mut db, &value)
            .unwrap_err()
            .code,
        ErrorCode::RevisionConflict
    );
    assert_eq!(
        edits::save_mask(
            &mut db,
            "mask",
            "page",
            Some("region"),
            &"a".repeat(64),
            &rev(2)
        )
        .unwrap(),
        rev(3)
    );
    edits::update_region(
        &mut db,
        "region",
        &rev(1),
        &RegionPatch::Bounds {
            bounds: PixelBounds {
                x: 1.,
                y: 1.,
                width: 50.,
                height: 50.,
            },
        },
    )
    .unwrap();
    assert_eq!(count(&db, "manga_masks"), 0);
}

#[test]
fn run_recovery_keeps_successful_steps_and_excludes_concurrent_mutators() {
    let mut db = book();
    let snapshot = runs::RunSnapshot {
        settings: shared::settings(&db).unwrap().choices,
        settings_revision: rev(0),
        glossary_revision: rev(0),
        selected_ids: vec!["chapter".into()],
        prompt_version: "v1".into(),
        stages: vec!["translation".into()],
        provider: None,
        instructions: None,
    };
    runs::create_run(&mut db, "run", "book_translation", &snapshot, "now").unwrap();
    assert!(runs::create_run(&mut db, "other", "book_translation", &snapshot, "now").is_err());
    runs::transition(&mut db, "run", &rev(0), JobState::Running, None, "now").unwrap();
    assert!(runs::transition(&mut db, "run", &rev(1), JobState::Succeeded, None, "now").is_err());
    let step = runs::StepAttempt {
        id: "step".into(),
        run_id: "run".into(),
        entity_kind: "chapter".into(),
        entity_id: "chapter".into(),
        stage: "translation".into(),
        attempt: 1,
        input_fingerprint: "hash".into(),
    };
    runs::begin_step(&mut db, &step).unwrap();
    assert!(runs::transition(&mut db, "run", &rev(1), JobState::Succeeded, None, "now").is_err());
    let tx = db.transaction().unwrap();
    runs::finish_step(&tx, "step", "translation-result", 10).unwrap();
    tx.commit().unwrap();
    assert_eq!(runs::interrupt_running(&mut db, "restart").unwrap(), 1);
    assert_eq!(
        runs::get_run(&db, "run").unwrap().state,
        JobState::Interrupted
    );
    assert_eq!(
        db.query_row("SELECT state FROM job_steps", [], |r| r.get::<_, String>(0))
            .unwrap(),
        "succeeded"
    );
    runs::transition(&mut db, "run", &rev(3), JobState::Queued, None, "resume").unwrap();
    runs::transition(&mut db, "run", &rev(4), JobState::Running, None, "resume").unwrap();
    assert!(runs::begin_step(
        &mut db,
        &runs::StepAttempt {
            attempt: 2,
            id: "duplicate".into(),
            ..step
        }
    )
    .is_err());
    runs::transition(&mut db, "run", &rev(5), JobState::Succeeded, None, "done").unwrap();
}

#[test]
fn reference_replacement_rolls_back_and_history_is_ordered() {
    let mut db = book();
    let reference = || edits::ReferenceChapter {
        id: "ref".into(),
        position: 0,
        title: "Reference".into(),
        text: "Text".into(),
    };
    edits::replace_reference(&mut db, &[reference()], &[("chapter".into(), "ref".into())]).unwrap();
    assert!(
        edits::replace_reference(&mut db, &[], &[("chapter".into(), "missing".into())]).is_err()
    );
    assert_eq!(count(&db, "book_reference_mappings"), 1);
    for id in ["1", "2", "3"] {
        shared::append_message(
            &db,
            &shared::HistoryMessage {
                id: id.into(),
                role: "user".into(),
                content: format!("Message {id}"),
                created_at: "now".into(),
            },
        )
        .unwrap();
    }
    assert_eq!(
        shared::history(&db, 2)
            .unwrap()
            .iter()
            .map(|m| m.id.as_str())
            .collect::<Vec<_>>(),
        vec!["2", "3"]
    );
}

#[test]
fn manual_translation_edit_publishes_snapshot_and_rejects_stale_editor() {
    let mut db = book();
    let first = results::save_translation(&mut db, &translation()).unwrap();
    let edited =
        results::edit_translation_block(&mut db, "translation", "text", &first, "Edited").unwrap();
    assert_ne!(first, edited);
    assert!(
        results::edit_translation_block(&mut db, "translation", "text", &first, "Lost update")
            .is_err()
    );
    let view = super::repository::ProjectRepository::new(&mut db, ProjectKind::Book)
        .unwrap()
        .chapter("chapter")
        .unwrap();
    assert_eq!(view.blocks[0].translated_text.as_deref(), Some("Edited"));
    assert_eq!(view.translation.unwrap().revision, edited);
    assert_eq!(
        db.query_row(
            "SELECT translated_text FROM book_translation_blocks WHERE translation_id='translation'",
            [],
            |r| r.get::<_, String>(0)
        )
        .unwrap(),
        "Translated"
    );
}

#[test]
fn replacement_preview_is_literal_project_scoped_and_atomic() {
    use crate::{
        app::{
            contracts::{EntitySelection, ProjectId},
            requests::BookReplacePreviewArgs,
        },
        application::book_edit,
    };
    let mut db = book();
    results::save_translation(&mut db, &translation()).unwrap();
    db.execute(
        "INSERT INTO book_chapters(id,position,source_title) VALUES('second',1,'Second')",
        [],
    )
    .unwrap();
    db.execute("INSERT INTO book_source_blocks(id,chapter_id,position,kind,text) VALUES('second-text','second',0,'text','Source')",[]).unwrap();
    let mut second = translation();
    second.id = "second-translation".into();
    second.chapter_id = "second".into();
    second.blocks = vec![("second-text".into(), "Translated".into())];
    results::save_translation(&mut db, &second).unwrap();
    let args = BookReplacePreviewArgs {
        project_id: ProjectId::new(),
        selection: EntitySelection::All,
        search: "translated".into(),
        replacement: "$1 literal".into(),
        case_sensitive: false,
    };
    let prepared = book_edit::preview(&mut db, &args).unwrap();
    assert_eq!(prepared.view.changes.len(), 2);
    assert!(prepared
        .view
        .changes
        .iter()
        .all(|c| c.after == "$1 literal"));
    let cache = book_edit::EditPreviews::default();
    let view = cache.insert(prepared).unwrap();
    assert!(cache.take(&ProjectId::new(), &view.preview_id).is_err());
    let prepared = cache.take(&args.project_id, &view.preview_id).unwrap();
    assert!(cache.take(&args.project_id, &view.preview_id).is_err());
    results::edit_translation_block(
        &mut db,
        "second-translation",
        "second-text",
        &rev(0),
        "Intervening edit",
    )
    .unwrap();
    assert_eq!(
        book_edit::apply(&mut db, prepared).unwrap_err().code,
        ErrorCode::RevisionConflict
    );
    // The first chapter would have been written before the second conflict: it must roll back.
    assert_eq!(count(&db, "book_translations"), 3);
    let view = super::repository::ProjectRepository::new(&mut db, ProjectKind::Book)
        .unwrap()
        .chapter("chapter")
        .unwrap();
    assert_eq!(
        view.blocks[0].translated_text.as_deref(),
        Some("Translated")
    );
    let prepared = book_edit::preview(&mut db, &args).unwrap();
    assert_eq!(book_edit::apply(&mut db, prepared).unwrap(), 1);
    let view = super::repository::ProjectRepository::new(&mut db, ProjectKind::Book)
        .unwrap()
        .chapter("chapter")
        .unwrap();
    assert_eq!(
        view.blocks[0].translated_text.as_deref(),
        Some("$1 literal")
    );
}
