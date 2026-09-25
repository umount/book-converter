//! A page completes every stage before the next page starts. Checkpoints reuse the
//! existing stage executors; both API roles and native versions are frozen together.
use super::{
    image_pipeline::{ImageOutput, ImagePipeline},
    local,
    pipeline::{RecognitionOutput, RecognitionPipeline},
    translation_pipeline::{TranslationOutput, TranslationPipeline},
};
use crate::{
    app::contracts::{AppError, EntitySelection, ProjectKind},
    jobs::durable::StepExecutor,
    project::lifecycle::ProjectLease,
    storage::{
        repository::{storage_error, ProjectRepository},
        runs, shared,
    },
};
use rusqlite::{Connection, Transaction};
use std::{future::Future, pin::Pin};
pub const VERSION: &str = "manga-automatic-v1";
pub const STAGES: [&str; 5] = [
    "recognition",
    "translation",
    "masks",
    "inpainting",
    "lettering",
];

pub struct AutomaticPipeline {
    pub recognition: RecognitionPipeline,
    pub translation: TranslationPipeline,
    pub masks: ImagePipeline,
    pub cleanup: ImagePipeline,
    pub lettering: ImagePipeline,
}
pub enum Output {
    Recognition(RecognitionOutput),
    Translation(TranslationOutput),
    Image(String, ImageOutput),
}
impl AutomaticPipeline {
    fn stage_run(&self, run: &runs::RunRecord, stage: &str) -> Result<runs::RunRecord, AppError> {
        let plan = run
            .snapshot
            .manga
            .as_ref()
            .ok_or_else(|| AppError::invalid("mangaPlan"))?;
        if self.recognition.provider.profile() != &plan.recognition
            || self.translation.provider.profile() != &plan.translation
            || run.kind != "manga_automatic"
            || run.snapshot.prompt_version != VERSION
            || run.snapshot.stages != STAGES
            || plan.mask_hash != self.masks.model_hash
            || plan.cleanup_hash != self.cleanup.model_hash
            || plan.lettering_hash != self.lettering.model_hash
        {
            return Err(AppError::invalid("mangaPlan"));
        }
        let mut derived = run.clone();
        derived.kind = format!("manga_{stage}");
        derived.snapshot.stages = vec![stage.into()];
        let (prompt, provider) = match stage {
            "recognition" => (
                super::recognition::PROMPT_VERSION.to_owned(),
                Some(plan.recognition.clone()),
            ),
            "translation" => (
                super::translation::PROMPT_VERSION.to_owned(),
                Some(plan.translation.clone()),
            ),
            "masks" => (format!("{}:{}", local::VERSION, plan.mask_hash), None),
            "inpainting" => (format!("{}:{}", local::VERSION, plan.cleanup_hash), None),
            "lettering" => (format!("{}:{}", local::VERSION, plan.lettering_hash), None),
            _ => return Err(AppError::invalid("mangaStage")),
        };
        derived.snapshot.prompt_version = prompt;
        derived.snapshot.provider = provider;
        Ok(derived)
    }
    fn image(&self, stage: &str) -> Result<&ImagePipeline, AppError> {
        match stage {
            "masks" => Ok(&self.masks),
            "inpainting" => Ok(&self.cleanup),
            "lettering" => Ok(&self.lettering),
            _ => Err(AppError::invalid("mangaStage")),
        }
    }
}
impl StepExecutor for AutomaticPipeline {
    type Output = Output;
    fn checkpoint_current(
        &self,
        lease: &ProjectLease,
        _run: &runs::RunRecord,
        entity: &str,
        stage: &str,
        fingerprint: &str,
    ) -> Result<bool, AppError> {
        lease.with_connection(|db,_|db.query_row("SELECT EXISTS(SELECT 1 FROM manga_results r JOIN manga_pages p ON p.id=r.page_id JOIN project_settings s ON s.singleton=1 JOIN glossary_state g ON g.singleton=1 WHERE r.page_id=?1 AND r.stage=?2 AND r.input_fingerprint=?3 AND r.validity='current' AND r.page_revision=p.revision AND r.settings_revision=s.revision AND r.glossary_revision=g.revision)",rusqlite::params![entity,stage,fingerprint],|r|r.get(0)).map_err(storage_error))
    }
    fn entity_kind(&self) -> &'static str {
        "page"
    }
    fn fingerprint(
        &self,
        lease: &ProjectLease,
        run: &runs::RunRecord,
        entity: &str,
        stage: &str,
    ) -> Result<String, AppError> {
        let derived = self.stage_run(run, stage)?;
        match stage {
            "recognition" => self.recognition.fingerprint(lease, &derived, entity, stage),
            "translation" => self.translation.fingerprint(lease, &derived, entity, stage),
            _ => self
                .image(stage)?
                .fingerprint(lease, &derived, entity, stage),
        }
    }
    fn compute<'a>(
        &'a self,
        lease: &'a ProjectLease,
        run: &'a runs::RunRecord,
        entity: &'a str,
        stage: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<Output, AppError>> + Send + 'a>> {
        Box::pin(async move {
            let derived = self.stage_run(run, stage)?;
            match stage {
                "recognition" => Ok(Output::Recognition(
                    self.recognition
                        .compute(lease, &derived, entity, stage)
                        .await?,
                )),
                "translation" => Ok(Output::Translation(
                    self.translation
                        .compute(lease, &derived, entity, stage)
                        .await?,
                )),
                _ => Ok(Output::Image(
                    stage.into(),
                    self.image(stage)?
                        .compute(lease, &derived, entity, stage)
                        .await?,
                )),
            }
        })
    }
    fn persist(&self, tx: &Transaction<'_>, output: Output) -> Result<String, AppError> {
        match output {
            Output::Recognition(value) => self.recognition.persist(tx, value),
            Output::Translation(value) => self.translation.persist(tx, value),
            Output::Image(stage, value) => self.image(&stage)?.persist(tx, value),
        }
    }
}

pub fn create(
    db: &mut Connection,
    id: &str,
    selection: &EntitySelection,
    max_pages: u32,
    force: bool,
    plan: runs::MangaPlan,
) -> Result<(), AppError> {
    ProjectRepository::new(db, ProjectKind::Manga)?;
    if max_pages == 0 {
        return Err(AppError::invalid("maxPages"));
    }
    let settings = shared::settings(db)?;
    let glossary = shared::glossary_revision(db)?;
    let rows = {
        let mut query=db.prepare("SELECT p.id,EXISTS(SELECT 1 FROM manga_results r WHERE r.page_id=p.id AND r.stage='lettering' AND r.validity='current' AND r.page_revision=p.revision AND r.settings_revision=?1 AND r.glossary_revision=?2) FROM manga_pages p JOIN manga_volumes v ON v.id=p.volume_id ORDER BY v.position,p.position").map_err(storage_error)?;
        let result = query
            .query_map(
                rusqlite::params![settings.revision.value()?, glossary.value()?],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, bool>(1)?)),
            )
            .map_err(storage_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(storage_error)?;
        result
    };
    let ordered = rows.iter().map(|r| r.0.clone()).collect::<Vec<_>>();
    let eligible = rows
        .into_iter()
        .filter(|r| force || !r.1)
        .map(|r| r.0)
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
        "manga_automatic",
        &runs::RunSnapshot {
            manga: Some(plan),
            retarget: None,
            settings: settings.choices,
            settings_revision: settings.revision,
            glossary_revision: glossary,
            selected_ids: selected,
            prompt_version: VERSION.into(),
            stages: STAGES.iter().map(|s| (*s).into()).collect(),
            provider: None,
            instructions: None,
        },
        &std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
            .to_string(),
    )
}
