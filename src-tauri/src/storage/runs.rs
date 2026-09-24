//! Durable run/step records. Scheduling and cancellation tokens belong to P04.
use super::{
    repository::{conflict, storage_error},
    shared::{next, ProcessingSettings},
};
use crate::app::contracts::{AppError, JobState, Revision};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunSnapshot {
    pub settings: ProcessingSettings,
    pub settings_revision: Revision,
    pub glossary_revision: Revision,
    pub selected_ids: Vec<String>,
    pub prompt_version: String,
}

#[derive(Debug, Clone)]
pub struct RunRecord {
    pub id: String,
    pub kind: String,
    pub state: JobState,
    pub snapshot: RunSnapshot,
    pub revision: Revision,
    pub created_at: String,
    pub updated_at: String,
    pub terminal_error: Option<AppError>,
}

fn encode<T: Serialize>(value: &T) -> Result<String, AppError> {
    serde_json::to_string(value).map_err(|_| AppError::invalid("job"))
}
fn state_name(state: JobState) -> Result<String, AppError> {
    Ok(encode(&state)?.trim_matches('"').to_string())
}

pub fn create_run(
    db: &mut Connection,
    id: &str,
    kind: &str,
    snapshot: &RunSnapshot,
    now: &str,
) -> Result<(), AppError> {
    if id.is_empty()
        || kind.is_empty()
        || now.is_empty()
        || snapshot.selected_ids.is_empty()
        || snapshot.prompt_version.is_empty()
    {
        return Err(AppError::invalid("job"));
    }
    let unique: std::collections::HashSet<_> = snapshot.selected_ids.iter().collect();
    if unique.len() != snapshot.selected_ids.len() || unique.iter().any(|id| id.is_empty()) {
        return Err(AppError::invalid("selection"));
    }
    let tx = db.transaction().map_err(storage_error)?;
    if super::shared::settings(&tx)?.revision != snapshot.settings_revision
        || super::shared::glossary_revision(&tx)? != snapshot.glossary_revision
        || super::shared::settings(&tx)?.choices != snapshot.settings
    {
        return Err(conflict());
    }
    tx.execute("INSERT INTO job_runs(id,kind,state,settings_snapshot,created_at,updated_at) VALUES(?1,?2,'queued',?3,?4,?4)",params![id,kind,encode(snapshot)?,now]).map_err(storage_error)?;
    tx.commit().map_err(storage_error)
}

pub fn get_run(db: &Connection, id: &str) -> Result<RunRecord, AppError> {
    use rusqlite::OptionalExtension;
    let row = db.query_row("SELECT kind,state,settings_snapshot,revision,created_at,updated_at,terminal_error FROM job_runs WHERE id=?1",[id],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?,r.get::<_,i64>(3)?,r.get::<_,String>(4)?,r.get::<_,String>(5)?,r.get::<_,Option<String>>(6)?))).optional().map_err(storage_error)?.ok_or_else(super::repository::not_found)?;
    Ok(RunRecord {
        id: id.into(),
        kind: row.0,
        state: serde_json::from_value(serde_json::Value::String(row.1))
            .map_err(|_| AppError::invalid("jobState"))?,
        snapshot: serde_json::from_str(&row.2).map_err(|_| AppError::invalid("jobSnapshot"))?,
        revision: Revision(row.3.to_string()),
        created_at: row.4,
        updated_at: row.5,
        terminal_error: row
            .6
            .map(|e| serde_json::from_str(&e))
            .transpose()
            .map_err(|_| AppError::invalid("jobError"))?,
    })
}

fn allowed(from: JobState, to: JobState) -> bool {
    use JobState::*;
    matches!(
        (from, to),
        (Queued, Running | Cancelled | Failed)
            | (Running, Succeeded | Failed | Cancelling | Interrupted)
            | (Cancelling, Cancelled | Failed | Interrupted)
            | (Interrupted | Failed | Cancelled, Queued)
    )
}

pub fn transition(
    db: &mut Connection,
    id: &str,
    expected: &Revision,
    to: JobState,
    error: Option<&AppError>,
    now: &str,
) -> Result<Revision, AppError> {
    let tx = db.transaction().map_err(storage_error)?;
    let record = get_run(&tx, id)?;
    if record.revision != *expected {
        return Err(conflict());
    }
    if !allowed(record.state, to) || now.is_empty() || (to == JobState::Failed) != error.is_some() {
        return Err(AppError::invalid("jobTransition"));
    }
    if to == JobState::Succeeded {
        let unfinished: i64 = tx
            .query_row(
                "SELECT COUNT(*) FROM job_steps AS step WHERE run_id=?1 AND state!='succeeded' AND NOT EXISTS(SELECT 1 FROM job_steps AS newer WHERE newer.run_id=step.run_id AND newer.entity_kind=step.entity_kind AND newer.entity_id=step.entity_id AND newer.stage=step.stage AND newer.attempt>step.attempt)",
                [id],
                |r| r.get(0),
            )
            .map_err(storage_error)?;
        if unfinished > 0 {
            return Err(AppError::invalid("unfinishedSteps"));
        }
    }
    let revision = next(expected.value()?)?;
    let changed=tx.execute("UPDATE job_runs SET state=?1,terminal_error=?2,updated_at=?3,revision=?4 WHERE id=?5 AND revision=?6",params![state_name(to)?,error.map(encode).transpose()?,now,revision,id,expected.value()?]).map_err(storage_error)?;
    if changed != 1 {
        return Err(conflict());
    }
    tx.commit().map_err(storage_error)?;
    Ok(Revision(revision.to_string()))
}

#[derive(Debug, Clone)]
pub struct StepAttempt {
    pub id: String,
    pub run_id: String,
    pub entity_kind: String,
    pub entity_id: String,
    pub stage: String,
    pub attempt: u32,
    pub input_fingerprint: String,
}

pub fn begin_step(db: &mut Connection, step: &StepAttempt) -> Result<(), AppError> {
    if step.attempt == 0
        || step.id.is_empty()
        || step.stage.is_empty()
        || step.input_fingerprint.is_empty()
    {
        return Err(AppError::invalid("step"));
    }
    let tx = db.transaction().map_err(storage_error)?;
    let run = get_run(&tx, &step.run_id)?;
    if run.state != JobState::Running || !run.snapshot.selected_ids.contains(&step.entity_id) {
        return Err(AppError::invalid("stepSelection"));
    }
    let query = match step.entity_kind.as_str() {
        "chapter" => "SELECT COUNT(*) FROM book_chapters WHERE id=?1",
        "page" => "SELECT COUNT(*) FROM manga_pages WHERE id=?1",
        _ => return Err(AppError::invalid("entityKind")),
    };
    if tx
        .query_row(query, [&step.entity_id], |r| r.get::<_, i64>(0))
        .map_err(storage_error)?
        != 1
    {
        return Err(super::repository::not_found());
    }
    let (attempt, blocked): (i64,i64) = tx.query_row("SELECT COALESCE(MAX(attempt),0),COALESCE(SUM(state IN ('running','queued','cancelling') OR (state='succeeded' AND input_fingerprint=?5)),0) FROM job_steps WHERE run_id=?1 AND entity_kind=?2 AND entity_id=?3 AND stage=?4",params![step.run_id,step.entity_kind,step.entity_id,step.stage,step.input_fingerprint],|r|Ok((r.get(0)?,r.get(1)?))).map_err(storage_error)?;
    if i64::from(step.attempt) != attempt + 1 || blocked > 0 {
        return Err(conflict());
    }
    tx.execute("INSERT INTO job_steps(id,run_id,entity_kind,entity_id,stage,attempt,input_fingerprint,state) VALUES(?1,?2,?3,?4,?5,?6,?7,'running')",params![step.id,step.run_id,step.entity_kind,step.entity_id,step.stage,step.attempt,step.input_fingerprint]).map_err(storage_error)?;
    tx.commit().map_err(storage_error)
}

/// Called inside the same transaction that persists the domain result.
pub fn finish_step(
    tx: &rusqlite::Transaction<'_>,
    id: &str,
    output: &str,
    duration_ms: u32,
) -> Result<(), AppError> {
    if output.is_empty() {
        return Err(AppError::invalid("stepOutput"));
    }
    let changed=tx.execute("UPDATE job_steps SET state='succeeded',output_reference=?1,duration_ms=?2 WHERE id=?3 AND state='running' AND EXISTS(SELECT 1 FROM job_runs WHERE job_runs.id=job_steps.run_id AND job_runs.state='running')",params![output,duration_ms,id]).map_err(storage_error)?;
    if changed != 1 {
        return Err(conflict());
    }
    Ok(())
}

/// Startup recovery never requeues work or repeats a paid request automatically.
pub fn interrupt_running(db: &mut Connection, now: &str) -> Result<usize, AppError> {
    let tx = db.transaction().map_err(storage_error)?;
    tx.execute(
        "UPDATE job_steps SET state='interrupted' WHERE state IN ('running','cancelling')",
        [],
    )
    .map_err(storage_error)?;
    let changed=tx.execute("UPDATE job_runs SET state='interrupted',updated_at=?1,revision=revision+1 WHERE state IN ('running','cancelling')",[now]).map_err(storage_error)?;
    tx.commit().map_err(storage_error)?;
    Ok(changed)
}
