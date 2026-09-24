use super::book::*;
use crate::ai::{Completion, ProviderProfile, Usage};
use crate::{
    ai::{Provider, Request},
    app::contracts::AppError,
};
use std::sync::Mutex;
use std::{future::Future, pin::Pin};

struct Fake {
    profile: ProviderProfile,
    replies: Mutex<std::collections::VecDeque<String>>,
    requests: Mutex<Vec<serde_json::Value>>,
}
impl Fake {
    fn new(replies: Vec<&str>) -> Self {
        Self {
            profile: ProviderProfile {
                id: "fake".into(),
                base_url: "https://unused.test".into(),
                model: "fake".into(),
                temperature: 0.,
                max_output_tokens: 1000,
                timeout_seconds: 1,
                network_retries: 0,
            },
            replies: Mutex::new(replies.into_iter().map(String::from).collect()),
            requests: Mutex::new(vec![]),
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
        if let Request::Structured { user, .. } = request {
            self.requests
                .lock()
                .unwrap()
                .push(serde_json::from_str(&user).unwrap());
        }
        Box::pin(async {
            Ok(Completion {
                text: self
                    .replies
                    .lock()
                    .unwrap()
                    .pop_front()
                    .expect("bounded number of calls"),
                finish_reason: "stop".into(),
                usage: Usage::default(),
                tool_calls: vec![],
            })
        })
    }
}
#[test]
fn unicode_segments_reassemble_without_changing_whitespace() {
    let text = "日 本語\n🙂 abcdef";
    let pieces = split_segments("block", text, 3).unwrap();
    assert_eq!(
        pieces.iter().map(|s| s.text.as_str()).collect::<String>(),
        text
    );
    assert_eq!(pieces[1].id, "block:1");
    assert!(pieces.iter().all(|s| s.text.chars().count() <= 3));
}
#[tokio::test]
async fn repairs_only_missing_or_duplicated_ids() {
    let fake = Fake::new(vec![
        r#"{"segments":[{"id":"a","text":"A"},{"id":"b","text":"wrong"},{"id":"b","text":"duplicate"}]}"#,
        r#"{"segments":[{"id":"b","text":"B"},{"id":"c","text":"C"}]}"#,
    ]);
    let segments = ["a", "b", "c"].map(|id| Segment {
        id: id.into(),
        text: format!("source {id}"),
    });
    let output = translate_segments(&fake, "JSON", &segments).await.unwrap();
    assert_eq!(output.len(), 3);
    let requests = fake.requests.lock().unwrap();
    assert_eq!(requests[1]["segments"].as_array().unwrap().len(), 2);
    assert!(requests[1]["segments"]
        .as_array()
        .unwrap()
        .iter()
        .all(|s| s["id"] != "a"));
}
#[tokio::test]
async fn unknown_ids_are_rejected_and_malformed_repairs_are_bounded() {
    let segment = Segment {
        id: "a".into(),
        text: "source".into(),
    };
    let fake = Fake::new(vec![r#"{"segments":[{"id":"invented","text":"bad"}]}"#]);
    assert!(
        translate_segments(&fake, "JSON", std::slice::from_ref(&segment))
            .await
            .is_err()
    );
    let fake = Fake::new(vec!["bad", "bad", "bad"]);
    assert!(translate_segments(&fake, "JSON", &[segment]).await.is_err());
    assert_eq!(fake.requests.lock().unwrap().len(), 3);
}

struct Echo {
    profile: ProviderProfile,
    glossary_calls: std::sync::atomic::AtomicUsize,
    fail_context_once: std::sync::atomic::AtomicBool,
}
impl Provider for Echo {
    fn profile(&self) -> &ProviderProfile {
        &self.profile
    }
    fn complete(
        &self,
        request: Request,
    ) -> Pin<Box<dyn Future<Output = Result<Completion, AppError>> + Send + '_>> {
        Box::pin(async move {
            let Request::Structured { user, system } = request else {
                panic!("structured request expected")
            };
            let text = if system.starts_with("Extract at most") {
                let payload: serde_json::Value = serde_json::from_str(&user).unwrap();
                assert_eq!(payload["bookInstructions"], "Keep the established names.");
                assert!(payload["referenceExcerpt"].as_str().unwrap().starts_with("Mapped reference"));
                assert!(payload["referenceExcerpt"].as_str().unwrap().chars().count() <= 16000);
                if payload["source"].as_str().unwrap().contains("Original") {
                    assert_eq!(payload["existingTerms"][0]["target"], "Canonical");
                    assert_eq!(payload["existingTerms"][0]["pinned"], true);
                }
                self.glossary_calls
                    .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                let source = if user.contains("Original") {
                    "Original"
                } else {
                    "Before"
                };
                serde_json::json!({"terms":[{"source":source,"target":"Термин","kind":"term"}]})
                    .to_string()
            } else if let Ok(mut payload) = serde_json::from_str::<serde_json::Value>(&user) {
                for segment in payload["segments"].as_array_mut().unwrap() {
                    segment["text"] =
                        format!("Translated {}", segment["text"].as_str().unwrap()).into();
                }
                payload.to_string()
            } else {
                if self
                    .fail_context_once
                    .swap(false, std::sync::atomic::Ordering::SeqCst)
                {
                    return Err(AppError::invalid("testContextFailure"));
                }
                r#"{"summary":"Continuity notes"}"#.into()
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
#[tokio::test]
async fn pipeline_populates_batch_glossary_and_resumes_without_repeating_completed_steps() {
    use crate::{
        app::{
            contracts::ProjectKind,
            requests::{LanguagePair, ProjectChoices},
        },
        jobs::durable,
        project::lifecycle::ProjectManager,
        storage::{runs, shared},
    };
    use std::sync::{atomic::AtomicBool, Arc};
    let root = std::env::temp_dir().join(format!("book-pipeline-{}", uuid::Uuid::new_v4()));
    let manager = ProjectManager::new(root.clone());
    let preview = manager
        .inspect_source(
            ProjectKind::Book,
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../tests/fixtures/structural.epub"),
        )
        .unwrap();
    let project = manager
        .create(
            &preview.import_id.0,
            &ProjectChoices {
                name: "Test".into(),
                languages: LanguagePair {
                    source: Some("en".into()),
                    target: "ru".into(),
                },
                processing_profile_id: None,
            },
        )
        .unwrap();
    let profile = Fake::new(vec![]).profile;
    manager.lease(&project.id).unwrap().with_connection(|db,_|{
        let selected_ids={let mut query=db.prepare("SELECT id FROM book_chapters WHERE EXISTS(SELECT 1 FROM book_source_blocks WHERE chapter_id=book_chapters.id AND kind IN ('text','caption')) ORDER BY position").unwrap();let rows=query.query_map([],|r|r.get::<_,String>(0)).unwrap();rows.collect::<Result<Vec<_>,_>>().unwrap()};
        db.execute("INSERT INTO book_presentation(singleton,instructions) VALUES(1,'Keep the established names.')",[]).unwrap();
        db.execute("INSERT INTO glossary_terms(id,source,target,kind,pinned) VALUES('pinned','Original','Canonical','term',1)",[]).unwrap();
        db.execute("INSERT INTO book_reference_chapters(id,position,title,text) VALUES('mapped',0,'Reference',?1)",[format!("Mapped reference{}", "文".repeat(20000))]).unwrap();
        for chapter in &selected_ids {
            db.execute("INSERT INTO book_reference_mappings(chapter_id,reference_id) VALUES(?1,'mapped')",[chapter]).unwrap();
        }
        let settings=shared::settings(db)?;
        runs::create_run(db,"run","book_translation",&runs::RunSnapshot{settings:settings.choices,settings_revision:settings.revision,glossary_revision:shared::glossary_revision(db)?,selected_ids,prompt_version:"book-v1".into(),stages:vec!["glossary".into(),"translation".into(),"context".into()],provider:Some(profile.clone()),instructions:None},"now")
    }).unwrap();
    let provider = Arc::new(Echo {
        profile,
        glossary_calls: std::sync::atomic::AtomicUsize::new(0),
        fail_context_once: AtomicBool::new(true),
    });
    let pipeline = BookPipeline {
        provider: provider.clone(),
        instructions: None,
    };
    assert!(durable::execute(
        &manager,
        &project.id,
        "run",
        &pipeline,
        Arc::new(AtomicBool::new(false)),
        |_| {}
    )
    .await
    .is_err());
    assert_eq!(
        provider
            .glossary_calls
            .load(std::sync::atomic::Ordering::SeqCst),
        2
    );
    durable::execute(
        &manager,
        &project.id,
        "run",
        &pipeline,
        Arc::new(AtomicBool::new(false)),
        |_| {},
    )
    .await
    .unwrap();
    assert_eq!(
        provider
            .glossary_calls
            .load(std::sync::atomic::Ordering::SeqCst),
        2
    );
    manager.lease(&project.id).unwrap().with_connection(|db,_|{
        assert_eq!(shared::glossary(db)?.len(),2);
        assert_eq!(db.query_row("SELECT target FROM glossary_terms WHERE id='pinned'",[],|r|r.get::<_,String>(0)).unwrap(),"Canonical");
        assert_eq!(runs::get_run(db,"run")?.snapshot.glossary_revision,shared::glossary_revision(db)?);
        assert_eq!(db.query_row("SELECT COUNT(*) FROM book_translations",[],|r|r.get::<_,i64>(0)).unwrap(),2);
        assert_eq!(db.query_row("SELECT COUNT(*) FROM book_contexts",[],|r|r.get::<_,i64>(0)).unwrap(),2);
        assert_eq!(db.query_row("SELECT COUNT(*) FROM book_source_blocks WHERE kind='image'",[],|r|r.get::<_,i64>(0)).unwrap(),3);
        assert_eq!(db.query_row("SELECT COUNT(*) FROM book_translation_blocks WHERE translated_text LIKE '%[[img:%'",[],|r|r.get::<_,i64>(0)).unwrap(),0);Ok(())
    }).unwrap();
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn metadata_job_persists_independently_and_becomes_stale_after_source_edit() {
    use crate::{
        app::{
            contracts::ProjectKind,
            requests::{LanguagePair, ProjectChoices},
        },
        jobs::durable,
        project::lifecycle::ProjectManager,
        storage::{runs, shared},
    };
    use std::sync::{atomic::AtomicBool, Arc};
    let root = std::env::temp_dir().join(format!("metadata-job-{}", uuid::Uuid::new_v4()));
    let manager = ProjectManager::new(root.clone());
    let preview = manager
        .inspect_source(
            ProjectKind::Book,
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../tests/fixtures/structural.epub"),
        )
        .unwrap();
    let project = manager
        .create(
            &preview.import_id.0,
            &ProjectChoices {
                name: "Source title".into(),
                languages: LanguagePair {
                    source: Some("en".into()),
                    target: "ru".into(),
                },
                processing_profile_id: None,
            },
        )
        .unwrap();
    struct MetadataProvider(ProviderProfile);
    impl Provider for MetadataProvider {
        fn profile(&self) -> &ProviderProfile {
            &self.0
        }
        fn complete(
            &self,
            request: Request,
        ) -> Pin<Box<dyn Future<Output = Result<Completion, AppError>> + Send + '_>> {
            assert!(matches!(request, Request::Structured { .. }));
            Box::pin(async {
                Ok(Completion {
                    text: r#"{"title":"Test title","author":"","summary":"Test summary"}"#.into(),
                    finish_reason: "stop".into(),
                    usage: Usage::default(),
                    tool_calls: vec![],
                })
            })
        }
    }
    let profile = Fake::new(vec![]).profile;
    manager
        .lease(&project.id)
        .unwrap()
        .with_connection(|db, _| {
            assert!(super::book_metadata::read(db)?.is_none());
            let chapter: String = db
                .query_row(
                    "SELECT id FROM book_chapters ORDER BY position LIMIT 1",
                    [],
                    |r| r.get(0),
                )
                .unwrap();
            let settings = shared::settings(db)?;
            runs::create_run(
                db,
                "metadata",
                "book_metadata",
                &runs::RunSnapshot {
                    settings: settings.choices,
                    settings_revision: settings.revision,
                    glossary_revision: shared::glossary_revision(db)?,
                    selected_ids: vec![chapter],
                    prompt_version: "book-metadata-v1".into(),
                    stages: vec!["metadata".into()],
                    provider: Some(profile.clone()),
                    instructions: None,
                },
                "now",
            )
        })
        .unwrap();
    let pipeline = BookPipeline {
        provider: Arc::new(MetadataProvider(profile)),
        instructions: None,
    };
    durable::execute(
        &manager,
        &project.id,
        "metadata",
        &pipeline,
        Arc::new(AtomicBool::new(false)),
        |_| {},
    )
    .await
    .unwrap();
    manager
        .lease(&project.id)
        .unwrap()
        .with_connection(|db, _| {
            let value = super::book_metadata::read(db)?.unwrap();
            assert!(value.current);
            assert_eq!(value.title, "Test title");
            assert_eq!(
                db.query_row("SELECT COUNT(*) FROM book_translations", [], |r| r
                    .get::<_, i64>(0))
                    .unwrap(),
                0
            );
            assert_eq!(
                runs::get_run(db, "metadata")?.state,
                crate::app::contracts::JobState::Succeeded
            );
            db.execute(
                "UPDATE book_chapters SET revision=revision+1 WHERE position=0",
                [],
            )
            .unwrap();
            assert!(!super::book_metadata::read(db)?.unwrap().current);
            Ok(())
        })
        .unwrap();
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn glossary_run_resumes_without_repeating_published_chapters_or_overwriting_pins() {
    use crate::{
        app::{
            contracts::{ProjectKind, Revision},
            requests::{LanguagePair, ProjectChoices},
        },
        jobs::durable,
        project::lifecycle::ProjectManager,
        storage::{runs, shared},
    };
    use std::sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc,
    };
    let root = std::env::temp_dir().join(format!("glossary-job-{}", uuid::Uuid::new_v4()));
    let manager = ProjectManager::new(root.clone());
    let preview = manager
        .inspect_source(
            ProjectKind::Book,
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../tests/fixtures/structural.epub"),
        )
        .unwrap();
    let project = manager
        .create(
            &preview.import_id.0,
            &ProjectChoices {
                name: "Glossary test".into(),
                languages: LanguagePair {
                    source: Some("en".into()),
                    target: "ru".into(),
                },
                processing_profile_id: None,
            },
        )
        .unwrap();
    struct Extractor {
        profile: ProviderProfile,
        calls: AtomicUsize,
    }
    impl Provider for Extractor {
        fn profile(&self) -> &ProviderProfile {
            &self.profile
        }
        fn complete(
            &self,
            request: Request,
        ) -> Pin<Box<dyn Future<Output = Result<Completion, AppError>> + Send + '_>> {
            let call = self.calls.fetch_add(1, Ordering::SeqCst);
            Box::pin(async move {
                let Request::Structured { user, .. } = request else {
                    panic!("structured request expected")
                };
                if call == 1 {
                    return Err(AppError::invalid("testFailure"));
                }
                let term = if user.contains("Original") {
                    assert_eq!(call, 0, "published chapter must not run twice");
                    "Original"
                } else {
                    "Before"
                };
                Ok(Completion{text:serde_json::json!({"terms":[{"source":term,"target":"Generated","kind":"term"}]}).to_string(),finish_reason:"stop".into(),usage:Usage::default(),tool_calls:vec![]})
            })
        }
    }
    let profile = Fake::new(vec![]).profile;
    manager
        .lease(&project.id)
        .unwrap()
        .with_connection(|db, _| {
            shared::put_term(
                db,
                &shared::GlossaryTerm {
                    id: "pinned".into(),
                    source: "Original".into(),
                    target: "Ручной перевод".into(),
                    kind: "name".into(),
                    pinned: true,
                    frequency: 0,
                    revision: Revision("0".into()),
                },
                None,
            )?;
            let selected = {
                let mut q = db
                    .prepare(
                        "SELECT id FROM book_chapters WHERE position IN (0,2) ORDER BY position",
                    )
                    .unwrap();
                let rows = q.query_map([], |r| r.get::<_, String>(0)).unwrap();
                rows.collect::<Result<Vec<_>, _>>().unwrap()
            };
            let settings = shared::settings(db)?;
            runs::create_run(
                db,
                "glossary",
                "book_glossary",
                &runs::RunSnapshot {
                    settings: settings.choices,
                    settings_revision: settings.revision,
                    glossary_revision: shared::glossary_revision(db)?,
                    selected_ids: selected,
                    prompt_version: "book-glossary-v1".into(),
                    stages: vec!["glossary".into()],
                    provider: Some(profile.clone()),
                    instructions: None,
                },
                "now",
            )
        })
        .unwrap();
    let provider = Arc::new(Extractor {
        profile,
        calls: AtomicUsize::new(0),
    });
    let pipeline = BookPipeline {
        provider: provider.clone(),
        instructions: None,
    };
    assert!(durable::execute(
        &manager,
        &project.id,
        "glossary",
        &pipeline,
        Arc::new(AtomicBool::new(false)),
        |_| {}
    )
    .await
    .is_err());
    durable::execute(
        &manager,
        &project.id,
        "glossary",
        &pipeline,
        Arc::new(AtomicBool::new(false)),
        |_| {},
    )
    .await
    .unwrap();
    assert_eq!(provider.calls.load(Ordering::SeqCst), 3);
    manager
        .lease(&project.id)
        .unwrap()
        .with_connection(|db, _| {
            let terms = shared::glossary(db)?;
            let pinned = terms.iter().find(|t| t.id == "pinned").unwrap();
            assert_eq!(pinned.target, "Ручной перевод");
            assert!(pinned.pinned);
            assert_eq!(pinned.frequency, 1);
            assert_eq!(terms.len(), 2);
            assert_eq!(
                db.query_row("SELECT COUNT(*) FROM book_glossary_results", [], |r| r
                    .get::<_, i64>(0))
                    .unwrap(),
                2
            );
            assert_eq!(
                db.query_row("SELECT COUNT(*) FROM book_translations", [], |r| r
                    .get::<_, i64>(0))
                    .unwrap(),
                0
            );
            assert_eq!(
                runs::get_run(db, "glossary")?.state,
                crate::app::contracts::JobState::Succeeded
            );
            Ok(())
        })
        .unwrap();
    std::fs::remove_dir_all(root).unwrap();
}
