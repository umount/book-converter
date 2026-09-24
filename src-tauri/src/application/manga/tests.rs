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
        let root = std::env::temp_dir().join(format!("manga-run-{}", uuid::Uuid::new_v4()));
        let manager = Arc::new(ProjectManager::new(root.clone()));
        let preview = manager
            .inspect_source(
                ProjectKind::Manga,
                &std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../tests/fixtures/two-volumes.cbz"),
            )
            .unwrap();
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
