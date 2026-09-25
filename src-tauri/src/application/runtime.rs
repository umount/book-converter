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
    if options.max_chapters == 0 {
        return Err(AppError::invalid("maxChapters"));
    }
    let lease = manager.lease(project)?;
    let snapshot = lease.with_connection(|db, _| {
        ProjectRepository::new(db, crate::app::contracts::ProjectKind::Book)?;
        let settings = shared::settings(db)?;
        let selected = select_batch(db, selection, options, &settings.choices.target_language)?;
        let (profile, key) =
            provider_profile(settings.choices.book_translation_profile.as_deref())?;
        ChatCompletions::new(profile.clone(), key)?;
        Ok(runs::RunSnapshot { manga: None,
            retarget: None,
            settings: settings.choices,
            settings_revision: settings.revision,
            glossary_revision: shared::glossary_revision(db)?,
            selected_ids: selected,
            prompt_version: "book-segments-v1".into(),
            stages: if options.extract_glossary {
                vec!["glossary".into(), "translation".into(), "context".into()]
            } else {
                vec!["translation".into(), "context".into()]
            },
            provider: Some(profile),
            instructions: Some(
                [
                    super::book_presentation::read(db)?.instructions,
                    options.instructions.clone().unwrap_or_default(),
                ]
                .into_iter()
                .filter(|s| !s.trim().is_empty())
                .collect::<Vec<_>>()
                .join("\n\n"),
            ),
        })
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

/// Resolve source order first, then cap eligible work, so skipped chapters do not consume the batch.
pub(crate) fn select_batch(
    db: &rusqlite::Connection,
    selection: &EntitySelection,
    options: &TranslationOptions,
    target: &str,
) -> Result<Vec<String>, AppError> {
    if options.max_chapters == 0 {
        return Err(AppError::invalid("maxChapters"));
    }
    let mut query = db
        .prepare(
            "SELECT c.id,
        EXISTS(SELECT 1 FROM book_source_blocks b WHERE b.chapter_id=c.id
            AND b.kind IN ('text','caption') AND length(trim(b.text))>0),
        EXISTS(SELECT 1 FROM book_translations t WHERE t.chapter_id=c.id
            AND t.status IN ('ready','needs_review') AND t.target_language=?1 AND (t.provenance='reference' OR t.source_revision=c.revision))
        FROM book_chapters c ORDER BY c.position",
        )
        .map_err(storage_error)?;
    let rows = query
        .query_map([target], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, bool>(1)?,
                r.get::<_, bool>(2)?,
            ))
        })
        .map_err(storage_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(storage_error)?;
    let ordered = rows.iter().map(|r| r.0.clone()).collect::<Vec<_>>();
    let eligible = rows
        .into_iter()
        .filter(|r| r.1 && (options.force || !r.2))
        .map(|r| r.0)
        .collect::<std::collections::HashSet<_>>();
    let selected = selection
        .resolve(&ordered)?
        .into_iter()
        .filter(|id| eligible.contains(id))
        .take(options.max_chapters as usize)
        .collect::<Vec<_>>();
    if selected.is_empty() {
        return Err(AppError::invalid("noEligibleChapters"));
    }
    Ok(selected)
}

pub fn resume_provider(
    manager: &ProjectManager,
    project: &ProjectId,
    job: &str,
) -> Result<super::book::BookPipeline, AppError> {
    let run = manager
        .lease(project)?
        .with_connection(|db, _| runs::get_run(db, job))?;
    if !["book_translation", "book_metadata", "book_glossary", "book_title", "book_retarget"].contains(&run.kind.as_str())
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

pub(crate) fn validate_credential_destination(
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

pub fn prepare_metadata_run(
    manager: &ProjectManager,
    project: &ProjectId,
    summary_only: bool,
) -> Result<JobRef, AppError> {
    let descriptor = manager.open(project)?;
    let lease = manager.lease(project)?;
    let missing = lease.with_connection(|db,_| db.query_row("SELECT COUNT(*)=0 FROM book_source_metadata",[],|r|r.get::<_,bool>(0)).map_err(storage_error))?;
    if missing {
        let original = descriptor.source.original_path.as_ref().and_then(|p| crate::book::load_book(std::path::Path::new(p)).ok()).map(|b| b.meta);
        lease.with_connection(|db,_| {
            let title = original.as_ref().and_then(|m|m.title.clone()).unwrap_or(descriptor.name.clone());
            db.execute("INSERT OR IGNORE INTO book_source_metadata(singleton,title,author,summary) VALUES(1,?1,?2,?3)",rusqlite::params![title,original.as_ref().and_then(|m|m.author.as_ref()),original.as_ref().and_then(|m|m.summary.as_ref())]).map_err(storage_error)?;
            Ok(())
        })?;
    }
    lease.with_connection(|db, _| {
        ProjectRepository::new(db, crate::app::contracts::ProjectKind::Book)?;
        db.execute("UPDATE book_source_metadata SET title=?1 WHERE singleton=1 AND (title IS NULL OR trim(title)='')", [&descriptor.name]).map_err(storage_error)?;
        let settings = shared::settings(db)?;
        let (profile, key) =
            provider_profile(settings.choices.book_translation_profile.as_deref())?;
        ChatCompletions::new(profile.clone(), key)?;
        let first: String = db
            .query_row(
                "SELECT id FROM book_chapters ORDER BY position LIMIT 1",
                [],
                |r| r.get(0),
            )
            .map_err(storage_error)?;
        let snapshot = runs::RunSnapshot { manga: None,
            retarget: None,
            settings: settings.choices,
            settings_revision: settings.revision,
            glossary_revision: shared::glossary_revision(db)?,
            selected_ids: vec![first],
            prompt_version: "book-metadata-v2".into(),
            stages: vec!["metadata".into()],
            provider: Some(profile),
            instructions: summary_only.then(|| "summary_only".into()),
        };
        let job_id = uuid::Uuid::new_v4().to_string();
        runs::create_run(db, &job_id, "book_metadata", &snapshot, &now())?;
        Ok(JobRef {
            project_id: project.clone(),
            job_id,
        })
    })
}

pub fn prepare_glossary_run(
    manager: &ProjectManager,
    project: &ProjectId,
    selection: &EntitySelection,
    max_chapters: u32,
    force: bool,
) -> Result<JobRef, AppError> {
    let lease = manager.lease(project)?;
    lease.with_connection(|db,_|{
        ProjectRepository::new(db,crate::app::contracts::ProjectKind::Book)?;
        let selected = select_glossary_batch(db, selection, max_chapters, force)?;
        let settings=shared::settings(db)?;
        let (profile,key)=provider_profile(settings.choices.book_translation_profile.as_deref())?;
        ChatCompletions::new(profile.clone(),key)?;
        let snapshot=runs::RunSnapshot{manga: None,
            retarget:None,settings:settings.choices,settings_revision:settings.revision,glossary_revision:shared::glossary_revision(db)?,selected_ids:selected,prompt_version:"book-glossary-v1".into(),stages:vec!["glossary".into()],provider:Some(profile),instructions:None};
        let job_id=uuid::Uuid::new_v4().to_string();
        runs::create_run(db,&job_id,"book_glossary",&snapshot,&now())?;
        Ok(JobRef{project_id:project.clone(),job_id})
    })
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

#[cfg(test)]
mod batch_tests {
    use super::*;
    #[test]
    fn batches_skip_ready_and_empty_chapters_before_limiting() {
        let directory = std::env::temp_dir().join(format!("batch-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        let db = crate::storage::create(
            &directory.join("project.db"),
            crate::app::contracts::ProjectKind::Book,
            "ru",
        )
        .unwrap();
        for i in 0..6 {
            let id = format!("c{i}");
            db.execute(
                "INSERT INTO book_chapters(id,position,source_title) VALUES(?1,?2,'Chapter')",
                rusqlite::params![id, i],
            )
            .unwrap();
            if i != 1 {
                db.execute("INSERT INTO book_source_blocks(id,chapter_id,position,kind,text) VALUES(?1,?1,0,'text','Text')", [&id]).unwrap();
            }
        }
        db.execute("INSERT INTO book_translations(id,chapter_id,source_revision,status,provenance,target_language,translated_title,context_fingerprint,glossary_revision,revision) VALUES('t','c0',0,'ready','manual','ru','','',0,0)", []).unwrap();
        let mut options = TranslationOptions {
            max_chapters: 2,
            extract_glossary: true,
            force: false,
            instructions: None,
        };
        assert_eq!(
            select_batch(&db, &EntitySelection::All, &options, "ru").unwrap(),
            ["c2", "c3"]
        );
        db.execute("UPDATE book_translations SET status='needs_review'", [])
            .unwrap();
        assert_eq!(
            select_batch(&db, &EntitySelection::All, &options, "ru").unwrap(),
            ["c2", "c3"]
        );
        options.force = true;
        assert_eq!(
            select_batch(&db, &EntitySelection::All, &options, "ru").unwrap(),
            ["c0", "c2"]
        );
        options.max_chapters = 0;
        assert!(select_batch(&db, &EntitySelection::All, &options, "ru").is_err());
        options.max_chapters = 10;
        options.force = false;
        assert!(select_batch(
            &db,
            &EntitySelection::ExplicitIds {
                ids: vec!["c0".into(), "c1".into()]
            },
            &options,
            "ru"
        )
        .is_err());
        drop(db);
        std::fs::remove_dir_all(directory).unwrap();
    }
}

pub fn prepare_title_run(manager:&ProjectManager,args:&crate::app::requests::StartBookTitleArgs)->Result<JobRef,AppError> {
    let lease=manager.lease(&args.project_id)?;
    lease.with_connection(|db,_|{
        let view=ProjectRepository::new(db,crate::app::contracts::ProjectKind::Book)?.chapter(&args.chapter_id.0)?;
        let translation=view.translation.ok_or_else(||AppError::invalid("noTranslation"))?;
        if translation.revision!=args.expected_revision {return Err(crate::storage::repository::conflict())}
        let settings=shared::settings(db)?;
        let (profile,key)=provider_profile(settings.choices.book_translation_profile.as_deref())?;
        ChatCompletions::new(profile.clone(),key)?;
        let snapshot=runs::RunSnapshot { manga: None,
            retarget: None,
            settings:settings.choices,settings_revision:settings.revision,glossary_revision:shared::glossary_revision(db)?,selected_ids:vec![args.chapter_id.0.clone()],prompt_version:"book-title-v1".into(),stages:vec!["title".into()],provider:Some(profile),instructions:Some(super::book_presentation::read(db)?.instructions),
        };
        let job_id=uuid::Uuid::new_v4().to_string();
        runs::create_run(db,&job_id,"book_title",&snapshot,&now())?;
        Ok(JobRef {project_id:args.project_id.clone(),job_id})
    })
}

/// Select eligible chapters before applying the cap; prior results remain historical.
pub(crate) fn select_glossary_batch(db: &rusqlite::Connection, selection: &EntitySelection, max_chapters: u32, force: bool) -> Result<Vec<String>, AppError> {
    if max_chapters == 0 { return Err(AppError::invalid("maxChapters")); }
    let settings = shared::settings(db)?;
    let mut query = db.prepare("SELECT c.id, EXISTS(SELECT 1 FROM book_source_blocks b WHERE b.chapter_id=c.id AND b.kind IN ('text','caption') AND length(trim(b.text))>0), EXISTS(SELECT 1 FROM book_glossary_results g WHERE g.chapter_id=c.id AND g.source_revision=c.revision AND g.settings_revision=?1) FROM book_chapters c ORDER BY c.position").map_err(storage_error)?;
    let rows = query.query_map([settings.revision.value()?], |r| Ok((r.get::<_,String>(0)?,r.get::<_,bool>(1)?,r.get::<_,bool>(2)?))).map_err(storage_error)?.collect::<Result<Vec<_>,_>>().map_err(storage_error)?;
    let ordered = rows.iter().map(|r|r.0.clone()).collect::<Vec<_>>();
    let eligible = rows.into_iter().filter(|r|r.1 && (force || !r.2)).map(|r|r.0).collect::<std::collections::HashSet<_>>();
    let selected = selection.resolve(&ordered)?.into_iter().filter(|id|eligible.contains(id)).take(max_chapters as usize).collect::<Vec<_>>();
    if selected.is_empty() { return Err(AppError::invalid("noEligibleChapters")); }
    Ok(selected)
}

#[cfg(test)]
mod glossary_batch_tests {
    use super::*;
    #[test]
    fn extraction_batches_skip_current_results_and_count_only_text_chapters() {
        let db = crate::storage::tests::database(crate::app::contracts::ProjectKind::Book);
        for (position,id) in ["done","image","next","last"].iter().enumerate() {
            db.execute("INSERT INTO book_chapters(id,position,source_title) VALUES(?1,?2,?1)",rusqlite::params![id,position]).unwrap();
            if *id != "image" { db.execute("INSERT INTO book_source_blocks(id,chapter_id,position,kind,text) VALUES(?1,?1,0,'text','Source')",[id]).unwrap(); }
        }
        db.execute("INSERT INTO book_glossary_results(id,chapter_id,source_revision,settings_revision,terms_json) VALUES('result','done',0,0,'[]')",[]).unwrap();
        assert_eq!(select_glossary_batch(&db,&EntitySelection::All,1,false).unwrap(),vec!["next"]);
        assert_eq!(select_glossary_batch(&db,&EntitySelection::All,2,false).unwrap(),vec!["next","last"]);
        assert_eq!(select_glossary_batch(&db,&EntitySelection::All,1,true).unwrap(),vec!["done"]);
        assert!(select_glossary_batch(&db,&EntitySelection::All,0,false).is_err());
        assert!(select_glossary_batch(&db,&EntitySelection::ExplicitIds{ids:vec!["done".into(),"image".into()]},2,false).is_err());
        assert!(select_glossary_batch(&db,&EntitySelection::ExplicitIds{ids:vec!["missing".into()]},2,false).is_err());
        db.execute("UPDATE book_chapters SET revision=1 WHERE id='done'",[]).unwrap();
        assert_eq!(select_glossary_batch(&db,&EntitySelection::All,1,false).unwrap(),vec!["done"]);
        db.execute("UPDATE book_chapters SET revision=0 WHERE id='done'",[]).unwrap();
        db.execute("UPDATE project_settings SET revision=1",[]).unwrap();
        assert_eq!(select_glossary_batch(&db,&EntitySelection::All,1,false).unwrap(),vec!["done"]);
    }
}
