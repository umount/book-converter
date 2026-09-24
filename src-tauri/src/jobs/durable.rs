//! UI-independent durable job runner; each successful result and step share a transaction.
use crate::{
    app::contracts::{AppError, ErrorCode, EventPayload, JobState, ProjectEvent, ProjectId},
    project::lifecycle::{ProjectLease, ProjectManager},
    storage::{repository::storage_error, runs},
};
use rusqlite::{params, OptionalExtension, Transaction};
use std::{
    future::Future,
    pin::Pin,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

pub trait StepExecutor: Send + Sync {
    type Output: Send;
    /// Includes source/dependency versions, role/model, prompt version and options, never credentials.
    fn fingerprint(
        &self,
        lease: &ProjectLease,
        run: &runs::RunRecord,
        entity: &str,
        stage: &str,
    ) -> Result<String, AppError>;
    fn compute<'a>(
        &'a self,
        lease: &'a ProjectLease,
        run: &'a runs::RunRecord,
        entity: &'a str,
        stage: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<Self::Output, AppError>> + Send + 'a>>;
    fn persist(&self, tx: &Transaction<'_>, output: Self::Output) -> Result<String, AppError>;
    /// Domain-specific prerequisite order; each pair is still a durable step.
    fn steps(&self, run: &runs::RunRecord) -> Vec<(String, String)> {
        run.snapshot
            .selected_ids
            .iter()
            .flat_map(|entity| {
                run.snapshot
                    .stages
                    .iter()
                    .map(move |stage| (entity.clone(), stage.clone()))
            })
            .collect()
    }
    /// Advance dependencies produced by this job in the same publication transaction.
    fn after_persist(
        &self,
        _tx: &Transaction<'_>,
        _run: &runs::RunRecord,
        _stage: &str,
    ) -> Result<(), AppError> {
        Ok(())
    }
    fn entity_kind(&self) -> &'static str;
}

fn cancelled() -> AppError {
    AppError {
        code: ErrorCode::JobCancelled,
        message_key: "errors.jobCancelled".into(),
        params: Default::default(),
        retryable: false,
    }
}
fn now() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .to_string()
}

fn emit(
    lease: &ProjectLease,
    project: &ProjectId,
    id: &str,
    sink: &impl Fn(ProjectEvent),
) -> Result<(), AppError> {
    let record = lease.with_connection(|db, _| runs::get_run(db, id))?;
    sink(ProjectEvent {
        version: 1,
        project_id: project.clone(),
        job_id: Some(id.into()),
        seq: record.revision,
        event: EventPayload::JobUpdated {
            state: record.state,
        },
    });
    Ok(())
}
fn transition(
    lease: &ProjectLease,
    id: &str,
    state: JobState,
    error: Option<&AppError>,
) -> Result<(), AppError> {
    lease.with_connection(|db, _| {
        let revision = runs::get_run(db, id)?.revision;
        runs::transition(db, id, &revision, state, error, &now())?;
        Ok(())
    })
}

/// The caller reserves one run in SQLite before spawning. Different projects can progress independently.
pub async fn execute<E: StepExecutor>(
    manager: &ProjectManager,
    project: &ProjectId,
    id: &str,
    executor: &E,
    cancel: Arc<AtomicBool>,
    sink: impl Fn(ProjectEvent),
) -> Result<(), AppError> {
    let lease = manager.lease(project)?;
    let record = lease.with_connection(|db, _| runs::get_run(db, id))?;
    match record.state {
        JobState::Interrupted | JobState::Failed | JobState::Cancelled => {
            transition(&lease, id, JobState::Queued, None)?
        }
        JobState::Queued => {}
        _ => return Err(AppError::invalid("jobState")),
    }
    transition(&lease, id, JobState::Running, None)?;
    emit(&lease, project, id, &sink)?;
    let result = execute_steps(&lease, id, executor, &cancel, &sink, project).await;
    // Deletion has already sealed new writes and waits for this lease to drop.
    if lease.cancelled() {
        return Err(cancelled());
    }
    match &result {
        Ok(()) => transition(&lease, id, JobState::Succeeded, None)?,
        Err(error) => {
            lease.with_connection(|db, _| {
                db.execute(
                    "UPDATE job_steps SET state=?1,error=?2 WHERE run_id=?3 AND state='running'",
                    params![
                        if error.code == ErrorCode::JobCancelled {
                            "cancelled"
                        } else {
                            "failed"
                        },
                        serde_json::to_string(error).map_err(|_| AppError::invalid("error"))?,
                        id
                    ],
                )
                .map_err(storage_error)?;
                Ok(())
            })?;
            if error.code == ErrorCode::JobCancelled {
                transition(&lease, id, JobState::Cancelling, None)?;
                transition(&lease, id, JobState::Cancelled, None)?;
            } else {
                transition(&lease, id, JobState::Failed, Some(error))?;
            }
        }
    }
    emit(&lease, project, id, &sink)?;
    result
}

async fn execute_steps<E: StepExecutor>(
    lease: &ProjectLease,
    id: &str,
    executor: &E,
    cancel: &AtomicBool,
    sink: &impl Fn(ProjectEvent),
    project: &ProjectId,
) -> Result<(), AppError> {
    let run = lease.with_connection(|db, _| runs::get_run(db, id))?;
    for (entity, stage) in executor.steps(&run) {
        let run = lease.with_connection(|db, _| runs::get_run(db, id))?;
        let (entity, stage) = (&entity, &stage);
        if cancel.load(Ordering::Acquire) || lease.cancelled() {
            return Err(cancelled());
        }
        let fingerprint = executor.fingerprint(lease, &run, entity, stage)?;
        let prior=lease.with_connection(|db,_|db.query_row("SELECT attempt,state,input_fingerprint FROM job_steps WHERE run_id=?1 AND entity_kind=?2 AND entity_id=?3 AND stage=?4 ORDER BY attempt DESC LIMIT 1",params![id,executor.entity_kind(),entity,stage],|r|Ok((r.get::<_,u32>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?))).optional().map_err(storage_error))?;
        if prior
            .as_ref()
            .is_some_and(|(_, state, hash)| state == "succeeded" && hash == &fingerprint)
        {
            continue;
        }
        let step = runs::StepAttempt {
            id: uuid::Uuid::new_v4().to_string(),
            run_id: id.into(),
            entity_kind: executor.entity_kind().into(),
            entity_id: entity.clone(),
            stage: stage.clone(),
            attempt: prior.map(|p| p.0 + 1).unwrap_or(1),
            input_fingerprint: fingerprint,
        };
        lease.with_connection(|db, _| runs::begin_step(db, &step))?;
        let start = std::time::Instant::now();
        let output = tokio::select! {
            result=executor.compute(lease,&run,entity,stage)=>result?,
            _=async {while !cancel.load(Ordering::Acquire)&&!lease.cancelled(){tokio::time::sleep(Duration::from_millis(25)).await;}}=>return Err(cancelled()),
        };
        if cancel.load(Ordering::Acquire) || lease.cancelled() {
            return Err(cancelled());
        }
        // Reject a result if its source or dependencies changed while the provider was working.
        if executor.fingerprint(lease, &run, entity, stage)? != step.input_fingerprint {
            return Err(AppError {
                code: ErrorCode::RevisionConflict,
                message_key: "errors.revisionConflict".into(),
                params: Default::default(),
                retryable: false,
            });
        }
        lease.with_connection(|db, _| {
            let tx = db.transaction().map_err(storage_error)?;
            let reference = executor.persist(&tx, output)?;
            executor.after_persist(&tx, &run, stage)?;
            runs::finish_step(
                &tx,
                &step.id,
                &reference,
                u32::try_from(start.elapsed().as_millis()).unwrap_or(u32::MAX),
            )?;
            tx.commit().map_err(storage_error)?;
            Ok(())
        })?;
        emit(lease, project, id, sink)?;
    }
    Ok(())
}
