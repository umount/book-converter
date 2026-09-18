//! Background job lifecycle plus operation-specific runners.

mod retarget;
mod translation;

use std::future::Future;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use tauri::{AppHandle, Emitter, Manager};

pub(crate) use retarget::run as run_retarget;
pub(crate) use translation::run as run_translation;

struct JobCleanup {
    app: AppHandle,
    project_id: String,
}

impl Drop for JobCleanup {
    fn drop(&mut self) {
        if let Some(state) = self.app.try_state::<crate::session::AppState>() {
            state.finish_job(&self.project_id);
        }
    }
}

/// Run a project's async operation on its own OS thread/runtime and guarantee
/// that the session job slot is released on every normal `Result` path.
///
/// `on_success` selects the operation-specific success event; failures share
/// the stable `job_error` event.
pub(crate) fn spawn_project_job<T, F, Fut, S>(
    app: AppHandle,
    project_id: String,
    cancel: Arc<AtomicBool>,
    run: F,
    on_success: S,
) where
    T: Send + 'static,
    F: FnOnce(AppHandle, String, Arc<AtomicBool>) -> Fut + Send + 'static,
    Fut: Future<Output = anyhow::Result<T>> + 'static,
    S: FnOnce(&AppHandle, &str, T) + Send + 'static,
{
    std::thread::spawn(move || {
        // Drop also runs during unwinding, so a panic cannot leave the project
        // permanently marked as busy.
        let cleanup = JobCleanup {
            app: app.clone(),
            project_id: project_id.clone(),
        };
        let result = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(anyhow::Error::from)
            .and_then(|runtime| {
                runtime.block_on(run(app.clone(), project_id.clone(), cancel))
            });

        drop(cleanup);
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
