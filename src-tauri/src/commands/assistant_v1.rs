use crate::{
    app::{
        contracts::{AppError, JobRef},
        requests::*,
        services::AppContext,
    },
    application::runtime::{provider_profile, Reservation},
};
use tauri::State;
#[tauri::command]
pub async fn assistant_project_view(
    context: State<'_, AppContext>,
    args: ProjectArgs,
) -> Result<AssistantView, AppError> {
    context.assistant.view(&context.manager, &args.project_id)
}
#[tauri::command]
pub async fn assistant_project_send(
    context: State<'_, AppContext>,
    args: AssistantSendArgs,
) -> Result<AssistantView, AppError> {
    let token = context
        .assistant_runs
        .reserve(&args.project_id, "assistant")?;
    let _reservation = Reservation {
        runtime: context.assistant_runs.clone(),
        project: args.project_id.clone(),
        job: "assistant".into(),
    };
    let profile = context
        .manager
        .lease(&args.project_id)?
        .with_connection(|db, _| {
            Ok(crate::storage::shared::settings(db)?
                .choices
                .assistant_profile)
        })?;
    let (profile, key) = provider_profile(profile.as_deref())?;
    let provider = crate::ai::ChatCompletions::new(profile, key)?;
    context
        .assistant
        .send(&context.manager, &args, &provider, token)
        .await
}
#[tauri::command]
pub async fn assistant_project_confirm(
    context: State<'_, AppContext>,
    app: tauri::AppHandle,
    args: AssistantConfirmArgs,
) -> Result<Option<JobRef>, AppError> {
    let _token = context
        .assistant_runs
        .reserve(&args.project_id, "assistant")?;
    let _reservation = Reservation {
        runtime: context.assistant_runs.clone(),
        project: args.project_id.clone(),
        job: "assistant".into(),
    };
    let job = context.assistant.confirm(&context.manager, &args)?;
    job.map(|job| super::book_v1::dispatch_created(&context, app, job))
        .transpose()
}
#[tauri::command]
pub async fn assistant_project_cancel(
    context: State<'_, AppContext>,
    args: ProjectArgs,
) -> Result<(), AppError> {
    context.assistant_runs.cancel(&args.project_id, "assistant");
    Ok(())
}
