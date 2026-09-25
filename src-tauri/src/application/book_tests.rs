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
    replies: Mutex<std::collections::VecDeque<Result<String, AppError>>>,
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
            replies: Mutex::new(replies.into_iter().map(|s|Ok(s.to_owned())).collect()),
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
        if let Request::Structured { user, system } = request {
            let mut payload:serde_json::Value=serde_json::from_str(&user).unwrap();
            payload["system"]=system.into();
            self.requests
                .lock()
                .unwrap()
                .push(payload);
        }
        Box::pin(async {
            Ok(Completion {
                text: self
                    .replies
                    .lock()
                    .unwrap()
                    .pop_front()
                    .expect("bounded number of calls")?,
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
    let fake = Fake::new(vec![r#"{"segments":[{"id":"invented","text":"bad"}]}"#; 3]);
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
            let text = if system.starts_with("Extract recurring names") {
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
            } else if system.starts_with("Repair only") {
                // Leave the fake English output unchanged; language repair has its own tests.
                r#"{"lines":[]}"#.into()
            } else if system.starts_with("Translate every") {
                let mut payload: serde_json::Value = serde_json::from_str(&user).unwrap();
                if payload["segments"].as_array().unwrap().iter().any(|s|s["text"].as_str().is_some_and(|t|t.contains("Before"))) {
                    assert!(system.contains("Continuity notes"));
                    assert!(system.contains("Translated Original text."));
                }
                for segment in payload["segments"].as_array_mut().unwrap() {
                    segment["text"] =
                        format!("Translated {}", segment["text"].as_str().unwrap()).into();
                }
                payload.to_string()
            } else {
                let payload: serde_json::Value = serde_json::from_str(&user).unwrap();
                let chapter = payload["chapter"].as_str().unwrap();
                if chapter.contains("Before") {
                    assert_eq!(payload["previousSummary"], "Continuity notes");
                } else {
                    assert_eq!(payload["previousSummary"], "");
                }
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
        runs::create_run(db,"run","book_translation",&runs::RunSnapshot{manga: None,
            retarget:None,settings:settings.choices,settings_revision:settings.revision,glossary_revision:shared::glossary_revision(db)?,selected_ids,prompt_version:"book-v1".into(),stages:vec!["glossary".into(),"translation".into(),"context".into()],provider:Some(profile.clone()),instructions:None},"now")
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
    manager.lease(&project.id).unwrap().with_connection(|db,_| {
        let chapter: String=db.query_row("SELECT id FROM book_chapters ORDER BY position LIMIT 1",[],|r|r.get(0)).unwrap();
        let view=crate::storage::repository::ProjectRepository::new(db,ProjectKind::Book)?.chapter(&chapter)?;
        assert_eq!(view.status,"failed"); assert!(view.translation_error.is_some());
        Ok(())
    }).unwrap();
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
            let Request::Structured { user, system } = request else { panic!("metadata must be structured") };
            let supplied: serde_json::Value = serde_json::from_str(&user).unwrap();
            if system.starts_with("Repair only") {
                assert!(supplied["lines"].as_array().unwrap().iter().any(|line| line["text"].as_str().unwrap().contains("中文")));
                return Box::pin(async { Ok(Completion {
                    text: r#"{"lines":[{"n":0,"text":"Тестовое название"},{"n":2,"text":"Тестовое описание"}]}"#.into(),
                    finish_reason: "stop".into(), usage: Usage::default(), tool_calls: vec![],
                }) });
            }
            assert!(supplied.get("title").is_some());
            assert!(supplied.get("author").is_some());
            assert!(supplied.get("annotation").is_some());
            assert!(supplied["excerpt"].as_str().is_some_and(|text| !text.is_empty()));
            assert!(system.contains("Translate the supplied source title and author"));
            Box::pin(async {
                Ok(Completion {
                    text: r#"{"title":"中文标题","author":"","summary":"中文简介"}"#.into(),
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
            let source = super::book_presentation::read(db)?;
            assert!(source.source_title.as_ref().is_some_and(|title| !title.is_empty()));
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
                &runs::RunSnapshot { manga: None,
            retarget: None,
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
    // An unrepairable source-language response must not publish metadata.
    let invalid_provider = Fake::new(vec![
        r#"{"title":"中文标题","author":"","summary":"中文简介"}"#,
        "invalid repair",
    ]);
    let lease = manager.lease(&project.id).unwrap();
    let run = lease.with_connection(|db, _| runs::get_run(db, "metadata")).unwrap();
    let error = match super::book_metadata::compute(&lease, &run, &invalid_provider).await {
        Ok(_) => panic!("foreign-language metadata must not be accepted"),
        Err(error) => error,
    };
    assert_eq!(error.params["field"], "metadataLanguage");
    lease.with_connection(|db, _| { assert!(super::book_metadata::read(db)?.is_none()); Ok(()) }).unwrap();
    drop(lease);
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
            assert_eq!(value.title, "Тестовое название");
            assert_eq!(value.summary, "Тестовое описание");
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
                &runs::RunSnapshot { manga: None,
            retarget: None,
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


#[tokio::test]
async fn wholly_invalid_translation_retries_the_complete_input_with_context() {
    let segments=[Segment{id:"title".into(),text:"Source title".into()},Segment{id:"body".into(),text:"Source body".into()}];
    for invalid in ["", "not JSON", r#"{"segments":[]}"#, r#"{"segments":[{"id":"other","text":"Wrong"}]}"#] {
        let fake=Fake::new(vec![invalid,r#"{"segments":[{"id":"title","text":"Заголовок"},{"id":"body","text":"Перевод"}]}"#]);
        let output=translate_segments(&fake,"Glossary and rolling context",&segments).await.unwrap();
        assert_eq!(output.len(),2);
        let requests=fake.requests.lock().unwrap();assert_eq!(requests.len(),2);assert_eq!(requests[0],requests[1]);
        assert_eq!(requests[1]["segments"].as_array().unwrap().len(),2);
    }
}

#[tokio::test]
async fn malformed_provider_envelope_retries_but_permanent_errors_do_not() {
    let segment=Segment{id:"body".into(),text:"Source".into()};
    let fake=Fake::new(vec![r#"{"segments":[{"id":"body","text":"Перевод"}]}"#]);
    fake.replies.lock().unwrap().push_front(Err(AppError{code:crate::app::contracts::ErrorCode::InvalidOutput,message_key:"errors.providerResponse".into(),params:Default::default(),retryable:false}));
    assert!(translate_segments(&fake,"Context",std::slice::from_ref(&segment)).await.is_ok());
    assert_eq!(fake.requests.lock().unwrap().len(),2);
    let fake=Fake::new(vec![]);
    fake.replies.lock().unwrap().push_front(Err(AppError::invalid("credentials")));
    assert!(translate_segments(&fake,"Context",&[segment]).await.is_err());
    assert_eq!(fake.requests.lock().unwrap().len(),1);
}

#[tokio::test]
async fn title_job_sends_only_title_and_glossary_and_rejects_late_edits() {
    use crate::{app::{contracts::ProjectKind,requests::{ProjectChoices,LanguagePair}},project::lifecycle::ProjectManager,storage::{repository::ProjectRepository,results,runs,shared},jobs::{durable,durable::StepExecutor}};
    use std::sync::{Arc,atomic::AtomicBool};
    let root=std::env::temp_dir().join(format!("title-job-{}",uuid::Uuid::new_v4()));
    let manager=ProjectManager::new(root.clone());
    let preview=manager.inspect_source(ProjectKind::Book,&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests/fixtures/structural.epub")).unwrap();
    let project=manager.create(&preview.import_id.0,&ProjectChoices{name:"Title".into(),languages:LanguagePair{source:Some("en".into()),target:"ru".into()},processing_profile_id:None}).unwrap();
    let lease=manager.lease(&project.id).unwrap();
    let chapter=lease.with_connection(|db,_|{
        let id:String=db.query_row("SELECT id FROM book_chapters ORDER BY position LIMIT 1",[],|r|r.get(0)).unwrap();
        let view=ProjectRepository::new(db,ProjectKind::Book)?.chapter(&id)?;
        results::save_translation(db,&results::BookTranslation{id:"original".into(),chapter_id:id.clone(),inputs:results::InputVersions{source:view.chapter.revision,settings:shared::settings(db)?.revision,glossary:shared::glossary_revision(db)?},expected_translation:None,title:"Old title".into(),provenance:"reference".into(),context_fingerprint:"reference".into(),blocks:view.blocks.iter().filter(|b|!matches!(b.content,crate::app::contracts::BookBlockContent::Image{..})).map(|b|(b.id.clone(),"FULL REFERENCE BODY".into())).collect()})?;
        db.execute("INSERT INTO glossary_terms(id,source,target,kind,pinned) VALUES('term','Chapter','Глава','term',1)",[]).unwrap();
        db.execute("INSERT INTO glossary_terms(id,source,target,kind,pinned) VALUES('body-term','Original','BODY ONLY TERM','term',1)",[]).unwrap();
        Ok(id)
    }).unwrap();
    let reply=serde_json::json!({"segments":[{"id":format!("{chapter}:title:0"),"text":"Глава первая"}]}).to_string();
    let provider=Arc::new(Fake::new(vec![&reply,&reply]));
    lease.with_connection(|db,_|{
        let settings=shared::settings(db)?;
        runs::create_run(db,"title-run","book_title",&runs::RunSnapshot{manga: None,
            retarget:None,settings:settings.choices,settings_revision:settings.revision,glossary_revision:shared::glossary_revision(db)?,selected_ids:vec![chapter.clone()],prompt_version:"book-title-v1".into(),stages:vec!["title".into()],provider:Some(provider.profile.clone()),instructions:Some("Keep chapter numbers".into())},"now")
    }).unwrap();
    let pipeline=BookPipeline{provider:provider.clone(),instructions:Some("Keep chapter numbers".into())};
    durable::execute(&manager,&project.id,"title-run",&pipeline,Arc::new(AtomicBool::new(false)),|_|{}).await.unwrap();
    lease.with_connection(|db,_|{
        let view=ProjectRepository::new(db,ProjectKind::Book)?.chapter(&chapter)?;
        assert_eq!(view.translation.as_ref().unwrap().title,"Глава первая");assert_eq!(view.translation.as_ref().unwrap().origin,"reference");assert_eq!(view.blocks[0].translated_text.as_deref(),Some("FULL REFERENCE BODY"));
        // Simulate recovery after the completed step was committed.
        db.execute("UPDATE job_runs SET state='interrupted' WHERE id='title-run'",[]).unwrap();Ok(())
    }).unwrap();
    durable::execute(&manager,&project.id,"title-run",&pipeline,Arc::new(AtomicBool::new(false)),|_|{}).await.unwrap();
    {
        let requests=provider.requests.lock().unwrap();assert_eq!(requests.len(),1);
        assert_eq!(requests[0]["segments"].as_array().unwrap().len(),1);
        let system=requests[0]["system"].as_str().unwrap();assert!(system.contains("Глава"));assert!(system.contains("Keep chapter numbers"));assert!(!system.contains("BODY ONLY TERM"));assert!(!requests[0].to_string().contains("FULL REFERENCE BODY"));assert!(!requests[0].to_string().contains("Original text."));
    }
    let run=lease.with_connection(|db,_|runs::get_run(db,"title-run")).unwrap();
    let late=pipeline.compute(&lease,&run,&chapter,"title").await.unwrap();
    lease.with_connection(|db,_|{
        let t=ProjectRepository::new(db,ProjectKind::Book)?.chapter(&chapter)?.translation.unwrap();
        results::edit_translation_title(db,&t.id,&t.revision,"Ручной заголовок")?;
        let tx=db.transaction().unwrap();assert!(pipeline.persist(&tx,late).is_err());Ok(())
    }).unwrap();
    drop(lease);drop(manager);std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn glossary_is_filtered_again_for_only_the_unresolved_fragments() {
    let terms=vec![super::book_terms::term("Alpha","Первый"),super::book_terms::term("Beta","Второй"),super::book_terms::term("NeverOccurs","НЕ ОТПРАВЛЯТЬ")];
    let fake=Fake::new(vec![r#"{"segments":[{"id":"a","text":"Первый"}]}"#,r#"{"segments":[{"id":"b","text":"Второй"}]}"#]);
    let segments=[Segment{id:"a".into(),text:"Alpha appears".into()},Segment{id:"b".into(),text:"Beta appears".into()}];
    super::book::translate_segments_with_glossary(&fake,"Translate",&segments,&terms).await.unwrap();
    let requests=fake.requests.lock().unwrap();let first=requests[0]["system"].as_str().unwrap();let retry=requests[1]["system"].as_str().unwrap();
    assert!(first.contains("Первый"));assert!(first.contains("Второй"));assert!(!first.contains("НЕ ОТПРАВЛЯТЬ"));
    assert!(retry.contains("Второй"));assert!(!retry.contains("Первый"));assert!(!retry.contains("NeverOccurs"));
}
