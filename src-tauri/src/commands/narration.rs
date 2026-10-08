use crate::{
    app::{contracts::AppError, requests::ProjectArgs, services::AppContext},
    models::ModelFailure,
    narration::{contracts::*, Runtime},
};
use tauri::Manager;

async fn runtime(app: &tauri::AppHandle) -> Result<Runtime, AppError> {
    let resources = app
        .path()
        .resource_dir()
        .map_err(|_| crate::narration::failure("audioRuntime"))?;
    tokio::task::spawn_blocking(move || Runtime::discover(&resources))
        .await
        .map_err(|_| crate::narration::failure("audioRuntime"))?
}
#[tauri::command]
pub async fn audio_setup(
    app: tauri::AppHandle,
    context: tauri::State<'_, AppContext>,
) -> Result<AudioSetupView, ModelFailure> {
    let runtime_ready = match app.path().resource_dir() {
        Ok(resources) => tokio::task::spawn_blocking(move || Runtime::is_available(&resources))
            .await
            .unwrap_or(false),
        Err(_) => false,
    };
    Ok(AudioSetupView {
        runtime_ready,
        engine: context.narration.engine_view(),
        files: context.models.list().await?,
        downloading: context.models.busy(),
    })
}
#[tauri::command]
pub async fn audio_engine_load(
    app: tauri::AppHandle,
    context: tauri::State<'_, AppContext>,
    args: AudioLoadArgs,
) -> Result<(), AppError> {
    context
        .narration
        .load_engine(context.models.clone(), runtime(&app).await?, args.device)
}
#[tauri::command]
pub async fn audio_engine_unload(context: tauri::State<'_, AppContext>) -> Result<(), AppError> {
    context.narration.unload_engine().await
}
#[tauri::command]
pub async fn audio_models_download(
    context: tauri::State<'_, AppContext>,
) -> Result<(), ModelFailure> {
    context.models.start_bundle().await
}
#[tauri::command]
pub fn audio_models_pause(context: tauri::State<'_, AppContext>) {
    context.models.pause_bundle();
}
#[tauri::command]
pub async fn audio_start(
    app: tauri::AppHandle,
    context: tauri::State<'_, AppContext>,
    args: AudioStartArgs,
) -> Result<AudioJobView, AppError> {
    context
        .narration
        .start(
            context.manager.clone(),
            context.models.clone(),
            runtime(&app).await?,
            args,
        )
        .await
}
#[tauri::command]
pub async fn audio_resume(
    app: tauri::AppHandle,
    context: tauri::State<'_, AppContext>,
    args: AudioJobArgs,
) -> Result<AudioJobView, AppError> {
    context
        .narration
        .resume(
            context.manager.clone(),
            context.models.clone(),
            runtime(&app).await?,
            args,
        )
        .await
}
#[tauri::command]
pub fn audio_cancel(
    context: tauri::State<'_, AppContext>,
    args: AudioJobArgs,
) -> Result<(), AppError> {
    context.narration.cancel(&args)
}
#[tauri::command]
pub async fn audio_list(
    context: tauri::State<'_, AppContext>,
    args: ProjectArgs,
) -> Result<Vec<AudioJobView>, AppError> {
    let service = context.narration.clone();
    let manager = context.manager.clone();
    tokio::task::spawn_blocking(move || {
        let _lease = manager.lease(&args.project_id)?;
        service.list(&args.project_id)
    })
    .await
    .map_err(|_| crate::narration::failure("audioStorage"))?
}
#[tauri::command]
pub async fn audio_export(
    context: tauri::State<'_, AppContext>,
    args: AudioExportArgs,
) -> Result<String, AppError> {
    let service = context.narration.clone();
    let manager = context.manager.clone();
    tokio::task::spawn_blocking(move || service.export(&manager, &args))
        .await
        .map_err(|_| crate::narration::failure("audioStorage"))?
}

#[tauri::command]
pub async fn audio_preview(
    app: tauri::AppHandle,
    context: tauri::State<'_, AppContext>,
    args: AudioJobArgs,
) -> Result<AudioPreviewView, AppError> {
    context
        .narration
        .preview(&context.manager, runtime(&app).await?, &args)
        .await
}

#[tauri::command]
pub async fn audio_preview_export(
    context: tauri::State<'_, AppContext>,
    args: AudioPreviewExportArgs,
) -> Result<String, AppError> {
    let service = context.narration.clone();
    let manager = context.manager.clone();
    tokio::task::spawn_blocking(move || service.export_preview(&manager, &args))
        .await
        .map_err(|_| crate::narration::failure("audioStorage"))?
}
