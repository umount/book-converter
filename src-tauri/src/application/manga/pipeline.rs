//! Checkpointed recognition publishes geometry and text together, never partial output.
use super::{
    recognition::{self, RecognitionInput, RecognizedRegion},
    regions,
};
use crate::{
    ai::Provider,
    app::contracts::{AppError, MangaStage, ProjectKind, Revision},
    assets::store::AssetStore,
    jobs::durable::StepExecutor,
    project::lifecycle::ProjectLease,
    storage::{
        repository::{conflict, storage_error, ProjectRepository},
        results::{self, InputVersions, MangaOutput, MangaResult},
        runs, shared,
    },
};
use rusqlite::{params, Connection, Transaction};
use sha2::{Digest, Sha256};
use std::{future::Future, pin::Pin, sync::Arc};

pub struct RecognitionPipeline {
    pub provider: Arc<dyn Provider>,
}
pub struct RecognitionOutput {
    page: String,
    fingerprint: String,
    inputs: InputVersions,
    previous_detection: Option<Revision>,
    previous_recognition: Option<Revision>,
    provider_version: String,
    regions: Vec<RecognizedRegion>,
}
fn check_run(db: &mut Connection, run: &runs::RunRecord, stage: &str) -> Result<(), AppError> {
    ProjectRepository::new(db, ProjectKind::Manga)?;
    if run.kind != "manga_recognition"
        || stage != "recognition"
        || run.snapshot.stages != ["recognition"]
        || run.snapshot.prompt_version != recognition::PROMPT_VERSION
    {
        return Err(AppError::invalid("mangaStage"));
    }
    if shared::settings(db)?.revision != run.snapshot.settings_revision
        || shared::glossary_revision(db)? != run.snapshot.glossary_revision
    {
        return Err(conflict());
    }
    Ok(())
}
fn fingerprint(db: &Connection, run: &runs::RunRecord, page: &str) -> Result<String, AppError> {
    let source:(String,i64,String,u32,u32)=db.query_row("SELECT p.original_asset_id,p.revision,v.reading_direction,p.width,p.height FROM manga_pages p JOIN manga_volumes v ON v.id=p.volume_id WHERE p.id=?1",[page],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?))).map_err(storage_error)?;
    let bytes = serde_json::to_vec(&(
        page,
        source,
        &run.snapshot.settings,
        &run.snapshot.settings_revision,
        &run.snapshot.glossary_revision,
        &run.snapshot.provider,
        &run.snapshot.prompt_version,
    ))
    .map_err(|_| AppError::invalid("fingerprint"))?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}
fn latest(db: &Connection, page: &str, stage: &str) -> Result<Option<Revision>, AppError> {
    let value: Option<i64> = db
        .query_row(
            "SELECT MAX(revision) FROM manga_results WHERE page_id=?1 AND stage=?2",
            params![page, stage],
            |r| r.get(0),
        )
        .map_err(storage_error)?;
    Ok(value.map(|v| Revision(v.to_string())))
}
impl StepExecutor for RecognitionPipeline {
    type Output = RecognitionOutput;
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
        lease.with_connection(|db, _| {
            check_run(db, run, stage)?;
            fingerprint(db, run, entity)
        })
    }
    fn compute<'a>(
        &'a self,
        lease: &'a ProjectLease,
        run: &'a runs::RunRecord,
        entity: &'a str,
        stage: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<Self::Output, AppError>> + Send + 'a>> {
        Box::pin(async move {
            let (path,asset_id,dimensions,rtl,mut output)=lease.with_connection(|db,root| {
                check_run(db,run,stage)?;
                let (asset,relative,revision,direction,width,height):(String,String,i64,String,u32,u32)=db.query_row("SELECT a.id,a.relative_path,p.revision,v.reading_direction,p.width,p.height FROM manga_pages p JOIN assets a ON a.id=p.original_asset_id JOIN manga_volumes v ON v.id=p.volume_id WHERE p.id=?1",[entity],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?))).map_err(storage_error)?;
                // Archive contents cannot select arbitrary files for upload.
                let name=relative.strip_prefix("assets/").ok_or_else(||AppError::invalid("assetPath"))?;
                let (hash,extension)=name.split_once('.').ok_or_else(||AppError::invalid("assetPath"))?;
                if hash!=asset || asset.len()!=64 || !asset.bytes().all(|b|b.is_ascii_hexdigit()) || !["png","jpg","jpeg","webp","gif"].contains(&extension) { return Err(AppError::invalid("assetPath")); }
                Ok((root.join(relative),asset,(width,height),direction=="rtl",RecognitionOutput {page:entity.into(),fingerprint:fingerprint(db,run,entity)?,inputs:InputVersions{source:Revision(revision.to_string()),settings:run.snapshot.settings_revision.clone(),glossary:run.snapshot.glossary_revision.clone()},previous_detection:latest(db,entity,"detection")?,previous_recognition:latest(db,entity,"recognition")?,provider_version:format!("{}:{}",self.provider.profile().id,self.provider.profile().model),regions:vec![]}))
            })?;
            static DECODING: std::sync::OnceLock<Arc<tokio::sync::Semaphore>> =
                std::sync::OnceLock::new();
            let permit = DECODING
                .get_or_init(|| Arc::new(tokio::sync::Semaphore::new(1)))
                .clone()
                .acquire_owned()
                .await
                .map_err(|_| AppError::invalid("image"))?;
            let input = tokio::task::spawn_blocking(move || {
                let _permit = permit;
                if std::fs::symlink_metadata(&path)
                    .map_err(|_| AppError::invalid("assetFile"))?
                    .len()
                    > 32 * 1024 * 1024
                {
                    return Err(AppError::invalid("imageSize"));
                }
                let bytes =
                    AssetStore::read_path(&path).map_err(|_| AppError::invalid("assetFile"))?;
                if format!("{:x}", Sha256::digest(&bytes)) != asset_id {
                    return Err(AppError::invalid("assetHash"));
                }
                let input = RecognitionInput::from_canonical(&bytes)?;
                if input.dimensions() != dimensions {
                    return Err(AppError::invalid("imageDimensions"));
                }
                Ok(input)
            })
            .await
            .map_err(|_| AppError::invalid("image"))??;
            output.regions = recognition::recognize(
                self.provider.as_ref(),
                input,
                run.snapshot
                    .settings
                    .source_language
                    .as_deref()
                    .ok_or_else(|| AppError::invalid("sourceLanguage"))?,
                rtl,
            )
            .await?;
            Ok(output)
        })
    }
    fn persist(&self, tx: &Transaction<'_>, output: Self::Output) -> Result<String, AppError> {
        // Stage revision guards and region mutation share the caller's transaction.
        let detection_id = uuid::Uuid::new_v4().to_string();
        let recognition_id = uuid::Uuid::new_v4().to_string();
        let (effective, issues) = regions::reconcile(tx, &output.page, &output.regions)?;
        let detection = serde_json::json!({"regions":effective.iter().map(|r|serde_json::json!({"id":r.id,"readingOrder":r.reading_order,"category":r.category,"bounds":r.bounds})).collect::<Vec<_>>()});
        results::save_manga_result_in(
            tx,
            &MangaResult {
                id: detection_id.clone(),
                page_id: output.page.clone(),
                stage: MangaStage::Detection,
                inputs: output.inputs.clone(),
                expected_result: output.previous_detection,
                fingerprint: output.fingerprint.clone(),
                provider_version: output.provider_version.clone(),
                output: MangaOutput::Structured(detection),
            },
        )?;
        results::save_manga_result_in(
            tx,
            &MangaResult {
                id: recognition_id.clone(),
                page_id: output.page,
                stage: MangaStage::Recognition,
                inputs: output.inputs,
                expected_result: output.previous_recognition,
                fingerprint: output.fingerprint,
                provider_version: output.provider_version,
                output: MangaOutput::Structured(
                    serde_json::json!({"regions":effective,"rawRegions":output.regions}),
                ),
            },
        )?;
        if !issues.is_empty() {
            for id in [&detection_id, &recognition_id] {
                tx.execute("UPDATE manga_reviews SET state='needs_review',issues_json=?2 WHERE result_id=?1",params![id,serde_json::to_string(&issues).map_err(|_|AppError::invalid("review"))?]).map_err(storage_error)?;
            }
        }
        Ok(recognition_id)
    }
}
