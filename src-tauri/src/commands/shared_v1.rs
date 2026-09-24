//! Shared profiles and glossary IPC for versioned projects.
use crate::{
    app::{
        contracts::{AppError, Revision},
        requests::*,
        services::AppContext,
    },
    application::preferences,
};
use tauri::State;
#[tauri::command]
pub async fn project_settings_get(
    context: State<'_, AppContext>,
    args: ProjectArgs,
) -> Result<ProjectSettingsView, AppError> {
    let manager = context.manager.clone();
    tauri::async_runtime::spawn_blocking(move || {
        manager
            .lease(&args.project_id)?
            .with_connection(|db, _| preferences::settings(db))
    })
    .await
    .map_err(|_| AppError::invalid("task"))?
}
#[tauri::command]
pub async fn project_settings_update(
    context: State<'_, AppContext>,
    args: ProjectSettingsUpdateArgs,
) -> Result<Revision, AppError> {
    let manager = context.manager.clone();
    tauri::async_runtime::spawn_blocking(move || {
        manager
            .lease(&args.project_id)?
            .with_connection(|db, _| preferences::update_settings(db, &args))
    })
    .await
    .map_err(|_| AppError::invalid("task"))?
}
#[tauri::command]
pub async fn glossary_list(
    context: State<'_, AppContext>,
    args: GlossaryListArgs,
) -> Result<GlossaryPage, AppError> {
    let manager = context.manager.clone();
    tauri::async_runtime::spawn_blocking(move || {
        manager
            .lease(&args.project_id)?
            .with_connection(|db, _| preferences::glossary_page(db, &args))
    })
    .await
    .map_err(|_| AppError::invalid("task"))?
}
#[tauri::command]
pub async fn glossary_put(
    context: State<'_, AppContext>,
    args: GlossaryPutArgs,
) -> Result<Revision, AppError> {
    let manager = context.manager.clone();
    tauri::async_runtime::spawn_blocking(move || {
        manager
            .lease(&args.project_id)?
            .with_connection(|db, _| preferences::put_term(db, &args))
    })
    .await
    .map_err(|_| AppError::invalid("task"))?
}
#[tauri::command]
pub async fn glossary_delete(
    context: State<'_, AppContext>,
    args: GlossaryDeleteArgs,
) -> Result<(), AppError> {
    let manager = context.manager.clone();
    tauri::async_runtime::spawn_blocking(move || {
        manager
            .lease(&args.project_id)?
            .with_connection(|db, _| preferences::delete_term(db, &args))
    })
    .await
    .map_err(|_| AppError::invalid("task"))?
}
