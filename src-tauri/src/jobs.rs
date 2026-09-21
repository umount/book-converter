//! Background job lifecycle plus operation-specific runners.

mod retarget;
mod translation;

use std::future::Future;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use tauri::{AppHandle, Emitter, Manager};

use crate::session::AppState;

pub(crate) use retarget::run as run_retarget;
pub(crate) use translation::run as run_translation;

/// A project's single background-job slot, held for as long as this value lives.
pub(crate) struct JobSlot {
    app: AppHandle,
    project_id: String,
    pub(crate) db: String,
    pub(crate) cancel: Arc<AtomicBool>,
}

impl Drop for JobSlot {
    fn drop(&mut self) {
        if let Some(state) = self.app.try_state::<crate::session::AppState>() {
            state.finish_job(&self.project_id);
        }
    }
}

/// The only way to mark a project busy.
pub(crate) fn lease(
    app: &AppHandle,
    state: &AppState,
    project_id: &str,
) -> Result<JobSlot, String> {
    let (db, cancel) = state.begin_job(project_id)?;
    Ok(JobSlot {
        app: app.clone(),
        project_id: project_id.to_string(),
        db,
        cancel,
    })
}

/// Run a project's async operation on its own OS thread/runtime. Consumes the
/// lease so `Drop` releases the slot on every path, panics included.
pub(crate) fn spawn<T, F, Fut, S>(slot: JobSlot, run: F, on_success: S)
where
    T: Send + 'static,
    F: FnOnce(AppHandle, String, Arc<AtomicBool>) -> Fut + Send + 'static,
    Fut: Future<Output = anyhow::Result<T>> + 'static,
    S: FnOnce(&AppHandle, &str, T) + Send + 'static,
{
    std::thread::spawn(move || {
        let app = slot.app.clone();
        let project_id = slot.project_id.clone();
        let cancel = slot.cancel.clone();
        let result = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(anyhow::Error::from)
            .and_then(|runtime| runtime.block_on(run(app.clone(), project_id.clone(), cancel)));

        drop(slot);
        match result {
            Ok(value) => on_success(&app, &project_id, value),
            Err(error) => {
                let _ = app.emit(
                    "job_error",
                    serde_json::json!({
                        "project": project_id,
                        "message": error.to_string(),
                    }),
                );
            }
        }
    });
}
