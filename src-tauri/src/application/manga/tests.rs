use super::{pipeline::RecognitionPipeline, regions, runtime};
use crate::{
    ai::{Completion, Provider, ProviderProfile, Request},
    app::{
        contracts::{
            AppError, EntitySelection, ErrorCode, JobState, ProjectId, ProjectKind, Revision,
        },
        requests::{LanguagePair, ProjectChoices, RegionPatch},
    },
    jobs::durable::{self, StepExecutor},
    project::lifecycle::ProjectManager,
    storage::{edits, runs},
};
use std::{
    future::Future,
    pin::Pin,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc, Mutex,
    },
};
fn profile() -> ProviderProfile {
    ProviderProfile {
        id: "vision".into(),
        base_url: "https://fake.test".into(),
        model: "fake".into(),
        temperature: 0.0,
        max_output_tokens: 1000,
        timeout_seconds: 10,
        network_retries: 0,
    }
}
struct Fixture {
    root: std::path::PathBuf,
    manager: Arc<ProjectManager>,
    id: ProjectId,
}
impl Fixture {
    fn new(max: u32) -> Self {
        Self::with_source(
            max,
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../tests/fixtures/two-volumes.cbz"),
        )
    }
    fn with_source(max: u32, path: &std::path::Path) -> Self {
        let root = std::env::temp_dir().join(format!("manga-run-{}", uuid::Uuid::new_v4()));
        let manager = Arc::new(ProjectManager::new(root.clone()));
        let preview = manager.inspect_source(ProjectKind::Manga, path).unwrap();
        let project = manager
            .create(
                &preview.import_id.0,
                &ProjectChoices {
                    name: "Manga test".into(),
                    languages: LanguagePair {
                        source: Some("ja".into()),
                        target: "ru".into(),
                    },
                    processing_profile_id: None,
                },
            )
            .unwrap();
        manager
            .lease(&project.id)
            .unwrap()
            .with_connection(|db, _| {
                runtime::create(db, "run", &EntitySelection::All, max, false, profile())
            })
            .unwrap();
        Self {
            root,
            manager,
            id: project.id,
        }
    }
    fn run(&self) -> runs::RunRecord {
        self.manager
            .lease(&self.id)
            .unwrap()
            .with_connection(|db, _| runs::get_run(db, "run"))
            .unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}
struct Fake {
    profile: ProviderProfile,
    calls: AtomicUsize,
    fail_on: usize,
    text: Mutex<String>,
    hook: Option<Box<dyn Fn() + Send + Sync>>,
}
fn reply(x: u32) -> String {
    serde_json::json!({"regions":[{"id":"request-local","readingOrder":0,"category":"dialogue","bounds":{"x":x,"y":1,"width":2,"height":2},"sourceText":"source"}]}).to_string()
}
impl Fake {
    fn new(fail_on: usize) -> Self {
        Self {
            profile: profile(),
            calls: AtomicUsize::new(0),
            fail_on,
            text: Mutex::new(reply(1)),
            hook: None,
        }
    }
}
impl Provider for Fake {
    fn profile(&self) -> &ProviderProfile {
        &self.profile
    }
    fn complete(
        &self,
        request: Request,
    ) -> Pin<Box<dyn Future<Output = Result<Completion, AppError>> + Send + '_>> {
        assert!(matches!(request, Request::Vision { .. }));
        let call = self.calls.fetch_add(1, Ordering::SeqCst) + 1;
        Box::pin(async move {
            if let Some(hook) = &self.hook {
                hook();
            }
            if call == self.fail_on {
                return Err(AppError::invalid("injectedFailure"));
            }
            Ok(Completion {
                text: self.text.lock().unwrap().clone(),
                finish_reason: "stop".into(),
                usage: Default::default(),
                tool_calls: vec![],
            })
        })
    }
}
#[tokio::test]
async fn recognition_resumes_after_restart_without_repeating_published_pages() {
    let f = Fixture::new(2);
    let fake = Arc::new(Fake::new(2));
    let pipeline = RecognitionPipeline {
        provider: fake.clone(),
    };
    assert!(durable::execute(
        &f.manager,
        &f.id,
        "run",
        &pipeline,
        Arc::new(AtomicBool::new(false)),
        |_| {}
    )
    .await
    .is_err());
    assert_eq!(f.run().state, JobState::Failed);
    f.manager
        .lease(&f.id)
        .unwrap()
        .with_connection(|db, _| {
            assert_eq!(
                db.query_row("SELECT COUNT(*) FROM manga_results", [], |r| r
                    .get::<_, i64>(0))
                    .unwrap(),
                2
            );
            assert_eq!(
                db.query_row(
                    "SELECT COUNT(*) FROM job_steps WHERE state='succeeded'",
                    [],
                    |r| r.get::<_, i64>(0)
                )
                .unwrap(),
                1
            );
            // Simulate a restart after requeuing but before a new result is published.
            let run = runs::get_run(db, "run")?;
            let rev = runs::transition(db, "run", &run.revision, JobState::Queued, None, "queued")?;
            runs::transition(db, "run", &rev, JobState::Running, None, "running")?;
            runs::interrupt_running(db, "restart")?;
            Ok(())
        })
        .unwrap();
    let reopened = ProjectManager::new(f.root.clone());
    durable::execute(
        &reopened,
        &f.id,
        "run",
        &pipeline,
        Arc::new(AtomicBool::new(false)),
        |_| {},
    )
    .await
    .unwrap();
    assert_eq!(fake.calls.load(Ordering::SeqCst), 3);
    assert_eq!(f.run().state, JobState::Succeeded);
    reopened
        .lease(&f.id)
        .unwrap()
        .with_connection(|db, _| {
            assert_eq!(
                db.query_row(
                    "SELECT COUNT(*) FROM manga_results WHERE validity='current'",
                    [],
                    |r| r.get::<_, i64>(0)
                )
                .unwrap(),
                4
            );
            assert_eq!(
                db.query_row("SELECT COUNT(*) FROM manga_regions", [], |r| r
                    .get::<_, i64>(0))
                    .unwrap(),
                2
            );
            runtime::create(db, "next", &EntitySelection::All, 1, false, profile())?;
            assert_ne!(
                runs::get_run(db, "next")?.snapshot.selected_ids[0],
                f.run().snapshot.selected_ids[0]
            );
            Ok(())
        })
        .unwrap();
}
#[tokio::test]
async fn stale_response_cannot_publish_regions_or_successful_step() {
    let f = Fixture::new(1);
    let page = f.run().snapshot.selected_ids[0].clone();
    let manager = f.manager.clone();
    let project = f.id.clone();
    let mut fake = Fake::new(0);
    fake.hook = Some(Box::new(move || {
        manager
            .lease(&project)
            .unwrap()
            .with_connection(|db, _| {
                db.execute(
                    "UPDATE manga_pages SET revision=revision+1 WHERE id=?1",
                    [&page],
                )
                .unwrap();
                Ok(())
            })
            .unwrap();
    }));
    let error = durable::execute(
        &f.manager,
        &f.id,
        "run",
        &RecognitionPipeline {
            provider: Arc::new(fake),
        },
        Arc::new(AtomicBool::new(false)),
        |_| {},
    )
    .await
    .unwrap_err();
    assert_eq!(error.code, ErrorCode::RevisionConflict);
    f.manager
        .lease(&f.id)
        .unwrap()
        .with_connection(|db, _| {
            for table in ["manga_regions", "manga_results"] {
                assert_eq!(
                    db.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r
                        .get::<_, i64>(0))
                        .unwrap(),
                    0
                );
            }
            assert_eq!(
                db.query_row(
                    "SELECT COUNT(*) FROM job_steps WHERE state='succeeded'",
                    [],
                    |r| r.get::<_, i64>(0)
                )
                .unwrap(),
                0
            );
            Ok(())
        })
        .unwrap();
}
#[tokio::test]
async fn rerun_preserves_matched_edits_and_retains_unmatched_manual_regions_for_review() {
    let f = Fixture::new(1);
    let page = f.run().snapshot.selected_ids[0].clone();
    let fake = Arc::new(Fake::new(0));
    let pipeline = RecognitionPipeline {
        provider: fake.clone(),
    };
    durable::execute(
        &f.manager,
        &f.id,
        "run",
        &pipeline,
        Arc::new(AtomicBool::new(false)),
        |_| {},
    )
    .await
    .unwrap();
    let id = f
        .manager
        .lease(&f.id)
        .unwrap()
        .with_connection(|db, _| {
            let region = regions::read(db, &page)?.remove(0);
            let rev = edits::update_region(
                db,
                &region.id,
                &Revision(region.revision.to_string()),
                &RegionPatch::SourceText {
                    text: "manual source".into(),
                },
            )?;
            edits::update_region(
                db,
                &region.id,
                &rev,
                &RegionPatch::TranslatedText {
                    text: "manual target".into(),
                },
            )?;
            runtime::create(
                db,
                "rerun",
                &EntitySelection::ExplicitIds {
                    ids: vec![page.clone()],
                },
                1,
                true,
                profile(),
            )?;
            Ok(region.id)
        })
        .unwrap();
    durable::execute(
        &f.manager,
        &f.id,
        "rerun",
        &pipeline,
        Arc::new(AtomicBool::new(false)),
        |_| {},
    )
    .await
    .unwrap();
    f.manager
        .lease(&f.id)
        .unwrap()
        .with_connection(|db, _| {
            let region = regions::read(db, &page)?.remove(0);
            assert_eq!(region.id, id);
            assert_eq!(region.source_text, "manual source");
            assert_eq!(region.translated_text.as_deref(), Some("manual target"));
            runtime::create(
                db,
                "moved",
                &EntitySelection::ExplicitIds {
                    ids: vec![page.clone()],
                },
                1,
                true,
                profile(),
            )?;
            Ok(())
        })
        .unwrap();
    *fake.text.lock().unwrap() = reply(8);
    durable::execute(
        &f.manager,
        &f.id,
        "moved",
        &pipeline,
        Arc::new(AtomicBool::new(false)),
        |_| {},
    )
    .await
    .unwrap();
    f.manager.lease(&f.id).unwrap().with_connection(|db,_| {
        let all=regions::read(db,&page)?;assert_eq!(all.len(),2);
        assert!(all.iter().any(|r|r.id==id && r.source_text=="manual source"));
        assert_eq!(db.query_row("SELECT COUNT(*) FROM manga_reviews v JOIN manga_results r ON r.id=v.result_id WHERE r.validity='current' AND v.state='needs_review'",[],|r|r.get::<_,i64>(0)).unwrap(),2);
        assert_eq!(db.query_row("SELECT COUNT(*) FROM manga_results",[],|r|r.get::<_,i64>(0)).unwrap(),6);Ok(())
    }).unwrap();
}
#[tokio::test]
async fn conflicting_publication_rolls_back_region_mutations() {
    let f = Fixture::new(1);
    let run = f.run();
    let page = &run.snapshot.selected_ids[0];
    let lease = f.manager.lease(&f.id).unwrap();
    let pipeline = RecognitionPipeline {
        provider: Arc::new(Fake::new(0)),
    };
    let first = pipeline
        .compute(&lease, &run, page, "recognition")
        .await
        .unwrap();
    let second = pipeline
        .compute(&lease, &run, page, "recognition")
        .await
        .unwrap();
    lease
        .with_connection(|db, _| {
            let tx = db.transaction().unwrap();
            pipeline.persist(&tx, first)?;
            tx.commit().unwrap();
            let before = serde_json::to_value(regions::read(db, page)?).unwrap();
            {
                let tx = db.transaction().unwrap();
                assert_eq!(
                    pipeline.persist(&tx, second).unwrap_err().code,
                    ErrorCode::RevisionConflict
                );
            }
            assert_eq!(
                serde_json::to_value(regions::read(db, page)?).unwrap(),
                before
            );
            Ok(())
        })
        .unwrap();
}

#[tokio::test]
async fn cancelling_an_inflight_request_leaves_no_published_regions() {
    struct Pending {
        profile: ProviderProfile,
        started: tokio::sync::Notify,
    }
    impl Provider for Pending {
        fn profile(&self) -> &ProviderProfile {
            &self.profile
        }
        fn complete(
            &self,
            _: Request,
        ) -> Pin<Box<dyn Future<Output = Result<Completion, AppError>> + Send + '_>> {
            Box::pin(async move {
                self.started.notify_one();
                std::future::pending().await
            })
        }
    }
    let f = Fixture::new(1);
    let fake = Arc::new(Pending {
        profile: profile(),
        started: tokio::sync::Notify::new(),
    });
    let pipeline = RecognitionPipeline {
        provider: fake.clone(),
    };
    let cancel = Arc::new(AtomicBool::new(false));
    let (result, ()) = tokio::join!(
        durable::execute(&f.manager, &f.id, "run", &pipeline, cancel.clone(), |_| {}),
        async {
            fake.started.notified().await;
            cancel.store(true, Ordering::Release);
        }
    );
    assert_eq!(result.unwrap_err().code, ErrorCode::JobCancelled);
    assert_eq!(f.run().state, JobState::Cancelled);
    f.manager
        .lease(&f.id)
        .unwrap()
        .with_connection(|db, _| {
            assert_eq!(
                db.query_row("SELECT COUNT(*) FROM manga_results", [], |r| r
                    .get::<_, i64>(0))
                    .unwrap(),
                0
            );
            assert_eq!(
                db.query_row("SELECT COUNT(*) FROM manga_regions", [], |r| r
                    .get::<_, i64>(0))
                    .unwrap(),
                0
            );
            Ok(())
        })
        .unwrap();
}

#[test]
fn admission_rejects_zero_batches_wrong_domains_and_unsupported_stages() {
    let mut db = crate::storage::tests::database(ProjectKind::Manga);
    assert!(runtime::create(&mut db, "zero", &EntitySelection::All, 0, false, profile()).is_err());
    let mut db = crate::storage::tests::database(ProjectKind::Book);
    assert!(runtime::create(&mut db, "book", &EntitySelection::All, 1, false, profile()).is_err());
    let f = Fixture::new(1);
    let args = crate::app::requests::StartMangaStageArgs {
        project_id: f.id.clone(),
        selection: EntitySelection::All,
        stage: crate::app::contracts::MangaStage::Lettering,
        options: crate::app::requests::MangaStageOptions {
            max_pages: 1,
            force: false,
        },
    };
    assert_eq!(
        runtime::prepare(&f.manager, &args).unwrap_err(),
        AppError::invalid("mangaStageUnavailable")
    );
}

#[tokio::test]
async fn page_view_distinguishes_unrecognized_blank_and_stale_results() {
    let f = Fixture::new(1);
    let page = f.run().snapshot.selected_ids[0].clone();
    let lease = f.manager.lease(&f.id).unwrap();
    lease
        .with_connection(|db, _| {
            assert!(super::view::page(db, &page)?.recognition.is_none());
            assert!(super::view::page(db, "missing").is_err());
            Ok(())
        })
        .unwrap();
    let fake = Fake::new(0);
    *fake.text.lock().unwrap() = "{\"regions\":[]}".into();
    durable::execute(
        &f.manager,
        &f.id,
        "run",
        &RecognitionPipeline {
            provider: Arc::new(fake),
        },
        Arc::new(AtomicBool::new(false)),
        |_| {},
    )
    .await
    .unwrap();
    lease
        .with_connection(|db, _| {
            let view = super::view::page(db, &page)?;
            assert!(view.regions.is_empty());
            assert!(view.recognition.unwrap().current);
            db.execute(
                "UPDATE manga_pages SET revision=revision+1 WHERE id=?1",
                [&page],
            )
            .unwrap();
            assert!(!super::view::page(db, &page)?.recognition.unwrap().current);
            Ok(())
        })
        .unwrap();
    let mut book = crate::storage::tests::database(ProjectKind::Book);
    assert!(super::view::page(&mut book, &page).is_err());
}

struct DialogueFake {
    profile: ProviderProfile,
    calls: AtomicUsize,
    fail_on: usize,
    hook: Option<Box<dyn Fn() + Send + Sync>>,
    requests: Mutex<Vec<serde_json::Value>>,
}
impl DialogueFake {
    fn new(fail_on: usize) -> Self {
        Self {
            profile: profile(),
            calls: AtomicUsize::new(0),
            fail_on,
            hook: None,
            requests: Mutex::new(vec![]),
        }
    }
}
impl Provider for DialogueFake {
    fn profile(&self) -> &ProviderProfile {
        &self.profile
    }
    fn complete(
        &self,
        request: Request,
    ) -> Pin<Box<dyn Future<Output = Result<Completion, AppError>> + Send + '_>> {
        let Request::Structured { system, user } = request else {
            panic!("dialogue must use text JSON API")
        };
        assert!(system.contains("lockedTranslation"));
        let payload: serde_json::Value = serde_json::from_str(&user).unwrap();
        self.requests.lock().unwrap().push(payload.clone());
        let call = self.calls.fetch_add(1, Ordering::SeqCst) + 1;
        Box::pin(async move {
            if let Some(hook) = &self.hook {
                hook();
            }
            if call == self.fail_on {
                return Err(AppError::invalid("injectedFailure"));
            }
            Ok(Completion{text:serde_json::json!({"regions":payload["translateIds"].as_array().unwrap().iter().rev().map(|id|serde_json::json!({"id":id,"translatedText":"Перевод"})).collect::<Vec<_>>()} ).to_string(),finish_reason:"stop".into(),usage:Default::default(),tool_calls:vec![]})
        })
    }
}
async fn recognized_fixture(pages: u32) -> Fixture {
    let f = Fixture::new(pages);
    durable::execute(
        &f.manager,
        &f.id,
        "run",
        &RecognitionPipeline {
            provider: Arc::new(Fake::new(0)),
        },
        Arc::new(AtomicBool::new(false)),
        |_| {},
    )
    .await
    .unwrap();
    f
}
fn create_dialogue(f: &Fixture, id: &str, pages: u32) {
    f.manager
        .lease(&f.id)
        .unwrap()
        .with_connection(|db, _| {
            runtime::create_stage(
                db,
                id,
                &EntitySelection::All,
                pages,
                false,
                profile(),
                crate::app::contracts::MangaStage::Translation,
            )
        })
        .unwrap();
}
#[tokio::test]
async fn dialogue_resumes_without_retranslating_published_pages() {
    let f = recognized_fixture(2).await;
    create_dialogue(&f, "dialogue", 2);
    let fake = Arc::new(DialogueFake::new(2));
    let pipeline = super::translation_pipeline::TranslationPipeline {
        provider: fake.clone(),
    };
    assert!(durable::execute(
        &f.manager,
        &f.id,
        "dialogue",
        &pipeline,
        Arc::new(AtomicBool::new(false)),
        |_| {}
    )
    .await
    .is_err());
    let reopened = ProjectManager::new(f.root.clone());
    durable::execute(
        &reopened,
        &f.id,
        "dialogue",
        &pipeline,
        Arc::new(AtomicBool::new(false)),
        |_| {},
    )
    .await
    .unwrap();
    assert_eq!(fake.calls.load(Ordering::SeqCst), 3);
    reopened.lease(&f.id).unwrap().with_connection(|db,_|{
        assert_eq!(runs::get_run(db,"dialogue")?.state,JobState::Succeeded);
        assert_eq!(db.query_row("SELECT COUNT(*) FROM manga_regions WHERE translated_text='Перевод'",[],|r|r.get::<_,i64>(0)).unwrap(),2);
        assert_eq!(db.query_row("SELECT COUNT(*) FROM manga_results WHERE stage='translation' AND validity='current'",[],|r|r.get::<_,i64>(0)).unwrap(),2);
        assert!(runtime::create_stage(db,"again",&EntitySelection::All,2,false,profile(),crate::app::contracts::MangaStage::Translation).is_err());Ok(())
    }).unwrap();
    let requests = fake.requests.lock().unwrap();
    assert_eq!(requests[0]["targetLanguage"], "ru");
    assert_eq!(requests[0]["sourceLanguage"], "ja");
}
#[tokio::test]
async fn dialogue_keeps_manual_translations_without_api_calls() {
    let f = recognized_fixture(1).await;
    f.manager
        .lease(&f.id)
        .unwrap()
        .with_connection(|db, _| {
            let id: String = db
                .query_row("SELECT id FROM manga_regions LIMIT 1", [], |r| r.get(0))
                .unwrap();
            edits::update_region(
                db,
                &id,
                &Revision("0".into()),
                &RegionPatch::TranslatedText {
                    text: "Авторская правка".into(),
                },
            )?;
            Ok(())
        })
        .unwrap();
    create_dialogue(&f, "dialogue", 1);
    let fake = Arc::new(DialogueFake::new(0));
    durable::execute(
        &f.manager,
        &f.id,
        "dialogue",
        &super::translation_pipeline::TranslationPipeline {
            provider: fake.clone(),
        },
        Arc::new(AtomicBool::new(false)),
        |_| {},
    )
    .await
    .unwrap();
    assert_eq!(fake.calls.load(Ordering::SeqCst), 0);
    f.manager
        .lease(&f.id)
        .unwrap()
        .with_connection(|db, _| {
            let text: String = db
                .query_row(
                    "SELECT translated_text FROM manga_regions LIMIT 1",
                    [],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(text, "Авторская правка");
            Ok(())
        })
        .unwrap();
}
#[tokio::test]
async fn late_source_edit_rejects_dialogue_and_success_checkpoint() {
    let f = recognized_fixture(1).await;
    create_dialogue(&f, "dialogue", 1);
    let manager = f.manager.clone();
    let project = f.id.clone();
    let mut fake = DialogueFake::new(0);
    fake.hook = Some(Box::new(move || {
        manager
            .lease(&project)
            .unwrap()
            .with_connection(|db, _| {
                let (id, rev): (String, i64) = db
                    .query_row("SELECT id,revision FROM manga_regions LIMIT 1", [], |r| {
                        Ok((r.get(0)?, r.get(1)?))
                    })
                    .unwrap();
                edits::update_region(
                    db,
                    &id,
                    &Revision(rev.to_string()),
                    &RegionPatch::SourceText {
                        text: "corrected".into(),
                    },
                )?;
                Ok(())
            })
            .unwrap();
    }));
    let error = durable::execute(
        &f.manager,
        &f.id,
        "dialogue",
        &super::translation_pipeline::TranslationPipeline {
            provider: Arc::new(fake),
        },
        Arc::new(AtomicBool::new(false)),
        |_| {},
    )
    .await
    .unwrap_err();
    assert_eq!(error.code, ErrorCode::RevisionConflict);
    f.manager
        .lease(&f.id)
        .unwrap()
        .with_connection(|db, _| {
            assert_eq!(
                db.query_row(
                    "SELECT COUNT(*) FROM manga_results WHERE stage='translation'",
                    [],
                    |r| r.get::<_, i64>(0)
                )
                .unwrap(),
                0
            );
            assert_eq!(
                db.query_row(
                    "SELECT COUNT(*) FROM manga_regions WHERE translated_text IS NOT NULL",
                    [],
                    |r| r.get::<_, i64>(0)
                )
                .unwrap(),
                0
            );
            Ok(())
        })
        .unwrap();
}
#[tokio::test]
async fn dialogue_prompt_uses_only_matching_terms_and_locked_text_as_context() {
    let f = recognized_fixture(1).await;
    let mut input = f
        .manager
        .lease(&f.id)
        .unwrap()
        .with_connection(|db, _| regions::read(db, &f.run().snapshot.selected_ids[0]))
        .unwrap();
    input[0].source_text = "王林 пришёл".into();
    let mut locked = input[0].clone();
    locked.id = "locked".into();
    locked.reading_order = 1;
    locked.translation_manual = true;
    locked.translated_text = Some("Сохранить точно".into());
    input.push(locked);
    let fake = DialogueFake::new(0);
    let terms = vec![
        crate::application::book_terms::term("王林", "Ван Линь"),
        crate::application::book_terms::term("韩立", "Хань Ли"),
    ];
    let translated = super::translation::translate(&fake, &input, "zh", "ru", &terms)
        .await
        .unwrap();
    assert_eq!(translated.len(), 1);
    assert_eq!(translated[0].id, input[0].id);
    let requests = fake.requests.lock().unwrap();
    let p = &requests[0];
    assert_eq!(p["glossary"].as_array().unwrap().len(), 1);
    assert_eq!(p["glossary"][0]["source"], "王林");
    assert_eq!(p["regions"][1]["lockedTranslation"], "Сохранить точно");
    assert_eq!(p["translateIds"].as_array().unwrap().len(), 1);
}

struct ImageFake {
    hook: Option<Box<dyn Fn() + Send + Sync>>,
}
impl super::local::ImageWorker for ImageFake {
    fn run<'a>(
        &'a self,
        request: &'a manga_inference::protocol::Request,
    ) -> Pin<
        Box<dyn Future<Output = Result<manga_inference::protocol::Response, AppError>> + Send + 'a>,
    > {
        Box::pin(async move {
            let input = request.input.read().unwrap();
            let mut layouts = Vec::new();
            let image = match &request.operation {
                manga_inference::protocol::Operation::Lettering { regions } => {
                    assert_eq!(input.to_rgb8().get_pixel(1, 1)[0], 240);
                    for region in regions {
                        assert_eq!(region.text, "Перевод");
                        layouts.push(manga_inference::text::TextLayout {
                            id: region.id.clone(),
                            font: "DejaVu Sans".into(),
                            font_sha256: manga_inference::text::font_hash(),
                            font_size: 12.0,
                            line_height: 14.0,
                            alignment: "center".into(),
                            stroke: 1,
                        });
                    }
                    input.clone()
                }
                manga_inference::protocol::Operation::Masks { regions, rectangles, .. } => {
                    assert!(!regions.is_empty() || !rectangles.is_empty());
                    let mut mask = image::GrayImage::new(input.width(), input.height());
                    mask.put_pixel(1, 1, image::Luma([255]));
                    image::DynamicImage::ImageLuma8(mask)
                }
                manga_inference::protocol::Operation::Inpainting { mask } => {
                    let mask = mask.read().unwrap().to_luma8();
                    assert_eq!(mask.get_pixel(1, 1)[0], 255);
                    let mut cleaned = input.to_rgb8();
                    cleaned.put_pixel(1, 1, image::Rgb([240, 240, 240]));
                    image::DynamicImage::ImageRgb8(cleaned)
                }
            };
            manga_inference::protocol::save_new(&image, &request.output).unwrap();
            if let Some(hook) = &self.hook {
                hook();
            }
            Ok(manga_inference::protocol::Response {
                layouts,
                version: 1,
                width: input.width(),
                height: input.height(),
                load_millis: 0,
                inference_millis: 1,
            })
        })
    }
}
fn create_image(f: &Fixture, id: &str, stage: crate::app::contracts::MangaStage) {
    f.manager
        .lease(&f.id)
        .unwrap()
        .with_connection(|db, _| {
            super::local::create_run(
                db,
                id,
                &crate::app::requests::StartMangaStageArgs {
                    project_id: f.id.clone(),
                    selection: EntitySelection::All,
                    stage,
                    options: crate::app::requests::MangaStageOptions {
                        max_pages: 1,
                        force: false,
                    },
                },
                "test-hash",
            )
        })
        .unwrap();
}
fn image_pipeline(
    stage: crate::app::contracts::MangaStage,
    worker: ImageFake,
) -> super::image_pipeline::ImagePipeline {
    super::image_pipeline::ImagePipeline {
        worker: Arc::new(worker),
        runtime: std::env::temp_dir().join("unused-runtime"),
        model: std::env::temp_dir().join("unused-model"),
        stage,
        model_hash: "test-hash".into(),
    }
}
#[tokio::test]
async fn local_images_and_lettering_publish_immutable_assets_with_checkpoints() {
    use crate::app::contracts::MangaStage;
    let f = recognized_fixture(1).await;
    let page = f.run().snapshot.selected_ids[0].clone();
    let original=f.manager.lease(&f.id).unwrap().with_connection(|db,root|{
        let relative:String=db.query_row("SELECT a.relative_path FROM manga_pages p JOIN assets a ON a.id=p.original_asset_id WHERE p.id=?1",[&page],|r|r.get(0)).unwrap();
        Ok((root.join(relative.clone()),std::fs::read(root.join(relative)).unwrap()))
    }).unwrap();
    create_dialogue(&f, "dialogue", 1);
    durable::execute(
        &f.manager,
        &f.id,
        "dialogue",
        &super::translation_pipeline::TranslationPipeline {
            provider: Arc::new(DialogueFake::new(0)),
        },
        Arc::new(AtomicBool::new(false)),
        |_| {},
    )
    .await
    .unwrap();
    for (id, stage) in [
        ("masks", MangaStage::Masks),
        ("cleanup", MangaStage::Inpainting),
        ("lettering", MangaStage::Lettering),
    ] {
        create_image(&f, id, stage.clone());
        durable::execute(
            &f.manager,
            &f.id,
            id,
            &image_pipeline(stage, ImageFake { hook: None }),
            Arc::new(AtomicBool::new(false)),
            |_| {},
        )
        .await
        .unwrap();
    }
    f.manager.lease(&f.id).unwrap().with_connection(|db,_|{
        assert_eq!(db.query_row("SELECT COUNT(*) FROM manga_masks WHERE page_id=?1",[&page],|r|r.get::<_,i64>(0)).unwrap(),1);
        assert_eq!(db.query_row("SELECT COUNT(*) FROM manga_results WHERE page_id=?1 AND stage IN ('masks','inpainting') AND validity='current' AND output_asset_id IS NOT NULL",[&page],|r|r.get::<_,i64>(0)).unwrap(),2);
        assert_eq!(runs::get_run(db,"cleanup")?.state,JobState::Succeeded);
        assert_eq!(runs::get_run(db,"lettering")?.state,JobState::Succeeded);
        let style:String=db.query_row("SELECT style_json FROM manga_regions WHERE page_id=?1 LIMIT 1",[&page],|r|r.get(0)).unwrap();
        assert_eq!(serde_json::from_str::<serde_json::Value>(&style).unwrap()["fontSize"],12.0);
        assert!(super::local::create_run(db,"again",&crate::app::requests::StartMangaStageArgs{project_id:f.id.clone(),selection:EntitySelection::All,stage:MangaStage::Masks,options:crate::app::requests::MangaStageOptions{max_pages:1,force:false}},"test-hash").is_err());
        Ok(())
    }).unwrap();
    assert_eq!(std::fs::read(original.0).unwrap(), original.1);
}
#[tokio::test]
async fn local_output_after_geometry_change_is_not_published() {
    use crate::app::contracts::MangaStage;
    let f = recognized_fixture(1).await;
    create_image(&f, "masks", MangaStage::Masks);
    let manager = f.manager.clone();
    let project = f.id.clone();
    let worker = ImageFake {
        hook: Some(Box::new(move || {
            manager
                .lease(&project)
                .unwrap()
                .with_connection(|db, _| {
                    db.execute("UPDATE manga_pages SET revision=revision+1", [])
                        .unwrap();
                    Ok(())
                })
                .unwrap();
        })),
    };
    let error = durable::execute(
        &f.manager,
        &f.id,
        "masks",
        &image_pipeline(MangaStage::Masks, worker),
        Arc::new(AtomicBool::new(false)),
        |_| {},
    )
    .await
    .unwrap_err();
    assert_eq!(error.code, ErrorCode::RevisionConflict);
    f.manager
        .lease(&f.id)
        .unwrap()
        .with_connection(|db, _| {
            assert_eq!(
                db.query_row("SELECT COUNT(*) FROM manga_masks", [], |r| r
                    .get::<_, i64>(0))
                    .unwrap(),
                0
            );
            assert_eq!(
                db.query_row(
                    "SELECT COUNT(*) FROM manga_results WHERE stage='masks'",
                    [],
                    |r| r.get::<_, i64>(0)
                )
                .unwrap(),
                0
            );
            Ok(())
        })
        .unwrap();
}

#[tokio::test]
#[ignore = "requires MANGA_RUNTIME, MANGA_WORKER, MANGA_MASK_MODEL, MANGA_LAMA_MODEL"]
async fn native_image_stages_publish_real_masks_and_cleanup() {
    use crate::app::contracts::MangaStage;
    let f = Fixture::with_source(
        1,
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../crates/manga-inference/tests/fixtures"),
    );
    let fake = Fake::new(0);
    *fake.text.lock().unwrap()=serde_json::json!({"regions":[{"id":"source","readingOrder":0,"category":"dialogue","bounds":{"x":0,"y":0,"width":384,"height":384},"sourceText":"Synthetic text"}]}).to_string();
    durable::execute(
        &f.manager,
        &f.id,
        "run",
        &RecognitionPipeline {
            provider: Arc::new(fake),
        },
        Arc::new(AtomicBool::new(false)),
        |_| {},
    )
    .await
    .unwrap();
    let page = f.run().snapshot.selected_ids[0].clone();
    for (id, stage, model) in [
        ("masks", MangaStage::Masks, "MANGA_MASK_MODEL"),
        ("cleanup", MangaStage::Inpainting, "MANGA_LAMA_MODEL"),
    ] {
        create_image(&f, id, stage.clone());
        let pipeline = super::image_pipeline::ImagePipeline {
            worker: Arc::new(super::local::NativeWorker {
                executable: std::env::var_os("MANGA_WORKER")
                    .expect("MANGA_WORKER")
                    .into(),
            }),
            runtime: std::env::var_os("MANGA_RUNTIME")
                .expect("MANGA_RUNTIME")
                .into(),
            model: std::env::var_os(model).expect(model).into(),
            stage,
            model_hash: "test-hash".into(),
        };
        durable::execute(
            &f.manager,
            &f.id,
            id,
            &pipeline,
            Arc::new(AtomicBool::new(false)),
            |_| {},
        )
        .await
        .unwrap();
    }
    f.manager.lease(&f.id).unwrap().with_connection(|db,root|{
        let load=|stage:&str|{
            let relative:String=db.query_row("SELECT a.relative_path FROM manga_results r JOIN assets a ON a.id=r.output_asset_id WHERE r.page_id=?1 AND r.stage=?2 AND r.validity='current'",rusqlite::params![page,stage],|r|r.get(0)).unwrap();image::open(root.join(relative)).unwrap()
        };
        let mask=load("masks").to_luma8();let clean=load("inpainting").to_rgb8();
        let original=image::load_from_memory(include_bytes!("../../../../crates/manga-inference/tests/fixtures/synthetic-dialogue.png")).unwrap().to_rgb8();
        assert!(mask.as_raw().contains(&255));
        for (x,y,pixel) in original.enumerate_pixels(){if mask.get_pixel(x,y)[0]==0{assert_eq!(pixel,clean.get_pixel(x,y));}}
        assert_eq!(runs::get_run(db,"cleanup")?.state,JobState::Succeeded);
        assert_eq!(runs::get_run(db,"lettering")?.state,JobState::Succeeded);
        let style:String=db.query_row("SELECT style_json FROM manga_regions WHERE page_id=?1 LIMIT 1",[&page],|r|r.get(0)).unwrap();
        assert_eq!(serde_json::from_str::<serde_json::Value>(&style).unwrap()["fontSize"],12.0);Ok(())
    }).unwrap();
}

#[tokio::test]
async fn lettering_rejects_missing_translation_and_late_text_changes() {
    use crate::app::contracts::MangaStage;
    let f = recognized_fixture(1).await;
    for (id, stage) in [
        ("mask", MangaStage::Masks),
        ("clean", MangaStage::Inpainting),
    ] {
        create_image(&f, id, stage.clone());
        durable::execute(
            &f.manager,
            &f.id,
            id,
            &image_pipeline(stage, ImageFake { hook: None }),
            Arc::new(AtomicBool::new(false)),
            |_| {},
        )
        .await
        .unwrap();
    }
    f.manager
        .lease(&f.id)
        .unwrap()
        .with_connection(|db, _| {
            assert!(super::local::create_run(
                db,
                "too-early",
                &crate::app::requests::StartMangaStageArgs {
                    project_id: f.id.clone(),
                    selection: EntitySelection::All,
                    stage: MangaStage::Lettering,
                    options: crate::app::requests::MangaStageOptions {
                        max_pages: 1,
                        force: false
                    }
                },
                "test-hash"
            )
            .is_err());
            Ok(())
        })
        .unwrap();
    create_dialogue(&f, "dialogue", 1);
    durable::execute(
        &f.manager,
        &f.id,
        "dialogue",
        &super::translation_pipeline::TranslationPipeline {
            provider: Arc::new(DialogueFake::new(0)),
        },
        Arc::new(AtomicBool::new(false)),
        |_| {},
    )
    .await
    .unwrap();
    create_image(&f, "lettering", MangaStage::Lettering);
    let manager = f.manager.clone();
    let id = f.id.clone();
    let worker = ImageFake {
        hook: Some(Box::new(move || {
            manager
                .lease(&id)
                .unwrap()
                .with_connection(|db, _| {
                    db.execute(
                        "UPDATE manga_regions SET translated_text='Изменённый перевод'",
                        [],
                    )
                    .unwrap();
                    Ok(())
                })
                .unwrap();
        })),
    };
    let error = durable::execute(
        &f.manager,
        &f.id,
        "lettering",
        &image_pipeline(MangaStage::Lettering, worker),
        Arc::new(AtomicBool::new(false)),
        |_| {},
    )
    .await
    .unwrap_err();
    assert_eq!(error.code, ErrorCode::RevisionConflict);
    f.manager
        .lease(&f.id)
        .unwrap()
        .with_connection(|db, _| {
            assert_eq!(
                db.query_row(
                    "SELECT COUNT(*) FROM manga_results WHERE stage='lettering'",
                    [],
                    |r| r.get::<_, i64>(0)
                )
                .unwrap(),
                0
            );
            Ok(())
        })
        .unwrap();
}

struct FailCleanupOnce {
    failed: AtomicBool,
    calls: Mutex<Vec<&'static str>>,
}
impl super::local::ImageWorker for FailCleanupOnce {
    fn run<'a>(
        &'a self,
        request: &'a manga_inference::protocol::Request,
    ) -> Pin<
        Box<dyn Future<Output = Result<manga_inference::protocol::Response, AppError>> + Send + 'a>,
    > {
        Box::pin(async move {
            use manga_inference::protocol::Operation;
            let stage = match request.operation {
                Operation::Masks { .. } => "masks",
                Operation::Inpainting { .. } => "inpainting",
                Operation::Lettering { .. } => "lettering",
            };
            self.calls.lock().unwrap().push(stage);
            if stage == "inpainting" && !self.failed.swap(true, Ordering::SeqCst) {
                return Err(AppError::invalid("injectedFailure"));
            }
            ImageFake { hook: None }.run(request).await
        })
    }
}
#[tokio::test]
async fn automatic_pipeline_resumes_without_repeating_api_calls_and_limits_pages() {
    use super::{automatic, translation_pipeline::TranslationPipeline};
    use crate::app::contracts::MangaStage;
    for invalidate in [false, true] {
        let f = Fixture::new(2);
        let vision = Arc::new(Fake::new(0));
        let mut dialogue = DialogueFake::new(0);
        dialogue.profile.id = "dialogue-profile".into();
        dialogue.profile.model = "dialogue-model".into();
        let dialogue = Arc::new(dialogue);
        let plan = runs::MangaPlan {
            recognition: vision.profile.clone(),
            translation: dialogue.profile.clone(),
            mask_hash: "test-hash".into(),
            cleanup_hash: "test-hash".into(),
            lettering_hash: "test-hash".into(),
        };
        f.manager
            .lease(&f.id)
            .unwrap()
            .with_connection(|db, _| {
                let old = runs::get_run(db, "run")?;
                runs::transition(db, "run", &old.revision, JobState::Cancelled, None, "1")?;
                automatic::create(
                    db,
                    "automatic",
                    &EntitySelection::All,
                    1,
                    false,
                    plan.clone(),
                )
            })
            .unwrap();
        let worker = Arc::new(FailCleanupOnce {
            failed: AtomicBool::new(false),
            calls: Mutex::new(vec![]),
        });
        let mut masks = image_pipeline(MangaStage::Masks, ImageFake { hook: None });
        masks.worker = worker.clone();
        let mut cleanup = image_pipeline(MangaStage::Inpainting, ImageFake { hook: None });
        cleanup.worker = worker.clone();
        let mut lettering = image_pipeline(MangaStage::Lettering, ImageFake { hook: None });
        lettering.worker = worker.clone();
        let pipeline = automatic::AutomaticPipeline {
            recognition: RecognitionPipeline {
                provider: vision.clone(),
            },
            translation: TranslationPipeline {
                provider: dialogue.clone(),
            },
            masks,
            cleanup,
            lettering,
        };
        assert!(durable::execute(
            &f.manager,
            &f.id,
            "automatic",
            &pipeline,
            Arc::new(AtomicBool::new(false)),
            |_| {}
        )
        .await
        .is_err());
        assert_eq!(vision.calls.load(Ordering::SeqCst), 1);
        assert_eq!(dialogue.calls.load(Ordering::SeqCst), 1);
        if invalidate {
            f.manager
                .lease(&f.id)
                .unwrap()
                .with_connection(|db, _| {
                    db.execute(
                        "UPDATE manga_results SET validity='stale' WHERE stage='translation'",
                        [],
                    )
                    .unwrap();
                    Ok(())
                })
                .unwrap();
        }
        durable::execute(
            &f.manager,
            &f.id,
            "automatic",
            &pipeline,
            Arc::new(AtomicBool::new(false)),
            |_| {},
        )
        .await
        .unwrap();
        assert_eq!(vision.calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            dialogue.calls.load(Ordering::SeqCst),
            if invalidate { 2 } else { 1 }
        );
        assert_eq!(
            *worker.calls.lock().unwrap(),
            ["masks", "inpainting", "inpainting", "lettering"]
        );
        f.manager
        .lease(&f.id)
        .unwrap()
        .with_connection(|db, _| {
            let run = runs::get_run(db, "automatic")?;
            assert_eq!(run.state, JobState::Succeeded);
            assert_eq!(run.snapshot.selected_ids.len(), 1);
            assert_eq!(
                run.snapshot.manga.unwrap().translation.id,
                "dialogue-profile"
            );
            assert_eq!(
                db.query_row(
                    "SELECT COUNT(DISTINCT stage) FROM job_steps WHERE run_id='automatic' AND state='succeeded'",
                    [],
                    |r| r.get::<_, i64>(0)
                )
                .unwrap(),
                5
            );
            automatic::create(db, "next", &EntitySelection::All, 1, false, plan)?;
            assert_ne!(
                runs::get_run(db, "next")?.snapshot.selected_ids,
                run.snapshot.selected_ids
            );
            Ok(())
        })
        .unwrap();
    }
}

#[tokio::test]
#[ignore = "requires prepared MANGA_WORKER, MANGA_RUNTIME, MANGA_MASK_MODEL and MANGA_LAMA_MODEL"]
async fn automatic_pipeline_with_real_native_models_publishes_rendered_page() {
    use super::{automatic, local, translation_pipeline::TranslationPipeline};
    use crate::app::contracts::MangaStage;
    use sha2::{Digest, Sha256};
    let f = Fixture::with_source(
        1,
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../crates/manga-inference/tests/fixtures"),
    );
    let vision = Arc::new(Fake::new(0));
    *vision.text.lock().unwrap()=serde_json::json!({"regions":[{"id":"source","readingOrder":0,"category":"dialogue","bounds":{"x":0,"y":0,"width":384,"height":384},"sourceText":"Synthetic text"}]}).to_string();
    let dialogue = Arc::new(DialogueFake::new(0));
    let worker = Arc::new(local::NativeWorker {
        executable: std::env::var_os("MANGA_WORKER").unwrap().into(),
    });
    let runtime = std::path::PathBuf::from(std::env::var_os("MANGA_RUNTIME").unwrap());
    let make = |stage, variable: &str| {
        let model = std::path::PathBuf::from(std::env::var_os(variable).unwrap());
        let model_hash = format!("{:x}", Sha256::digest(std::fs::read(&model).unwrap()));
        super::image_pipeline::ImagePipeline {
            worker: worker.clone(),
            runtime: runtime.clone(),
            model,
            stage,
            model_hash,
        }
    };
    let masks = make(MangaStage::Masks, "MANGA_MASK_MODEL");
    let cleanup = make(MangaStage::Inpainting, "MANGA_LAMA_MODEL");
    let lettering = super::image_pipeline::ImagePipeline {
        worker: worker.clone(),
        runtime: runtime.clone(),
        model: runtime,
        stage: MangaStage::Lettering,
        model_hash: format!(
            "{}:{}",
            manga_inference::text::VERSION,
            manga_inference::text::font_hash()
        ),
    };
    let plan = runs::MangaPlan {
        recognition: vision.profile.clone(),
        translation: dialogue.profile.clone(),
        mask_hash: masks.model_hash.clone(),
        cleanup_hash: cleanup.model_hash.clone(),
        lettering_hash: lettering.model_hash.clone(),
    };
    f.manager
        .lease(&f.id)
        .unwrap()
        .with_connection(|db, _| {
            let old = runs::get_run(db, "run")?;
            runs::transition(db, "run", &old.revision, JobState::Cancelled, None, "1")?;
            automatic::create(db, "automatic", &EntitySelection::All, 1, false, plan)
        })
        .unwrap();
    let pipeline = automatic::AutomaticPipeline {
        recognition: RecognitionPipeline { provider: vision },
        translation: TranslationPipeline { provider: dialogue },
        masks,
        cleanup,
        lettering,
    };
    durable::execute(
        &f.manager,
        &f.id,
        "automatic",
        &pipeline,
        Arc::new(AtomicBool::new(false)),
        |_| {},
    )
    .await
    .unwrap();
    f.manager
        .lease(&f.id)
        .unwrap()
        .with_connection(|db, root| {
            let run = runs::get_run(db, "automatic")?;
            assert_eq!(run.state, JobState::Succeeded);
            let view = super::view::page(db, &run.snapshot.selected_ids[0])?;
            let rendered = view.rendered_asset_id.unwrap();
            assert_ne!(rendered, view.page.original_asset_id);
            assert!(root.join(format!("assets/{}.png", rendered.0)).is_file());
            assert_eq!(
                db.query_row(
                    "SELECT COUNT(DISTINCT stage) FROM job_steps WHERE run_id='automatic' AND state='succeeded'",
                    [],
                    |r| r.get::<_, i64>(0)
                )
                .unwrap(),
                5
            );
            Ok(())
        })
        .unwrap();
}

#[tokio::test]
async fn edited_regions_rebuild_without_api_and_keep_geometry_and_direction() {
    use crate::app::{contracts::MangaStage,requests::{StartMangaStageArgs,MangaStageOptions}};
    let f=recognized_fixture(1).await;
    let page=f.run().snapshot.selected_ids[0].clone();
    create_dialogue(&f,"dialogue",1);
    durable::execute(&f.manager,&f.id,"dialogue",&super::translation_pipeline::TranslationPipeline{provider:Arc::new(DialogueFake::new(0))},Arc::new(AtomicBool::new(false)),|_|{}).await.unwrap();
    f.manager.lease(&f.id).unwrap().with_connection(|db,_|{
        let r=regions::read(db,&page)?.remove(0);
        let mut bounds=r.bounds.clone(); bounds.x+=1.0;
        let rev=edits::update_region(db,&r.id,&Revision(r.revision.to_string()),&RegionPatch::Bounds{bounds:bounds.clone()})?;
        edits::update_region(db,&r.id,&rev,&RegionPatch::Direction{vertical:true})?;
        assert_eq!(regions::read(db,&page)?[0].bounds,bounds);
        super::local::create_run_mode(db,"rebuild",&StartMangaStageArgs{project_id:f.id.clone(),stage:MangaStage::Masks,selection:EntitySelection::ExplicitIds{ids:vec![page.clone()]},options:MangaStageOptions{max_pages:1,force:true}},"test-hash",true)
    }).unwrap();
    let pipeline=super::rebuild::RebuildPipeline{
        masks:image_pipeline(MangaStage::Masks,ImageFake{hook:None}),
        cleanup:image_pipeline(MangaStage::Inpainting,ImageFake{hook:None}),
        lettering:image_pipeline(MangaStage::Lettering,ImageFake{hook:None}),
    };
    durable::execute(&f.manager,&f.id,"rebuild",&pipeline,Arc::new(AtomicBool::new(false)),|_|{}).await.unwrap();
    f.manager.lease(&f.id).unwrap().with_connection(|db,_|{
        let r=regions::read(db,&page)?.remove(0);
        assert!(r.vertical && r.manual_bounds);
        assert_eq!(r.translated_text.as_deref(),Some("Перевод"));
        assert!(super::view::page(db,&page)?.rendered_asset_id.is_some());
        assert_eq!(runs::get_run(db,"rebuild")?.state,JobState::Succeeded);
        Ok(())
    }).unwrap();
}
