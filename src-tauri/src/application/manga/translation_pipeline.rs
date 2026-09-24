//! Checkpointed page dialogue translation; result and region text publish atomically.
use super::{
    regions,
    translation::{self, TranslatedRegion},
};
use crate::{
    ai::Provider,
    app::contracts::{AppError, MangaStage, ProjectKind, Revision},
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
pub struct TranslationPipeline {
    pub provider: Arc<dyn Provider>,
}
pub struct TranslationOutput {
    result: MangaResult,
    regions: Vec<regions::Region>,
    translated: Vec<TranslatedRegion>,
}
fn check(db: &mut Connection, run: &runs::RunRecord, stage: &str) -> Result<(), AppError> {
    ProjectRepository::new(db, ProjectKind::Manga)?;
    if run.kind != "manga_translation"
        || stage != "translation"
        || run.snapshot.stages != ["translation"]
        || run.snapshot.prompt_version != translation::PROMPT_VERSION
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
fn source(db: &Connection, page: &str) -> Result<i64, AppError> {
    let revision = db
        .query_row(
            "SELECT revision FROM manga_pages WHERE id=?1",
            [page],
            |r| r.get(0),
        )
        .map_err(storage_error)?;
    let recognized: bool = db
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM manga_results WHERE page_id=?1 AND stage='recognition')",
            [page],
            |r| r.get(0),
        )
        .map_err(storage_error)?;
    if !recognized {
        return Err(AppError::invalid("mangaRecognitionRequired"));
    }
    Ok(revision)
}
fn fingerprint(db: &Connection, run: &runs::RunRecord, page: &str) -> Result<String, AppError> {
    let regions = regions::read(db, page)?;
    // Output text/revisions are excluded so a successful checkpoint can be reused.
    // Manual edits also bump the page source revision, invalidating this fingerprint.
    let dependencies = regions
        .iter()
        .map(|r| {
            (
                &r.id,
                r.reading_order,
                &r.category,
                &r.bounds,
                &r.source_text,
                r.translation_manual,
                if r.translation_manual {
                    r.translated_text.as_ref()
                } else {
                    None
                },
            )
        })
        .collect::<Vec<_>>();
    let bytes = serde_json::to_vec(&(
        page,
        source(db, page)?,
        dependencies,
        &run.snapshot.settings,
        &run.snapshot.settings_revision,
        &run.snapshot.glossary_revision,
        &run.snapshot.provider,
        &run.snapshot.prompt_version,
    ))
    .map_err(|_| AppError::invalid("fingerprint"))?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}
impl StepExecutor for TranslationPipeline {
    type Output = TranslationOutput;
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
            check(db, run, stage)?;
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
            let (mut output,terms)=lease.with_connection(|db,_|{
                check(db,run,stage)?;
                let previous:Option<i64>=db.query_row("SELECT MAX(revision) FROM manga_results WHERE page_id=?1 AND stage='translation'",[entity],|r|r.get(0)).map_err(storage_error)?;
                Ok((TranslationOutput{result:MangaResult{id:uuid::Uuid::new_v4().to_string(),page_id:entity.into(),stage:MangaStage::Translation,inputs:InputVersions{source:Revision(source(db,entity)?.to_string()),settings:run.snapshot.settings_revision.clone(),glossary:run.snapshot.glossary_revision.clone()},expected_result:previous.map(|r|Revision(r.to_string())),fingerprint:fingerprint(db,run,entity)?,provider_version:format!("{}:{}",self.provider.profile().id,self.provider.profile().model),output:MangaOutput::Structured(serde_json::json!({}))},regions:regions::read(db,entity)?,translated:vec![]},shared::glossary(db)?))
            })?;
            output.translated = translation::translate(
                self.provider.as_ref(),
                &output.regions,
                run.snapshot
                    .settings
                    .source_language
                    .as_deref()
                    .ok_or_else(|| AppError::invalid("sourceLanguage"))?,
                &run.snapshot.settings.target_language,
                &terms,
            )
            .await?;
            Ok(output)
        })
    }
    fn persist(&self, tx: &Transaction<'_>, mut output: Self::Output) -> Result<String, AppError> {
        // Validate every captured row, including manually locked rows, before mutation.
        for region in &output.regions {
            let revision: i64 = tx
                .query_row(
                    "SELECT revision FROM manga_regions WHERE id=?1 AND page_id=?2",
                    params![region.id, output.result.page_id],
                    |r| r.get(0),
                )
                .map_err(storage_error)?;
            if revision != region.revision {
                return Err(conflict());
            }
        }
        for translated in &output.translated {
            let count=tx.execute("UPDATE manga_regions SET translated_text=?1,text_revision=text_revision+1,revision=revision+1 WHERE id=?2 AND page_id=?3 AND translation_manual=0",params![translated.translated_text,translated.id,output.result.page_id]).map_err(storage_error)?;
            if count != 1 {
                return Err(conflict());
            }
        }
        // Keep a complete immutable snapshot, including preserved manual translations.
        output.result.output = MangaOutput::Structured(
            serde_json::json!({"regions":regions::read(tx,&output.result.page_id)?.iter().map(|r|serde_json::json!({"id":r.id,"translatedText":r.translated_text,"manual":r.translation_manual})).collect::<Vec<_>>()}),
        );
        results::save_manga_result_in(tx, &output.result)?;
        Ok(output.result.id)
    }
}
