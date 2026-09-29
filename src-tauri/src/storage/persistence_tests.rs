use super::{reference, results, runs, shared, tests::database};
use crate::app::contracts::{ErrorCode, JobState, ProjectKind, Revision};
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
fn glossary_marks_only_matching_chapters_and_ignores_pin_changes() {
    let mut db = book();
    results::save_translation(&mut db, &translation()).unwrap();
    let mut term = shared::GlossaryTerm {
        id: "term".into(),
        source: "Absent".into(),
        target: "Target".into(),
        kind: "term".into(),
        pinned: false,
        frequency: 0,
        revision: rev(0),
    };
    let status = |db: &rusqlite::Connection| {
        db.query_row("SELECT status FROM book_translations", [], |r| {
            r.get::<_, String>(0)
        })
        .unwrap()
    };
    shared::put_term(&mut db, &term, None).unwrap();
    assert_eq!(status(&db), "ready");
    term.source = "Source".into();
    shared::put_term(&mut db, &term, Some(&rev(0))).unwrap();
    assert_eq!(status(&db), "needs_review");
    db.execute("UPDATE book_translations SET status='ready'", [])
        .unwrap();
    term.pinned = true;
    shared::put_term(&mut db, &term, Some(&rev(1))).unwrap();
    assert_eq!(status(&db), "ready");
    term.target = "Changed".into();
    shared::put_term(&mut db, &term, Some(&rev(2))).unwrap();
    assert_eq!(status(&db), "needs_review");
    db.execute("UPDATE book_translations SET status='ready'", [])
        .unwrap();
    shared::delete_term(&mut db, "term", &rev(3)).unwrap();
    assert_eq!(status(&db), "needs_review");
}

#[test]
fn glossary_and_settings_are_atomic_versioned_and_invalidate_results() {
    let mut db = book();
    results::save_translation(&mut db, &translation()).unwrap();
    let mut choices = shared::settings(&db).unwrap().choices;
    choices.book_translation_profile = Some("other".into());
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
fn run_recovery_keeps_successful_steps_and_excludes_concurrent_mutators() {
    let mut db = book();
    let snapshot = runs::RunSnapshot {
        retarget: None,
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
    runs::transition(&mut db, "run", &rev(4), JobState::Queued, None, "resume").unwrap();
    runs::transition(&mut db, "run", &rev(5), JobState::Running, None, "resume").unwrap();
    assert!(runs::begin_step(
        &mut db,
        &runs::StepAttempt {
            attempt: 2,
            id: "duplicate".into(),
            ..step
        }
    )
    .is_err());
    runs::transition(&mut db, "run", &rev(6), JobState::Succeeded, None, "done").unwrap();
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

#[test]
fn chapter_instructions_are_revision_guarded_and_invalidate_translation() {
    let mut db = book();
    results::save_translation(&mut db, &translation()).unwrap();
    let mut repository =
        super::repository::ProjectRepository::new(&mut db, ProjectKind::Book).unwrap();
    assert_eq!(
        repository
            .update_chapter_instructions("chapter", &rev(0), "Keep honorifics")
            .unwrap(),
        rev(1)
    );
    assert_eq!(
        repository
            .update_chapter_instructions("chapter", &rev(0), "Stale editor")
            .unwrap_err()
            .code,
        ErrorCode::RevisionConflict
    );
    let chapter = repository.chapter("chapter").unwrap();
    assert_eq!(chapter.instructions, "Keep honorifics");
    assert_eq!(chapter.translation.unwrap().status, "needs_review");
}

#[test]
fn languages_can_only_be_chosen_when_finalizing_an_import() {
    let mut db = rusqlite::Connection::open_in_memory().unwrap();
    db.execute_batch(include_str!("schema.sql")).unwrap();
    db.execute("INSERT INTO project_settings(singleton,kind,target_language,languages_locked) VALUES(1,'book','und',0)",[]).unwrap();
    let mut choices = shared::settings(&db).unwrap().choices;
    choices.source_language = Some("zh".into());
    choices.target_language = "ru".into();
    assert_eq!(
        shared::finalize_import_settings(&mut db, &rev(0), &choices).unwrap(),
        rev(1)
    );
    assert_eq!(
        shared::finalize_import_settings(&mut db, &rev(1), &choices).unwrap(),
        rev(1)
    );
    let mut changed = choices.clone();
    changed.target_language = "en".into();
    assert!(shared::update_settings(&mut db, &rev(1), &changed).is_err());
    assert!(shared::finalize_import_settings(&mut db, &rev(1), &changed).is_err());
    assert!(db
        .execute("UPDATE project_settings SET target_language='en'", [])
        .is_err());
    assert!(db
        .execute("UPDATE project_settings SET source_language='ja'", [])
        .is_err());
    assert!(db
        .execute("UPDATE project_settings SET languages_locked=0", [])
        .is_err());
    choices.book_translation_profile = Some("new-model".into());
    assert_eq!(
        shared::update_settings(&mut db, &rev(1), &choices).unwrap(),
        rev(2)
    );
    assert_eq!(shared::settings(&db).unwrap().choices, choices);
    let json = serde_json::json!({"projectId":crate::app::contracts::ProjectId::new(),"expectedRevision":"2","choices":{"languages":{"source":"en","target":"ja"},"bookTranslationProfile":null,"assistantProfile":null}});
    assert!(
        serde_json::from_value::<crate::app::requests::ProjectSettingsUpdateArgs>(json).is_err()
    );
}

#[test]
fn extracted_glossary_preserves_edits_counts_occurrences_and_rejects_late_publication() {
    use crate::application::book_glossary::{self, ExtractedTerm, GlossaryOutput};
    let mut db = book();
    let pinned = shared::GlossaryTerm {
        id: "pinned".into(),
        source: "Source".into(),
        target: "Manual".into(),
        kind: "name".into(),
        pinned: true,
        frequency: 0,
        revision: rev(0),
    };
    shared::put_term(&mut db, &pinned, None).unwrap();
    let mut translated = translation();
    translated.inputs.glossary = rev(1);
    results::save_translation(&mut db, &translated).unwrap();
    let output = |id: &str, g: Revision| GlossaryOutput {
        id: id.into(),
        chapter: "chapter".into(),
        source_revision: rev(0),
        settings_revision: rev(0),
        glossary_revision: g,
        terms: vec![
            (
                ExtractedTerm {
                    source: "Source".into(),
                    target: "AI override".into(),
                    kind: "term".into(),
                },
                2,
            ),
            (
                ExtractedTerm {
                    source: "New".into(),
                    target: "Новый".into(),
                    kind: "term".into(),
                },
                1,
            ),
        ],
    };
    let tx = db.transaction().unwrap();
    book_glossary::persist(&tx, output("first", rev(1))).unwrap();
    tx.commit().unwrap();
    let terms = shared::glossary(&db).unwrap();
    let term = terms.iter().find(|t| t.id == "pinned").unwrap();
    assert!(term.pinned);
    assert_eq!(term.target, "Manual");
    assert_eq!(term.kind, "name");
    assert_eq!(term.frequency, 2);
    assert_eq!(terms.len(), 2);
    assert_eq!(
        db.query_row("SELECT status FROM book_translations", [], |r| r
            .get::<_, String>(0))
            .unwrap(),
        "ready"
    );
    let revision = shared::glossary_revision(&db).unwrap();
    let tx = db.transaction().unwrap();
    book_glossary::persist(&tx, output("second", revision.clone())).unwrap();
    tx.commit().unwrap();
    assert_eq!(shared::glossary_revision(&db).unwrap(), revision);
    assert_eq!(count(&db, "book_term_occurrences"), 2);
    let tx = db.transaction().unwrap();
    assert_eq!(
        book_glossary::persist(&tx, output("late", rev(0)))
            .unwrap_err()
            .code,
        ErrorCode::RevisionConflict
    );
    drop(tx);
    assert_eq!(count(&db, "book_glossary_results"), 2);
}

#[test]
fn glossary_api_paginates_and_rejects_a_stale_editor() {
    use crate::{
        app::{
            contracts::{ProjectId, TermId},
            requests::*,
        },
        application::preferences,
    };
    let mut db = book();
    let project = ProjectId::new();
    let mut args = GlossaryPutArgs {
        project_id: project.clone(),
        term_id: TermId("a".into()),
        source: "Alpha".into(),
        target: "A".into(),
        kind: "term".into(),
        pinned: true,
        expected_revision: None,
        expected_settings_revision: rev(0),
    };
    preferences::put_term(&mut db, &args).unwrap();
    args.term_id = TermId("b".into());
    args.source = "Beta".into();
    preferences::put_term(&mut db, &args).unwrap();
    let mut list = GlossaryListArgs {
        query: String::new(),
        pinned_only: false,
        project_id: project,
        cursor: None,
        limit: 1,
    };
    let first = preferences::glossary_page(&mut db, &list).unwrap();
    assert_eq!(first.total, 2);
    assert_eq!(first.items[0].source, "Alpha");
    list.cursor = first.next_cursor;
    let second = preferences::glossary_page(&mut db, &list).unwrap();
    assert_eq!(second.items[0].source, "Beta");
    assert!(second.next_cursor.is_none());
    args.expected_revision = Some(rev(0));
    args.target = "New".into();
    assert_eq!(preferences::put_term(&mut db, &args).unwrap(), rev(1));
    assert_eq!(
        preferences::put_term(&mut db, &args).unwrap_err().code,
        ErrorCode::RevisionConflict
    );
    list.cursor = None;
    list.query = "New".into();
    assert_eq!(preferences::glossary_page(&mut db, &list).unwrap().total, 1);
    list.query = "%".into();
    assert_eq!(preferences::glossary_page(&mut db, &list).unwrap().total, 0);
    args.expected_revision = Some(rev(1));
    args.target = "爷爷".into();
    args.pinned = false;
    preferences::put_term(&mut db, &args).unwrap();
    list.query = "爷".into();
    assert_eq!(preferences::glossary_page(&mut db, &list).unwrap().total, 1);
    list.pinned_only = true;
    assert_eq!(preferences::glossary_page(&mut db, &list).unwrap().total, 0);
    list.query.clear();
    let pinned = preferences::glossary_page(&mut db, &list).unwrap();
    assert_eq!(pinned.total, 1);
    assert_eq!(pinned.items[0].source, "Alpha");
    let mut settings = shared::settings(&db).unwrap().choices;
    settings.book_translation_profile = Some("changed".into());
    shared::update_settings(&mut db, &rev(0), &settings).unwrap();
    args.expected_revision = Some(rev(1));
    assert_eq!(
        preferences::put_term(&mut db, &args).unwrap_err().code,
        ErrorCode::RevisionConflict
    );
}

#[test]
fn replacement_after_glossary_change_preserves_review_and_rejects_later_changes() {
    use crate::{
        app::{
            contracts::{EntitySelection, ProjectId},
            requests::BookReplacePreviewArgs,
        },
        application::book_edit,
    };
    let mut db = book();
    results::save_translation(&mut db, &translation()).unwrap();
    let args = BookReplacePreviewArgs {
        project_id: ProjectId::new(),
        selection: EntitySelection::All,
        search: "Translated".into(),
        replacement: "Corrected".into(),
        case_sensitive: true,
    };
    let old_preview = book_edit::preview(&mut db, &args).unwrap();
    db.execute("UPDATE glossary_state SET revision=revision+1", [])
        .unwrap();
    db.execute("UPDATE book_translations SET status='needs_review'", [])
        .unwrap();
    assert_eq!(
        book_edit::apply(&mut db, old_preview).unwrap_err().code,
        ErrorCode::RevisionConflict
    );
    let current = book_edit::preview(&mut db, &args).unwrap();
    assert_eq!(book_edit::apply(&mut db, current).unwrap(), 1);
    let (status, text): (String, String) = db.query_row(
        "SELECT status,translated_text FROM book_translations JOIN book_translation_blocks ON translation_id=book_translations.id ORDER BY revision DESC LIMIT 1",
        [], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
    assert_eq!(status, "needs_review");
    assert_eq!(text, "Corrected");
    let second = BookReplacePreviewArgs {
        search: "Corrected".into(),
        replacement: "Next".into(),
        ..args
    };
    let pending = book_edit::preview(&mut db, &second).unwrap();
    db.execute("UPDATE project_settings SET revision=revision+1", [])
        .unwrap();
    assert_eq!(
        book_edit::apply(&mut db, pending).unwrap_err().code,
        ErrorCode::RevisionConflict
    );
    assert_eq!(count(&db, "book_translations"), 2);
}

#[test]
fn manual_editor_can_correct_outdated_translation_without_clearing_review() {
    let mut db = book();
    let original = translation();
    results::save_translation(&mut db, &original).unwrap();
    db.execute("UPDATE glossary_state SET revision=1", [])
        .unwrap();
    db.execute("UPDATE project_settings SET revision=1", [])
        .unwrap();
    db.execute("UPDATE book_chapters SET revision=1", [])
        .unwrap();
    db.execute("UPDATE book_translations SET status='needs_review'", [])
        .unwrap();
    assert_eq!(
        results::edit_translation_block(
            &mut db,
            &original.id,
            "text",
            &rev(0),
            "Manual correction"
        )
        .unwrap(),
        rev(1)
    );
    let (status, text): (String, String) = db.query_row(
        "SELECT status,translated_text FROM book_translations JOIN book_translation_blocks ON translation_id=book_translations.id ORDER BY revision DESC LIMIT 1", [],
        |r| Ok((r.get(0)?,r.get(1)?))).unwrap();
    assert_eq!(status, "needs_review");
    assert_eq!(text, "Manual correction");
    assert_eq!(
        results::edit_translation_block(&mut db, &original.id, "text", &rev(0), "Late edit")
            .unwrap_err()
            .code,
        ErrorCode::RevisionConflict
    );
}

#[test]
fn reference_lists_omit_bodies_and_fingerprint_includes_full_text() {
    use crate::application::book_reference;
    let db = book();
    let text = "🙂正文".repeat(10000);
    db.execute("INSERT INTO book_reference_chapters(id,position,title,text) VALUES('large',0,'Long reference',?1)", [&text]).unwrap();
    let view = book_reference::read(&db).unwrap();
    let json = serde_json::to_string(&view).unwrap();
    assert!(json.len() < 1024);
    assert!(!json.contains("正文"));
    db.execute(
        "UPDATE book_reference_chapters SET text='Updated' WHERE id='large'",
        [],
    )
    .unwrap();
    assert_ne!(
        book_reference::read(&db).unwrap().fingerprint,
        view.fingerprint
    );
}

#[test]
fn legacy_origin_and_language_flags_follow_manual_corrections() {
    use super::repository::ProjectRepository;
    let mut db = book();
    db.execute("UPDATE project_settings SET target_language='ru'", [])
        .unwrap();
    let mut value = translation();
    value.title = "Глава".into();
    value.blocks[0].1 = "Перевод 未翻译 foreignword".into();
    results::save_translation(&mut db, &value).unwrap();
    let view = ProjectRepository::new(&mut db, ProjectKind::Book)
        .unwrap()
        .chapter("chapter")
        .unwrap();
    assert_eq!(view.status, "done");
    assert_eq!(view.translation.unwrap().origin, "model");
    assert!(view.lang_issues.contains(&"未翻译".into()));
    assert!(view.lang_issues.contains(&"foreignword".into()));
    results::edit_translation_block(&mut db, &value.id, "text", &rev(0), "Исправленный перевод")
        .unwrap();
    let view = ProjectRepository::new(&mut db, ProjectKind::Book)
        .unwrap()
        .chapter("chapter")
        .unwrap();
    assert_eq!(view.translation.unwrap().origin, "manual");
    assert!(view.lang_issues.is_empty());
}

#[test]
fn bulk_correction_keeps_legacy_manual_origin_for_reference() {
    use crate::{
        app::{
            contracts::{EntitySelection, ProjectId},
            requests::BookReplacePreviewArgs,
        },
        application::book_edit,
    };
    let mut db = book();
    let mut value = translation();
    value.provenance = "reference".into();
    results::save_translation(&mut db, &value).unwrap();
    db.execute("UPDATE glossary_state SET revision=1", [])
        .unwrap();
    let preview = book_edit::preview(
        &mut db,
        &BookReplacePreviewArgs {
            project_id: ProjectId::new(),
            selection: EntitySelection::All,
            search: "Translated".into(),
            replacement: "Corrected".into(),
            case_sensitive: true,
        },
    )
    .unwrap();
    book_edit::apply(&mut db, preview).unwrap();
    let view = super::repository::ProjectRepository::new(&mut db, ProjectKind::Book)
        .unwrap()
        .chapter("chapter")
        .unwrap();
    let t = view.translation.unwrap();
    assert_eq!(t.origin, "manual");
    assert_eq!(t.status, "ready");
}

#[test]
fn title_revision_preserves_reference_body_origin_and_context() {
    let mut db = book();
    let mut value = translation();
    value.provenance = "reference".into();
    results::save_translation(&mut db, &value).unwrap();
    results::save_context(
        &db,
        &results::BookContext {
            id: "ctx".into(),
            translation_id: value.id.clone(),
            translation_revision: rev(0),
            summary: "Summary".into(),
            previous_tail: "Tail".into(),
            predecessor_id: None,
        },
    )
    .unwrap();
    assert_eq!(
        results::edit_translation_title(&mut db, &value.id, &rev(0), "Исправленный заголовок")
            .unwrap(),
        rev(1)
    );
    let view = super::repository::ProjectRepository::new(&mut db, ProjectKind::Book)
        .unwrap()
        .chapter("chapter")
        .unwrap();
    assert_eq!(
        view.chapter.translated_title.as_deref(),
        Some("Исправленный заголовок")
    );
    let title = view.translation.unwrap();
    assert_eq!(title.title, "Исправленный заголовок");
    assert_eq!(title.origin, "reference");
    assert_eq!(title.status, "ready");
    assert_eq!(
        view.blocks[0].translated_text.as_deref(),
        Some("Translated")
    );
    let context:(String,String,i64)=db.query_row("SELECT summary,previous_tail,translation_revision FROM book_contexts WHERE translation_id=?1",[&title.id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).unwrap();
    assert_eq!(context, ("Summary".into(), "Tail".into(), 1));
    assert_eq!(
        results::edit_translation_title(&mut db, &value.id, &rev(0), "Late title")
            .unwrap_err()
            .code,
        ErrorCode::RevisionConflict
    );
    db.execute(
        "UPDATE book_translations SET status='needs_review' WHERE id=?1",
        [&title.id],
    )
    .unwrap();
    results::edit_translation_title(&mut db, &title.id, &rev(1), "Next title").unwrap();
    let view = super::repository::ProjectRepository::new(&mut db, ProjectKind::Book)
        .unwrap()
        .chapter("chapter")
        .unwrap();
    assert_eq!(view.translation.unwrap().status, "needs_review");
}

#[test]
fn chapter_list_states_match_reader_without_loading_chapter_bodies() {
    let mut db = book();
    let flags = |db: &rusqlite::Connection| {
        db.query_row(
            "SELECT status,origin,needs_review FROM book_chapter_states WHERE id='chapter'",
            [],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, Option<String>>(1)?,
                    r.get::<_, bool>(2)?,
                ))
            },
        )
        .unwrap()
    };
    assert_eq!(flags(&db), ("pending".into(), None, false));
    let mut value = translation();
    value.provenance = "reference".into();
    results::save_translation(&mut db, &value).unwrap();
    assert_eq!(flags(&db), ("done".into(), Some("reference".into()), false));
    db.execute("UPDATE book_translations SET status='needs_review'", [])
        .unwrap();
    assert_eq!(flags(&db), ("done".into(), Some("reference".into()), true));
    let view = super::repository::ProjectRepository::new(&mut db, ProjectKind::Book)
        .unwrap()
        .chapter("chapter")
        .unwrap();
    assert_eq!(view.chapter.status, view.status);
    assert_eq!(view.chapter.origin.as_deref(), Some("reference"));
    assert!(view.chapter.needs_review);
    db.execute(
        "INSERT INTO book_chapters(id,position,source_title) VALUES('empty',1,'No text')",
        [],
    )
    .unwrap();
    assert_eq!(
        db.query_row(
            "SELECT status FROM book_chapter_states WHERE id='empty'",
            [],
            |r| r.get::<_, String>(0)
        )
        .unwrap(),
        "skipped"
    );
}

#[test]
fn automatic_glossary_growth_preserves_previous_translation_review_state() {
    use crate::application::book_glossary::{persist, ExtractedTerm, GlossaryOutput};
    let mut db = book();
    results::save_translation(&mut db, &translation()).unwrap();
    db.execute(
        "INSERT INTO book_chapters(id,position,source_title) VALUES('next',1,'Next')",
        [],
    )
    .unwrap();
    for (index, frequency) in [1, 2, 3].into_iter().enumerate() {
        if index == 2 {
            db.execute("UPDATE book_translations SET status='needs_review'", [])
                .unwrap();
        }
        let glossary_revision = shared::glossary_revision(&db).unwrap();
        let tx = db.transaction().unwrap();
        persist(
            &tx,
            GlossaryOutput {
                id: format!("glossary-{index}"),
                chapter: "next".into(),
                source_revision: rev(0),
                settings_revision: rev(0),
                glossary_revision,
                terms: vec![(
                    ExtractedTerm {
                        source: "Name".into(),
                        target: "Имя".into(),
                        kind: "name".into(),
                    },
                    frequency,
                )],
            },
        )
        .unwrap();
        tx.commit().unwrap();
        assert_eq!(
            shared::glossary_revision(&db).unwrap(),
            rev(index as u32 + 1)
        );
        let expected = if index == 2 { "needs_review" } else { "ready" };
        assert_eq!(
            db.query_row("SELECT status FROM book_translations", [], |r| r
                .get::<_, String>(0))
                .unwrap(),
            expected
        );
        assert_eq!(
            db.query_row(
                "SELECT needs_review FROM book_chapter_states WHERE id='chapter'",
                [],
                |r| r.get::<_, bool>(0)
            )
            .unwrap(),
            index == 2
        );
    }
}

#[test]
fn active_step_locks_previous_chapter_and_releases_it_when_advancing() {
    let mut db = book();
    let value = translation();
    results::save_translation(&mut db, &value).unwrap();
    for (id, position) in [("next", 10), ("third", 11)] {
        db.execute(
            "INSERT INTO book_chapters(id,position,source_title) VALUES(?1,?2,'Title')",
            rusqlite::params![id, position],
        )
        .unwrap();
        db.execute("INSERT INTO book_source_blocks(id,chapter_id,position,kind,text) VALUES(?1,?1,0,'text','Source')", [id]).unwrap();
    }
    db.execute("INSERT INTO job_runs(id,kind,state,settings_snapshot,created_at,updated_at) VALUES('busy','book_translation','running','{}','0','0')", []).unwrap();
    db.execute("INSERT INTO job_steps(id,run_id,entity_kind,entity_id,stage,attempt,input_fingerprint,state) VALUES('step','busy','chapter','next','translation',1,'test','running')", []).unwrap();
    let title =
        results::edit_translation_title(&mut db, &value.id, &rev(0), "New title").unwrap_err();
    let body = results::edit_translation_block(
        &mut db,
        &value.id,
        &value.blocks[0].0,
        &rev(0),
        "New body",
    )
    .unwrap_err();
    assert_eq!(title.params["field"], "chapterEditBusy");
    assert_eq!(body.params["field"], "chapterEditBusy");
    db.execute("UPDATE job_steps SET entity_id='third'", [])
        .unwrap();
    results::edit_translation_title(&mut db, &value.id, &rev(0), "New title").unwrap();
}

#[test]
fn reference_replacement_rolls_back_and_history_is_ordered() {
    let mut db = book();
    let reference = || reference::ReferenceChapter {
        id: "ref".into(),
        position: 0,
        title: "Reference".into(),
        text: "Text".into(),
    };
    reference::replace_reference(&mut db, &[reference()], &[("chapter".into(), "ref".into())])
        .unwrap();
    assert!(
        reference::replace_reference(&mut db, &[], &[("chapter".into(), "missing".into())])
            .is_err()
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
fn reference_mapping_rejects_stale_views_and_invalidates_inflight_translation() {
    use crate::{
        app::{
            contracts::{ChapterId, ProjectId},
            requests::{BookReferenceMapArgs, ReferenceMapping},
        },
        application::book_reference,
    };
    let mut db = book();
    reference::replace_reference(
        &mut db,
        &[reference::ReferenceChapter {
            id: "ref".into(),
            position: 0,
            title: "Reference".into(),
            text: "Reference text".into(),
        }],
        &[],
    )
    .unwrap();
    results::save_translation(&mut db, &translation()).unwrap();
    let before = book_reference::read(&db).unwrap();
    let mut args = BookReferenceMapArgs {
        project_id: ProjectId::new(),
        expected_fingerprint: before.fingerprint.clone(),
        mappings: vec![ReferenceMapping {
            chapter_id: ChapterId("chapter".into()),
            reference_id: "ref".into(),
        }],
    };
    let after = book_reference::map(&mut db, &args).unwrap();
    assert_ne!(before.fingerprint, after.fingerprint);
    assert_eq!(
        db.query_row("SELECT revision FROM book_chapters", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        1
    );
    assert_eq!(
        book_reference::map(&mut db, &args).unwrap_err().code,
        ErrorCode::RevisionConflict
    );
    let mut late = translation();
    late.id = "late".into();
    late.expected_translation = Some(rev(0));
    assert_eq!(
        results::save_translation(&mut db, &late).unwrap_err().code,
        ErrorCode::RevisionConflict
    );
    args.expected_fingerprint = after.fingerprint.clone();
    // Reapplying identical correspondence is a no-op for chapter revisions.
    book_reference::map(&mut db, &args).unwrap();
    assert_eq!(
        db.query_row("SELECT revision FROM book_chapters", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        1
    );
    args.mappings[0].reference_id = "missing".into();
    assert!(book_reference::map(&mut db, &args).is_err());
    assert_eq!(book_reference::read(&db).unwrap(), after);
}
