//! Durable run/step records. Scheduling and cancellation tokens belong to P04.
use super::{
    repository::{conflict, storage_error},
    shared::{next, ProcessingSettings},
};
use crate::app::contracts::{AppError, JobState, Revision};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetargetPlan {
    pub old_target: String,
    pub target: String,
    pub source: String,
    pub kind: String,
    pub translations: Vec<(String, String, Revision)>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunSnapshot {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retarget: Option<RetargetPlan>,
    pub settings: ProcessingSettings,
    pub settings_revision: Revision,
    pub glossary_revision: Revision,
    pub selected_ids: Vec<String>,
    pub prompt_version: String,
    pub stages: Vec<String>,
    pub provider: Option<crate::ai::ProviderProfile>,
    pub instructions: Option<String>,
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
        || snapshot.stages.is_empty()
        || snapshot.stages.iter().any(|s| s.is_empty())
        || snapshot
            .stages
            .iter()
            .collect::<std::collections::HashSet<_>>()
            .len()
            != snapshot.stages.len()
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
        let completed: usize = tx.query_row(
            "SELECT COUNT(*) FROM job_steps AS step WHERE run_id=?1 AND state='succeeded' AND NOT EXISTS(SELECT 1 FROM job_steps AS newer WHERE newer.run_id=step.run_id AND newer.entity_kind=step.entity_kind AND newer.entity_id=step.entity_id AND newer.stage=step.stage AND newer.attempt>step.attempt)",
            [id], |r| r.get(0),
        ).map_err(storage_error)?;
        if unfinished > 0
            || completed != record.snapshot.selected_ids.len() * record.snapshot.stages.len()
        {
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
    if run.state != JobState::Running
        || !run.snapshot.selected_ids.contains(&step.entity_id)
        || !run.snapshot.stages.contains(&step.stage)
    {
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
    tx.execute("UPDATE job_runs SET revision=revision+1 WHERE id=(SELECT run_id FROM job_steps WHERE id=?1)",[id]).map_err(storage_error)?;
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

/// Remaining processing time, refreshed at step boundaries (not a wall-clock countdown).
/// Prefer this run's samples; otherwise use the last 30 comparable stage samples.
pub fn remaining_seconds(db: &Connection, run: &RunRecord) -> Result<Option<u32>, AppError> {
    if run.state == JobState::Succeeded {
        return Ok(Some(0));
    }
    if !matches!(run.state, JobState::Queued | JobState::Running) {
        return Ok(None);
    }
    estimate_remaining(
        db,
        &run.id,
        &run.snapshot.stages,
        run.snapshot.selected_ids.len(),
    )
}

fn estimate_remaining(
    db: &Connection,
    id: &str,
    stages: &[String],
    count: usize,
) -> Result<Option<u32>, AppError> {
    let mut remaining_ms = 0.0_f64;
    for stage in stages {
        let completed: usize = db.query_row(
            "SELECT COUNT(*) FROM job_steps s WHERE s.run_id=?1 AND s.stage=?2 AND s.state='succeeded'
             AND NOT EXISTS(SELECT 1 FROM job_steps n WHERE n.run_id=s.run_id AND n.entity_kind=s.entity_kind
             AND n.entity_id=s.entity_id AND n.stage=s.stage AND n.attempt>s.attempt)",
            params![id, stage], |r| r.get(0)).map_err(storage_error)?;
        let remaining = count.saturating_sub(completed);
        if remaining == 0 {
            continue;
        }
        let average: Option<f64> = db.query_row(
            "SELECT AVG(duration_ms) FROM (
                SELECT s.duration_ms FROM job_steps s JOIN job_runs r ON r.id=s.run_id
                JOIN job_runs current ON current.id=?1
                WHERE s.stage=?2 AND s.state='succeeded' AND s.duration_ms IS NOT NULL
                AND r.kind=current.kind
                AND json_extract(r.settings_snapshot,'$.settings')=json_extract(current.settings_snapshot,'$.settings')
                AND json_extract(r.settings_snapshot,'$.provider') IS json_extract(current.settings_snapshot,'$.provider')
                AND (s.run_id=?1 OR NOT EXISTS(SELECT 1 FROM job_steps own
                    WHERE own.run_id=?1 AND own.stage=?2 AND own.state='succeeded' AND own.duration_ms IS NOT NULL))
                ORDER BY s.rowid DESC LIMIT 30
             )", params![id, stage], |r| r.get(0)).map_err(storage_error)?;
        let Some(average) = average else {
            return Ok(None);
        };
        remaining_ms += average * remaining as f64;
    }
    Ok(Some(
        (remaining_ms / 1000.0).ceil().min(u32::MAX as f64) as u32
    ))
}

#[cfg(test)]
mod eta_tests {
    use super::*;
    #[test]
    fn estimates_each_stage_prefers_current_samples_and_counts_latest_attempts() {
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch("CREATE TABLE job_runs(id TEXT,kind TEXT,settings_snapshot TEXT);
            CREATE TABLE job_steps(run_id TEXT,entity_kind TEXT,entity_id TEXT,stage TEXT,attempt INTEGER,state TEXT,duration_ms INTEGER);
            INSERT INTO job_runs VALUES('old','book_translation','{\"settings\":{}}'),('new','book_translation','{\"settings\":{}}');
            INSERT INTO job_steps VALUES('old','chapter','a','translation',1,'succeeded',10000);").unwrap();
        let stages = vec!["translation".into(), "context".into()];
        assert_eq!(estimate_remaining(&db, "new", &stages, 3).unwrap(), None);
        db.execute_batch(
            "INSERT INTO job_steps VALUES('old','chapter','a','context',1,'succeeded',2000);",
        )
        .unwrap();
        assert_eq!(
            estimate_remaining(&db, "new", &stages, 3).unwrap(),
            Some(36)
        );
        db.execute_batch(
            "INSERT INTO job_steps VALUES('new','chapter','a','translation',1,'succeeded',4000);",
        )
        .unwrap();
        assert_eq!(
            estimate_remaining(&db, "new", &stages, 3).unwrap(),
            Some(14)
        );
        db.execute_batch(
            "INSERT INTO job_steps VALUES('new','chapter','a','translation',2,'failed',NULL);",
        )
        .unwrap();
        assert_eq!(
            estimate_remaining(&db, "new", &stages, 3).unwrap(),
            Some(18)
        );
        db.execute_batch("UPDATE job_runs SET settings_snapshot='{\"settings\":{\"model\":\"other\"}}' WHERE id='old';").unwrap();
        assert_eq!(estimate_remaining(&db, "new", &stages, 3).unwrap(), None);
        assert_eq!(estimate_remaining(&db, "new", &stages, 0).unwrap(), Some(0));
    }
}
