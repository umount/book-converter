//! Background translation job body and progress-event adapter.

use std::sync::atomic::AtomicBool;

use tauri::{AppHandle, Emitter};

use crate::config::Config;
use crate::dto::Progress;
use crate::orchestrator::Orchestrator;
use crate::state::Store;
use crate::translator::DeepSeekClient;

pub(crate) async fn run(
    project_id: &str,
    db: &str,
    limit: Option<usize>,
    only_index: Option<usize>,
    cancel: &AtomicBool,
    app: &AppHandle,
) -> anyhow::Result<()> {
    let store = Store::open(db)?;
    let config = Config::load_for(&store);
    let client = DeepSeekClient::new(config.clone())?;
    // Nothing else can translate this project while its job slot is held.
    let _ = store.recover();
    let style = store.project_metadata()?.reference_style;
    let mut orchestrator = Orchestrator::new(&client, &store, &config, style)?;
    let emit = |event: crate::orchestrator::ProgressEvent| {
        let _ = app.emit(
            "progress",
            Progress {
                project: project_id.to_string(),
                done: event.stats.done,
                total: event.stats.total,
                failed: event.stats.failed,
                pending: event.stats.pending,
                running: true,
                job_done: event.job_done,
                job_total: event.job_total,
                current_idx: event.current_idx,
                current_number: event.current_number,
                current_title: event.current_title,
                next_number: event.next_number,
                max_number: None,
                phase: event.phase.to_string(),
                last_ms: event.last_ms,
                eta_secs: event.eta_secs,
            },
        );
    };
    if let Some(index) = only_index {
        orchestrator.run_one(index, cancel, emit).await
    } else {
        orchestrator.run(limit, cancel, emit).await
    }
}
