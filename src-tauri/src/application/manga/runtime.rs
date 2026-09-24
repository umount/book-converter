//! Explicit, bounded admission for recognition; this is not the full typesetting pipeline.
use super::{pipeline::RecognitionPipeline, recognition::PROMPT_VERSION};
use crate::{
    ai::{ChatCompletions, ProviderProfile},
    app::{
        contracts::{AppError, EntitySelection, JobRef, JobState, MangaStage, ProjectKind},
        requests::StartMangaStageArgs,
    },
    application::runtime::{provider_profile, validate_credential_destination},
    project::lifecycle::ProjectManager,
    storage::{
        repository::{storage_error, ProjectRepository},
        runs, shared,
    },
};
use rusqlite::Connection;
use std::sync::Arc;

pub fn prepare(manager: &ProjectManager, args: &StartMangaStageArgs) -> Result<JobRef, AppError> {
    let lease = manager.lease(&args.project_id)?;
    lease.with_connection(|db, _| {
        ProjectRepository::new(db, ProjectKind::Manga)?;
        if args.stage != MangaStage::Recognition {
            return Err(AppError::invalid("mangaStageUnavailable"));
        }
        let settings = shared::settings(db)?;
        // Never silently reuse the book model for image uploads.
        let selected = settings
            .choices
            .manga_recognition_profile
            .as_deref()
            .ok_or_else(|| AppError::invalid("mangaRecognitionProfile"))?;
        let (profile, key) = provider_profile(Some(selected))?;
        ChatCompletions::new(profile.clone(), key)?;
        let id = uuid::Uuid::new_v4().to_string();
        create(
            db,
            &id,
            &args.selection,
            args.options.max_pages,
            args.options.force,
            profile,
        )?;
        Ok(JobRef {
            project_id: args.project_id.clone(),
            job_id: id,
        })
    })
}
pub(crate) fn create(
    db: &mut Connection,
    id: &str,
    selection: &EntitySelection,
    max_pages: u32,
    force: bool,
    profile: ProviderProfile,
) -> Result<(), AppError> {
    ProjectRepository::new(db, ProjectKind::Manga)?;
    if max_pages == 0 {
        return Err(AppError::invalid("maxPages"));
    }
    let settings = shared::settings(db)?;
    let glossary = shared::glossary_revision(db)?;
    let rows = {
        let mut query=db.prepare("SELECT p.id,EXISTS(SELECT 1 FROM manga_results r WHERE r.page_id=p.id AND r.stage='recognition' AND r.validity='current' AND r.page_revision=p.revision AND r.settings_revision=?1 AND r.glossary_revision=?2) FROM manga_pages p JOIN manga_volumes v ON v.id=p.volume_id ORDER BY v.position,p.position").map_err(storage_error)?;
        let rows = query
            .query_map(
                rusqlite::params![settings.revision.value()?, glossary.value()?],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, bool>(1)?)),
            )
            .map_err(storage_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(storage_error)?;
        rows
    };
    let ordered = rows.iter().map(|(id, _)| id.clone()).collect::<Vec<_>>();
    let eligible = rows
        .into_iter()
        .filter(|(_, ready)| force || !ready)
        .map(|(id, _)| id)
        .collect::<std::collections::HashSet<_>>();
    let selected = selection
        .resolve(&ordered)?
        .into_iter()
        .filter(|id| eligible.contains(id))
        .take(max_pages as usize)
        .collect::<Vec<_>>();
    if selected.is_empty() {
        return Err(AppError::invalid("noEligiblePages"));
    }
    runs::create_run(
        db,
        id,
        "manga_recognition",
        &runs::RunSnapshot {
            retarget: None,
            settings: settings.choices,
            settings_revision: settings.revision,
            glossary_revision: glossary,
            selected_ids: selected,
            prompt_version: PROMPT_VERSION.into(),
            stages: vec!["recognition".into()],
            provider: Some(profile),
            instructions: None,
        },
        &std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
            .to_string(),
    )
}
pub fn resume_provider(
    manager: &ProjectManager,
    project: &crate::app::contracts::ProjectId,
    job: &str,
) -> Result<RecognitionPipeline, AppError> {
    let run = manager.lease(project)?.with_connection(|db, _| {
        ProjectRepository::new(db, ProjectKind::Manga)?;
        runs::get_run(db, job)
    })?;
    if run.kind != "manga_recognition"
        || !matches!(
            run.state,
            JobState::Queued | JobState::Interrupted | JobState::Failed | JobState::Cancelled
        )
    {
        return Err(AppError::invalid("jobState"));
    }
    let saved = run
        .snapshot
        .provider
        .ok_or_else(|| AppError::invalid("providerProfile"))?;
    let id = run
        .snapshot
        .settings
        .manga_recognition_profile
        .as_deref()
        .ok_or_else(|| AppError::invalid("mangaRecognitionProfile"))?;
    let (configured, key) = provider_profile(Some(id))?;
    validate_credential_destination(&saved, &configured)?;
    Ok(RecognitionPipeline {
        provider: Arc::new(ChatCompletions::new(saved, key)?),
    })
}
