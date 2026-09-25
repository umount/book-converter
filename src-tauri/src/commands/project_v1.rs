//! Versioned project commands delegate to UI-independent project services.
use crate::app::contracts::{AppError, ProjectDescriptor};
use crate::app::services::AppContext;

#[tauri::command]
pub fn project_inspect_manifest(
    context: tauri::State<'_, AppContext>,
    path: String,
) -> Result<ProjectDescriptor, AppError> {
    context.inspect_manifest(&path)
}

use crate::app::requests::*;

#[tauri::command]
pub async fn project_list(
    context: tauri::State<'_, AppContext>,
) -> Result<Vec<ProjectSummary>, AppError> {
    let manager = context.manager.clone();
    tauri::async_runtime::spawn_blocking(move || manager.catalog())
        .await
        .map_err(|_| AppError::invalid("task"))?
}
#[tauri::command]
pub async fn project_inspect_source(
    context: tauri::State<'_, AppContext>,
    args: InspectSourceArgs,
    on_progress: tauri::ipc::Channel<ImportProgress>,
) -> Result<ImportPreview, AppError> {
    let manager = context.manager.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut last = std::time::Instant::now();
        manager.inspect_source_with_progress(
            args.kind,
            std::path::Path::new(&args.path),
            &mut |value| {
                if value.completed == 0
                    || value.total == Some(value.completed)
                    || last.elapsed() >= std::time::Duration::from_millis(100)
                {
                    let _ = on_progress.send(value);
                    last = std::time::Instant::now();
                }
            },
        )
    })
    .await
    .map_err(|_| AppError::invalid("task"))?
}
#[tauri::command]
pub async fn project_create(
    context: tauri::State<'_, AppContext>,
    args: CreateProjectArgs,
) -> Result<ProjectDescriptor, AppError> {
    let manager = context.manager.clone();
    tauri::async_runtime::spawn_blocking(move || manager.create(&args.import_id.0, &args.choices))
        .await
        .map_err(|_| AppError::invalid("task"))?
}
#[tauri::command]
pub async fn project_cancel_import(
    context: tauri::State<'_, AppContext>,
    args: ImportSessionArgs,
) -> Result<(), AppError> {
    let manager = context.manager.clone();
    tauri::async_runtime::spawn_blocking(move || manager.cancel_import(&args.import_id.0))
        .await
        .map_err(|_| AppError::invalid("task"))?
}
#[tauri::command]
pub async fn project_open(
    context: tauri::State<'_, AppContext>,
    args: ProjectArgs,
) -> Result<ProjectDescriptor, AppError> {
    let manager = context.manager.clone();
    tauri::async_runtime::spawn_blocking(move || manager.open(&args.project_id))
        .await
        .map_err(|_| AppError::invalid("task"))?
}
#[tauri::command]
pub async fn project_delete(
    context: tauri::State<'_, AppContext>,
    args: ProjectArgs,
) -> Result<(), AppError> {
    let manager = context.manager.clone();
    tauri::async_runtime::spawn_blocking(move || manager.delete(&args.project_id))
        .await
        .map_err(|_| AppError::invalid("task"))?
}
#[tauri::command]
pub async fn project_archive_export(
    context: tauri::State<'_, AppContext>,
    args: ArchiveExportArgs,
) -> Result<(), AppError> {
    let manager = context.manager.clone();
    tauri::async_runtime::spawn_blocking(move || {
        manager.export_archive(&args.project_id, std::path::Path::new(&args.destination))
    })
    .await
    .map_err(|_| AppError::invalid("task"))?
}
#[tauri::command]
pub async fn project_archive_import(
    context: tauri::State<'_, AppContext>,
    args: ArchiveImportArgs,
) -> Result<ProjectDescriptor, AppError> {
    let manager = context.manager.clone();
    tauri::async_runtime::spawn_blocking(move || {
        manager.import_archive(std::path::Path::new(&args.path))
    })
    .await
    .map_err(|_| AppError::invalid("task"))?
}
