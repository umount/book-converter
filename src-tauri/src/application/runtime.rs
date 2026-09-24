//! Book job admission and cancellation. The runner owns a project lease until completion.
use crate::{
    ai::{ChatCompletions, ProviderProfile},
    app::{
        contracts::{AppError, EntitySelection, JobRef, JobState, ProjectId},
        requests::TranslationOptions,
    },
    project::lifecycle::ProjectManager,
    storage::{
        repository::{storage_error, ProjectRepository},
        runs, shared,
    },
};
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
};
#[derive(Default)]
pub struct BookRuntime {
    active: Mutex<HashMap<(String, String), Arc<AtomicBool>>>,
}
impl BookRuntime {
    pub fn cancel(&self, project: &ProjectId, job: &str) -> bool {
        let map = self.active.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(token) = map.get(&(project.as_str().into(), job.into())) {
            token.store(true, Ordering::Release);
            true
        } else {
            false
        }
    }
    pub fn reserve(&self, project: &ProjectId, job: &str) -> Result<Arc<AtomicBool>, AppError> {
        let mut map = self.active.lock().unwrap_or_else(|p| p.into_inner());
        let key = (project.as_str().into(), job.into());
        if map.contains_key(&key) {
            return Err(AppError::invalid("jobRunning"));
        }
        let token = Arc::new(AtomicBool::new(false));
        map.insert(key, token.clone());
        Ok(token)
    }
    pub fn release(&self, project: &ProjectId, job: &str) {
        self.active
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(&(project.as_str().into(), job.into()));
    }
}

pub fn provider_profile(selected: Option<&str>) -> Result<(ProviderProfile, String), AppError> {
    let config = crate::config::Config::load();
    let profile = if let Some(id) = selected {
        let json = crate::settings::get(&crate::settings::db_path(), &format!("ai_profile:{id}"))
            .map_err(|_| AppError::invalid("providerProfile"))?
            .ok_or_else(|| AppError::invalid("providerProfile"))?;
        let profile: ProviderProfile =
            serde_json::from_str(&json).map_err(|_| AppError::invalid("providerProfile"))?;
        if profile.id != id {
            return Err(AppError::invalid("providerProfile"));
        }
        profile
    } else {
        ProviderProfile {
            id: "default-book".into(),
            base_url: config.base_url,
            model: config.model,
            temperature: config.temperature,
            max_output_tokens: config.max_output_tokens,
            timeout_seconds: config.request_timeout_secs,
            network_retries: u32::try_from(config.max_retries.min(5)).unwrap_or(5),
        }
    };
    let credential = match selected {
        Some(id) => {
            crate::settings::get(&crate::settings::db_path(), &format!("ai_credential:{id}"))
                .map_err(|_| AppError::invalid("providerCredential"))?
                .unwrap_or_default()
        }
        None => config.api_key,
    };
    Ok((profile, credential))
}

pub fn prepare_book_run(
    manager: &ProjectManager,
    project: &ProjectId,
    selection: &EntitySelection,
    options: &TranslationOptions,
) -> Result<JobRef, AppError> {
    let lease = manager.lease(project)?;
    let snapshot=lease.with_connection(|db,_|{
        ProjectRepository::new(db,crate::app::contracts::ProjectKind::Book)?;
        let settings=shared::settings(db)?;
        let ordered={let mut query=db.prepare("SELECT id FROM book_chapters ORDER BY position").map_err(storage_error)?;let rows=query.query_map([],|r|r.get::<_,String>(0)).map_err(storage_error)?;rows.collect::<Result<Vec<_>,_>>().map_err(storage_error)?};
        let mut selected=selection.resolve(&ordered)?;
        let mut text_chapters=Vec::new();
        for id in selected {if db.query_row("SELECT EXISTS(SELECT 1 FROM book_source_blocks WHERE chapter_id=?1 AND kind IN ('text','caption') AND length(trim(text))>0)",[&id],|r|r.get::<_,bool>(0)).map_err(storage_error)?{text_chapters.push(id);}}
        selected=text_chapters;
        if !options.force {let mut eligible=Vec::new();for id in selected{let ready=db.query_row("SELECT EXISTS(SELECT 1 FROM book_translations WHERE chapter_id=?1 AND status='ready' AND target_language=?2 AND source_revision=(SELECT revision FROM book_chapters WHERE id=?1))",rusqlite::params![id,settings.choices.target_language],|r|r.get::<_,bool>(0)).map_err(storage_error)?;if !ready{eligible.push(id);}}selected=eligible;}
        let (profile,key)=provider_profile(settings.choices.book_translation_profile.as_deref())?;
        ChatCompletions::new(profile.clone(),key)?;
        Ok(runs::RunSnapshot{settings:settings.choices,settings_revision:settings.revision,glossary_revision:shared::glossary_revision(db)?,selected_ids:selected,prompt_version:"book-segments-v1".into(),stages:vec!["translation".into(),"context".into()],provider:Some(profile),instructions:options.instructions.clone()})
    })?;
    let job_id = uuid::Uuid::new_v4().to_string();
    lease.with_connection(|db, _| {
        runs::create_run(db, &job_id, "book_translation", &snapshot, &now())
    })?;
    Ok(JobRef {
        project_id: project.clone(),
        job_id,
    })
}

pub fn resume_provider(
    manager: &ProjectManager,
    project: &ProjectId,
    job: &str,
) -> Result<super::book::BookPipeline, AppError> {
    let run = manager
        .lease(project)?
        .with_connection(|db, _| runs::get_run(db, job))?;
    if run.kind != "book_translation"
        || !matches!(
            run.state,
            JobState::Queued | JobState::Interrupted | JobState::Failed | JobState::Cancelled
        )
    {
        return Err(AppError::invalid("jobState"));
    }
    let profile = run
        .snapshot
        .provider
        .ok_or_else(|| AppError::invalid("providerProfile"))?;
    // A project archive cannot choose a new destination for a locally stored credential.
    // Keep the saved model/options, but require the endpoint to remain locally configured.
    let (configured, key) =
        provider_profile(run.snapshot.settings.book_translation_profile.as_deref())?;
    validate_credential_destination(&profile, &configured)?;
    let provider = Arc::new(ChatCompletions::new(profile, key)?);
    Ok(super::book::BookPipeline {
        provider,
        instructions: run.snapshot.instructions,
    })
}
fn now() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .to_string()
}

/// Run once during application setup, before accepting job commands.
pub fn recover_interrupted(manager: &ProjectManager) -> Result<usize, AppError> {
    let mut recovered = 0;
    for project in manager.catalog()? {
        recovered += manager
            .lease(&project.descriptor.id)?
            .with_connection(|db, _| runs::interrupt_running(db, &now()))?;
    }
    Ok(recovered)
}

/// Release admission even if a spawned task is aborted or unwinds.
pub(crate) struct Reservation {
    pub runtime: Arc<BookRuntime>,
    pub project: ProjectId,
    pub job: String,
}
impl Drop for Reservation {
    fn drop(&mut self) {
        self.runtime.release(&self.project, &self.job);
    }
}

fn validate_credential_destination(
    saved: &ProviderProfile,
    configured: &ProviderProfile,
) -> Result<(), AppError> {
    if saved.id != configured.id
        || saved.base_url.trim_end_matches('/') != configured.base_url.trim_end_matches('/')
    {
        return Err(AppError::invalid("providerEndpointChanged"));
    }
    Ok(())
}

#[cfg(test)]
mod credential_tests {
    use super::*;
    #[test]
    fn imported_snapshot_cannot_redirect_a_local_credential() {
        let configured = ProviderProfile {
            id: "book".into(),
            base_url: "https://provider.example/v1".into(),
            model: "model".into(),
            temperature: 0.5,
            max_output_tokens: 100,
            timeout_seconds: 10,
            network_retries: 0,
        };
        let mut saved = configured.clone();
        saved.model = "previous-model".into();
        assert!(validate_credential_destination(&saved, &configured).is_ok());
        saved.base_url = "https://different.example/v1".into();
        assert!(validate_credential_destination(&saved, &configured).is_err());
        saved = configured.clone();
        saved.id = "other-profile".into();
        assert!(validate_credential_destination(&saved, &configured).is_err());
    }
}
