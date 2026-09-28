//! Cross-service acceptance with real imports/exports and an offline provider.
use super::{book::BookPipeline, book_export, book_presentation, book_search, runtime};
use crate::{
    ai::{Completion, Provider, ProviderProfile, Request, Usage},
    app::{contracts::*, requests::*},
    project::lifecycle::ProjectManager,
    storage::{repository::ProjectRepository, results, runs, shared},
};
use std::{
    future::Future,
    pin::Pin,
    sync::{atomic::AtomicBool, Arc},
};

struct OfflineProvider(ProviderProfile);
impl Provider for OfflineProvider {
    fn profile(&self) -> &ProviderProfile {
        &self.0
    }
    fn complete(
        &self,
        request: Request,
    ) -> Pin<Box<dyn Future<Output = Result<Completion, AppError>> + Send + '_>> {
        Box::pin(async move {
            let Request::Structured { system, user } = request else {
                panic!("structured requests required")
            };
            let mut input: serde_json::Value = serde_json::from_str(&user).unwrap();
            let text = if system.starts_with("Translate every") {
                assert!(system.contains("Keep the narrative calm."));
                for segment in input["segments"].as_array_mut().unwrap() {
                    segment["text"] =
                        "The traveler returned to the quiet garden by the sea.".into();
                }
                input.to_string()
            } else {
                assert!(system.starts_with("Update the rolling story summary"));
                serde_json::json!({"summary":"The traveler returned to the garden."}).to_string()
            };
            Ok(Completion {
                text,
                finish_reason: "stop".into(),
                usage: Usage::default(),
                tool_calls: vec![],
            })
        })
    }
}
struct Scratch(std::path::PathBuf);
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[tokio::test]
async fn imported_book_survives_batch_edit_reopen_search_and_export() {
    let scratch =
        Scratch(std::env::temp_dir().join(format!("book-workflow-{}", uuid::Uuid::new_v4())));
    std::fs::create_dir_all(&scratch.0).unwrap();
    let root = scratch.0.join("projects");
    let manager = ProjectManager::new(root.clone());
    let source =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests/fixtures/structural.epub");
    let inspected = manager.inspect_source(ProjectKind::Book, &source).unwrap();
    let project = manager
        .create(
            &inspected.import_id.0,
            &ProjectChoices {
                name: "Workflow".into(),
                languages: LanguagePair {
                    source: Some("en".into()),
                    target: "en".into(),
                },
                processing_profile_id: None,
            },
        )
        .unwrap();
    let provider = Arc::new(OfflineProvider(ProviderProfile {
        id: "offline".into(),
        base_url: "https://unused.invalid".into(),
        model: "fixture".into(),
        temperature: 0.,
        max_output_tokens: 1000,
        context_window_tokens: crate::ai::default_context_window_tokens(),
        timeout_seconds: 1,
        network_retries: 0,
    }));
    let options = TranslationOptions {
        max_chapters: 1,
        extract_glossary: false,
        force: false,
        instructions: None,
    };
    let chapter_id = manager
        .lease(&project.id)
        .unwrap()
        .with_connection(|db, _| {
            let old = book_presentation::read(db)?;
            book_presentation::update(
                db,
                &UpdateBookPresentationArgs {
                    project_id: project.id.clone(),
                    title: Some("The Garden".into()),
                    author: Some("Fixture Author".into()),
                    summary: Some("An offline acceptance book.".into()),
                    instructions: "Keep the narrative calm.".into(),
                    expected_revision: old.revision,
                },
            )?;
            let settings = shared::settings(db)?;
            let selected = runtime::select_batch(
                db,
                &EntitySelection::All,
                &options,
                &settings.choices.target_language,
            )?;
            assert_eq!(selected.len(), 1);
            let chapter = selected[0].clone();
            runs::create_run(
                db,
                "workflow-job",
                "book_translation",
                &runs::RunSnapshot {
                    manga: None,
            retarget: None,
                    settings: settings.choices,
                    settings_revision: settings.revision,
                    glossary_revision: shared::glossary_revision(db)?,
                    selected_ids: selected,
                    prompt_version: "book-segments-v1".into(),
                    stages: vec!["translation".into(), "context".into()],
                    provider: Some(provider.profile().clone()),
                    instructions: Some("Keep the narrative calm.".into()),
                },
                "1",
            )?;
            Ok(chapter)
        })
        .unwrap();
    crate::jobs::durable::execute(
        &manager,
        &project.id,
        "workflow-job",
        &BookPipeline {
            provider,
            instructions: Some("Keep the narrative calm.".into()),
        },
        Arc::new(AtomicBool::new(false)),
        |_| {},
    )
    .await
    .unwrap();
    let edited = "The traveler found a silver compass in the garden.";
    let block_id = manager
        .lease(&project.id)
        .unwrap()
        .with_connection(|db, _| {
            let chapter = ProjectRepository::new(db, ProjectKind::Book)?.chapter(&chapter_id)?;
            let translation = chapter.translation.unwrap();
            let block = chapter
                .blocks
                .iter()
                .find(|b| b.translated_text.is_some())
                .unwrap();
            results::edit_translation_block(
                db,
                &translation.id,
                &block.id,
                &translation.revision,
                edited,
            )?;
            let count: i64 = db
                .query_row(
                    "SELECT COUNT(*) FROM book_translations WHERE status='ready'",
                    [],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(count, 1, "batch must not translate the rest of the book");
            Ok(block.id.clone())
        })
        .unwrap();
    drop(manager);
    let manager = ProjectManager::new(root);
    assert_eq!(manager.open(&project.id).unwrap().id, project.id);
    manager
        .lease(&project.id)
        .unwrap()
        .with_connection(|db, _| {
            assert_eq!(
                book_presentation::read(db)?.title.as_deref(),
                Some("The Garden")
            );
            let matches = book_search::search(
                db,
                &BookSearchArgs {
                    project_id: project.id.clone(),
                    query: "silver compass".into(),
                    side: BookSearchSide::Translation,
                    case_sensitive: false,
                    cursor: None,
                    limit: 10,
                },
            )?;
            assert_eq!(matches.matches.len(), 1);
            assert_eq!(matches.matches[0].block_id.0, block_id);
            let next = runtime::select_batch(db, &EntitySelection::All, &options, "en")?;
            assert_eq!(next.len(), 1);
            assert_ne!(
                next[0], chapter_id,
                "next batch must skip the edited translation"
            );
            assert_eq!(
                runs::get_run(db, "workflow-job")?.state,
                JobState::Succeeded
            );
            Ok(())
        })
        .unwrap();
    let destination = scratch.0.join("translated.epub");
    let mut args = BookExportArgs {
        project_id: project.id.clone(),
        selection: EntitySelection::All,
        destination: destination.to_string_lossy().into_owned(),
        overwrite: false,
        format: BookExportFormat::Epub,
        incomplete_policy: IncompletePolicy::Reject,
    };
    assert!(book_export::export_book(&manager, &args).is_err());
    assert!(
        !destination.exists(),
        "failed export must not publish a partial book"
    );
    args.incomplete_policy = IncompletePolicy::Originals;
    book_export::export_book(&manager, &args).unwrap();
    let bytes = std::fs::read(&destination).unwrap();
    assert!(book_export::export_book(&manager, &args).is_err());
    assert_eq!(
        std::fs::read(&destination).unwrap(),
        bytes,
        "export must not overwrite an existing file"
    );
    let imported = manager
        .inspect_source(ProjectKind::Book, &destination)
        .unwrap();
    let copy = manager
        .create(
            &imported.import_id.0,
            &ProjectChoices {
                name: "Export roundtrip".into(),
                languages: LanguagePair {
                    source: Some("en".into()),
                    target: "ru".into(),
                },
                processing_profile_id: None,
            },
        )
        .unwrap();
    manager
        .lease(&copy.id)
        .unwrap()
        .with_connection(|db, _| {
            let found = book_search::search(
                db,
                &BookSearchArgs {
                    project_id: copy.id.clone(),
                    query: edited.into(),
                    side: BookSearchSide::Source,
                    case_sensitive: true,
                    cursor: None,
                    limit: 10,
                },
            )?;
            assert_eq!(
                found.matches.len(),
                1,
                "export must contain the newest manual revision"
            );
            let images: i64 = db
                .query_row(
                    "SELECT COUNT(*) FROM book_source_blocks WHERE kind='image'",
                    [],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(
                images, 3,
                "image occurrences must survive the export roundtrip"
            );
            Ok(())
        })
        .unwrap();
}
