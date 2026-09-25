//! Rebuild edited page artwork from the original without repeating API stages.
use super::{
    image_pipeline::{ImageOutput, ImagePipeline},
    local,
};
use crate::{
    app::contracts::AppError, jobs::durable::StepExecutor, project::lifecycle::ProjectLease,
    storage::runs,
};
use rusqlite::Transaction;
use std::{future::Future, pin::Pin};
pub struct RebuildPipeline {
    pub masks: ImagePipeline,
    pub cleanup: ImagePipeline,
    pub lettering: ImagePipeline,
}
impl RebuildPipeline {
    fn stage(
        &self,
        run: &runs::RunRecord,
        stage: &str,
    ) -> Result<(&ImagePipeline, runs::RunRecord), AppError> {
        let pipeline = match stage {
            "masks" => &self.masks,
            "inpainting" => &self.cleanup,
            "lettering" => &self.lettering,
            _ => return Err(AppError::invalid("mangaStage")),
        };
        let mut derived = run.clone();
        derived.kind = format!("manga_{stage}");
        derived.snapshot.stages = vec![stage.into()];
        derived.snapshot.prompt_version = format!("{}:{}", local::VERSION, pipeline.model_hash);
        Ok((pipeline, derived))
    }
}
impl StepExecutor for RebuildPipeline {
    type Output = (String, ImageOutput);
    fn fingerprint(
        &self,
        lease: &ProjectLease,
        run: &runs::RunRecord,
        entity: &str,
        stage: &str,
    ) -> Result<String, AppError> {
        let (p, r) = self.stage(run, stage)?;
        p.fingerprint(lease, &r, entity, stage)
    }
    fn compute<'a>(
        &'a self,
        lease: &'a ProjectLease,
        run: &'a runs::RunRecord,
        entity: &'a str,
        stage: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<Self::Output, AppError>> + Send + 'a>> {
        Box::pin(async move {
            let (p, r) = self.stage(run, stage)?;
            Ok((stage.into(), p.compute(lease, &r, entity, stage).await?))
        })
    }
    fn persist(&self, tx: &Transaction<'_>, output: Self::Output) -> Result<String, AppError> {
        match output.0.as_str() {
            "masks" => self.masks.persist(tx, output.1),
            "inpainting" => self.cleanup.persist(tx, output.1),
            "lettering" => self.lettering.persist(tx, output.1),
            _ => Err(AppError::invalid("mangaStage")),
        }
    }
    fn checkpoint_current(
        &self,
        lease: &ProjectLease,
        run: &runs::RunRecord,
        entity: &str,
        stage: &str,
        fingerprint: &str,
    ) -> Result<bool, AppError> {
        let (p, r) = self.stage(run, stage)?;
        p.checkpoint_current(lease, &r, entity, stage, fingerprint)
    }
    fn entity_kind(&self) -> &'static str {
        "page"
    }
}
