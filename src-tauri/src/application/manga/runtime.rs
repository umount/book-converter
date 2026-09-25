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
        if !matches!(
            args.stage.clone(),
            MangaStage::Recognition | MangaStage::Translation
        ) {
            return Err(AppError::invalid("mangaStageUnavailable"));
        }
        let settings = shared::settings(db)?;
        let (profile, _) = if args.stage == MangaStage::Translation {
            super::preflight::translation_provider(&settings.choices)?
        } else {
            super::preflight::recognition_provider(&settings.choices)?
        };
        let id = uuid::Uuid::new_v4().to_string();
        create_stage(
            db,
            &id,
            &args.selection,
            args.options.max_pages,
            args.options.force,
            profile,
            args.stage.clone(),
        )?;
        Ok(JobRef {
            project_id: args.project_id.clone(),
            job_id: id,
        })
    })
}
#[cfg(test)]
pub(crate) fn create(
    db: &mut Connection,
    id: &str,
    selection: &EntitySelection,
    max_pages: u32,
    force: bool,
    profile: ProviderProfile,
) -> Result<(), AppError> {
    create_stage(
        db,
        id,
        selection,
        max_pages,
        force,
        profile,
        MangaStage::Recognition,
    )
}
#[allow(clippy::too_many_arguments)]
pub(crate) fn create_stage(
    db: &mut Connection,
    id: &str,
    selection: &EntitySelection,
    max_pages: u32,
    force: bool,
    profile: ProviderProfile,
    stage: MangaStage,
) -> Result<(), AppError> {
    let (stage_name, kind, prompt) = match stage {
        MangaStage::Recognition => ("recognition", "manga_recognition", PROMPT_VERSION),
        MangaStage::Translation => (
            "translation",
            "manga_translation",
            super::translation::PROMPT_VERSION,
        ),
        _ => return Err(AppError::invalid("mangaStageUnavailable")),
    };
    ProjectRepository::new(db, ProjectKind::Manga)?;
    if max_pages == 0 {
        return Err(AppError::invalid("maxPages"));
    }
    let settings = shared::settings(db)?;
    let glossary = shared::glossary_revision(db)?;
    let rows = {
        let mut query=db.prepare("SELECT p.id,EXISTS(SELECT 1 FROM manga_results r WHERE r.page_id=p.id AND r.stage=?3 AND r.validity='current' AND r.page_revision=p.revision AND r.settings_revision=?1 AND r.glossary_revision=?2) FROM manga_pages p JOIN manga_volumes v ON v.id=p.volume_id WHERE (?3='recognition' OR EXISTS(SELECT 1 FROM manga_results recognized WHERE recognized.page_id=p.id AND recognized.stage='recognition')) ORDER BY v.position,p.position").map_err(storage_error)?;
        let rows = query
            .query_map(
                rusqlite::params![settings.revision.value()?, glossary.value()?, stage_name],
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
        kind,
        &runs::RunSnapshot {
            manga: None,
            retarget: None,
            settings: settings.choices,
            settings_revision: settings.revision,
            glossary_revision: glossary,
            selected_ids: selected,
            prompt_version: prompt.into(),
            stages: vec![stage_name.into()],
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
    Ok(RecognitionPipeline {
        provider: resume_client(manager, project, job, false)?,
    })
}
pub fn resume_translation(
    manager: &ProjectManager,
    project: &crate::app::contracts::ProjectId,
    job: &str,
) -> Result<super::translation_pipeline::TranslationPipeline, AppError> {
    Ok(super::translation_pipeline::TranslationPipeline {
        provider: resume_client(manager, project, job, true)?,
    })
}
fn resume_client(
    manager: &ProjectManager,
    project: &crate::app::contracts::ProjectId,
    job: &str,
    translation: bool,
) -> Result<Arc<dyn crate::ai::Provider>, AppError> {
    let run = manager.lease(project)?.with_connection(|db, _| {
        ProjectRepository::new(db, ProjectKind::Manga)?;
        runs::get_run(db, job)
    })?;
    if run.kind
        != if translation {
            "manga_translation"
        } else {
            "manga_recognition"
        }
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
    let id = if translation {
        run.snapshot.settings.manga_translation_profile.as_deref()
    } else {
        run.snapshot.settings.manga_recognition_profile.as_deref()
    }
    .ok_or_else(|| AppError::invalid("mangaProfile"))?;
    let (configured, key) = provider_profile(Some(id))?;
    validate_credential_destination(&saved, &configured)?;
    Ok(Arc::new(ChatCompletions::new(saved, key)?))
}

/// Credentials are resolved at execution time; snapshots never contain secrets.
pub(crate) fn saved_client(
    saved: ProviderProfile,
    id: &str,
) -> Result<Arc<dyn crate::ai::Provider>, AppError> {
    let (configured, key) = provider_profile(Some(id))?;
    validate_credential_destination(&saved, &configured)?;
    Ok(Arc::new(ChatCompletions::new(saved, key)?))
}
