use super::durable::*;
use crate::app::contracts::{AppError, JobState, ProjectId};
use crate::project::lifecycle::{ProjectLease, ProjectManager};
use crate::storage::{repository::storage_error, runs};
use crate::{
    app::contracts::{ErrorCode, ProjectKind},
    app::requests::{LanguagePair, ProjectChoices},
    storage::shared,
};
use rusqlite::{params, Transaction};
use std::sync::Mutex;
use std::{
    future::Future,
    pin::Pin,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

struct Fixture {
    root: std::path::PathBuf,
    manager: ProjectManager,
    id: ProjectId,
    chapter: String,
}
impl Fixture {
    fn new(stages: Vec<String>) -> Self {
        let root = std::env::temp_dir().join(format!("durable-{}", uuid::Uuid::new_v4()));
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
                    name: "Job test".into(),
                    languages: LanguagePair {
                        source: Some("en".into()),
                        target: "ru".into(),
                    },
                    processing_profile_id: None,
                },
            )
            .unwrap();
        let chapter = manager
            .lease(&project.id)
            .unwrap()
            .with_connection(|db, _| {
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
                    "run",
                    "test",
                    &runs::RunSnapshot {
                        settings: settings.choices,
                        settings_revision: settings.revision,
                        glossary_revision: shared::glossary_revision(db)?,
                        selected_ids: vec![chapter.clone()],
                        prompt_version: "fake-v1".into(),
                        stages,
                        provider: None,
                        instructions: None,
                    },
                    "now",
                )?;
                Ok(chapter)
            })
            .unwrap();
        Self {
            root,
            manager,
            id: project.id,
            chapter,
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}
struct Fake {
    fail_summary: AtomicBool,
    calls: Mutex<Vec<String>>,
    edit_during_request: bool,
}
impl StepExecutor for Fake {
    type Output = (String, String);
    fn fingerprint(
        &self,
        lease: &ProjectLease,
        _: &runs::RunRecord,
        entity: &str,
        stage: &str,
    ) -> Result<String, AppError> {
        lease.with_connection(|db, _| {
            let revision: i64 = db
                .query_row(
                    "SELECT revision FROM book_chapters WHERE id=?1",
                    [entity],
                    |r| r.get(0),
                )
                .map_err(storage_error)?;
            Ok(format!("{entity}:{stage}:{revision}"))
        })
    }
    fn compute<'a>(
        &'a self,
        lease: &'a ProjectLease,
        _: &'a runs::RunRecord,
        entity: &'a str,
        stage: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<Self::Output, AppError>> + Send + 'a>> {
        Box::pin(async move {
            self.calls.lock().unwrap().push(stage.into());
            if stage == "summary" && self.fail_summary.swap(false, Ordering::AcqRel) {
                return Err(AppError::invalid("fakeFailure"));
            }
            if self.edit_during_request {
                lease.with_connection(|db, _| {
                    db.execute(
                        "UPDATE book_chapters SET revision=revision+1 WHERE id=?1",
                        [entity],
                    )
                    .map_err(storage_error)?;
                    Ok(())
                })?;
            }
            Ok((stage.into(), entity.into()))
        })
    }
    fn persist(
        &self,
        tx: &Transaction<'_>,
        (stage, entity): Self::Output,
    ) -> Result<String, AppError> {
        tx.execute(
            "UPDATE book_chapters SET instructions=?1 WHERE id=?2",
            params![stage, entity],
        )
        .map_err(storage_error)?;
        Ok(format!("{entity}:{stage}"))
    }
    fn entity_kind(&self) -> &'static str {
        "chapter"
    }
}

#[tokio::test]
async fn resume_skips_committed_translation_when_summary_failed() {
    let f = Fixture::new(vec!["translation".into(), "summary".into()]);
    let fake = Fake {
        fail_summary: AtomicBool::new(true),
        calls: Mutex::new(vec![]),
        edit_during_request: false,
    };
    let cancel = Arc::new(AtomicBool::new(false));
    let events = Mutex::new(Vec::new());
    assert!(
        execute(&f.manager, &f.id, "run", &fake, cancel.clone(), |e| events
            .lock()
            .unwrap()
            .push(e))
        .await
        .is_err()
    );
    execute(&f.manager, &f.id, "run", &fake, cancel, |e| {
        events.lock().unwrap().push(e)
    })
    .await
    .unwrap();
    assert_eq!(
        *fake.calls.lock().unwrap(),
        vec!["translation", "summary", "summary"]
    );
    let events = events.lock().unwrap();
    assert!(events
        .windows(2)
        .all(|e| e[0].seq.value().unwrap() < e[1].seq.value().unwrap()));
    f.manager
        .lease(&f.id)
        .unwrap()
        .with_connection(|db, _| {
            assert_eq!(runs::get_run(db, "run")?.state, JobState::Succeeded);
            Ok(())
        })
        .unwrap();
}
#[tokio::test]
async fn late_result_cannot_overwrite_new_source() {
    let f = Fixture::new(vec!["translation".into()]);
    let fake = Fake {
        fail_summary: AtomicBool::new(false),
        calls: Mutex::new(vec![]),
        edit_during_request: true,
    };
    assert_eq!(
        execute(
            &f.manager,
            &f.id,
            "run",
            &fake,
            Arc::new(AtomicBool::new(false)),
            |_| {}
        )
        .await
        .unwrap_err()
        .code,
        ErrorCode::RevisionConflict
    );
    f.manager
        .lease(&f.id)
        .unwrap()
        .with_connection(|db, _| {
            assert_eq!(
                db.query_row(
                    "SELECT instructions FROM book_chapters WHERE id=?1",
                    [&f.chapter],
                    |r| r.get::<_, String>(0)
                )
                .unwrap(),
                ""
            );
            Ok(())
        })
        .unwrap();
}

struct Slow;
impl StepExecutor for Slow {
    type Output = ();
    fn fingerprint(
        &self,
        _: &ProjectLease,
        _: &runs::RunRecord,
        _: &str,
        _: &str,
    ) -> Result<String, AppError> {
        Ok("unchanged".into())
    }
    fn compute<'a>(
        &'a self,
        _: &'a ProjectLease,
        _: &'a runs::RunRecord,
        _: &'a str,
        _: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<(), AppError>> + Send + 'a>> {
        Box::pin(std::future::pending())
    }
    fn persist(&self, _: &Transaction<'_>, _: ()) -> Result<String, AppError> {
        panic!("cancelled work must not persist")
    }
    fn entity_kind(&self) -> &'static str {
        "chapter"
    }
}
#[tokio::test]
async fn cancellation_drops_pending_request_and_records_terminal_state() {
    let f = Fixture::new(vec!["translation".into()]);
    let cancel = Arc::new(AtomicBool::new(false));
    let trigger = cancel.clone();
    let run = execute(&f.manager, &f.id, "run", &Slow, cancel, |_| {});
    let stop = async move {
        tokio::time::sleep(Duration::from_millis(50)).await;
        trigger.store(true, Ordering::Release);
    };
    let (result, _) =
        tokio::time::timeout(Duration::from_secs(3), async { tokio::join!(run, stop) })
            .await
            .unwrap();
    assert_eq!(result.unwrap_err().code, ErrorCode::JobCancelled);
    f.manager
        .lease(&f.id)
        .unwrap()
        .with_connection(|db, _| {
            assert_eq!(runs::get_run(db, "run")?.state, JobState::Cancelled);
            Ok(())
        })
        .unwrap();
}

#[tokio::test]
async fn a_pending_project_does_not_block_another_and_deletion_drains_it() {
    let pending = Arc::new(Fixture::new(vec!["translation".into()]));
    let other = Fixture::new(vec!["translation".into()]);
    let started = Arc::new(tokio::sync::Notify::new());
    let signal = started.clone();
    let task_fixture = pending.clone();
    let task = tokio::spawn(async move {
        execute(
            &task_fixture.manager,
            &task_fixture.id,
            "run",
            &Slow,
            Arc::new(AtomicBool::new(false)),
            |event| {
                if matches!(
                    event.event,
                    crate::app::contracts::EventPayload::JobUpdated {
                        state: JobState::Running
                    }
                ) {
                    signal.notify_one();
                }
            },
        )
        .await
    });
    tokio::time::timeout(Duration::from_secs(3), started.notified())
        .await
        .unwrap();
    let fake = Fake {
        fail_summary: AtomicBool::new(false),
        calls: Mutex::new(vec![]),
        edit_during_request: false,
    };
    tokio::time::timeout(
        Duration::from_secs(3),
        execute(
            &other.manager,
            &other.id,
            "run",
            &fake,
            Arc::new(AtomicBool::new(false)),
            |_| {},
        ),
    )
    .await
    .unwrap()
    .unwrap();
    let deleting = pending.clone();
    tokio::time::timeout(
        Duration::from_secs(3),
        tokio::task::spawn_blocking(move || deleting.manager.delete(&deleting.id)),
    )
    .await
    .unwrap()
    .unwrap()
    .unwrap();
    assert_eq!(
        task.await.unwrap().unwrap_err().code,
        ErrorCode::JobCancelled
    );
    assert!(!pending
        .root
        .join("projects")
        .join(pending.id.as_str())
        .exists());
    assert!(pending.manager.lease(&pending.id).is_err());
}
