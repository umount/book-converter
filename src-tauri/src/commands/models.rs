//! Global model installation is independent of project data and language choices.
use crate::{
    app::services::AppContext,
    models::{ModelArgs, ModelFailure, ModelView},
};
#[tauri::command]
pub async fn model_list(
    context: tauri::State<'_, AppContext>,
) -> Result<Vec<ModelView>, ModelFailure> {
    context.models.list().await
}
#[tauri::command]
pub async fn model_download(
    context: tauri::State<'_, AppContext>,
    args: ModelArgs,
) -> Result<(), ModelFailure> {
    context.models.start(&args.model_id).await
}
#[tauri::command]
pub fn model_pause(
    context: tauri::State<'_, AppContext>,
    args: ModelArgs,
) -> Result<(), ModelFailure> {
    context.models.pause(&args.model_id)
}
#[tauri::command]
pub async fn model_remove(
    context: tauri::State<'_, AppContext>,
    args: ModelArgs,
) -> Result<(), ModelFailure> {
    context.models.remove(&args.model_id).await
}
