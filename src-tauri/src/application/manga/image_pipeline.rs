//! Local derived images publish with the same revision/checkpoint rules as API stages.
use super::{local, regions};
use crate::{
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
use manga_inference::{
    protocol::{Asset, Operation, Request},
    Crop,
};
use rusqlite::{params, Connection, Transaction};
use sha2::{Digest, Sha256};
use std::{
    future::Future,
    io::Read,
    path::{Path, PathBuf},
    pin::Pin,
    sync::Arc,
};
pub struct ImagePipeline {
    pub worker: Arc<dyn local::ImageWorker>,
    pub runtime: PathBuf,
    pub model: PathBuf,
    pub stage: MangaStage,
    pub model_hash: String,
}
pub struct ImageOutput {
    result: MangaResult,
    root: PathBuf,
    bytes: Vec<u8>,
    layouts: Vec<manga_inference::text::TextLayout>,
}
impl ImagePipeline {
    fn name(&self) -> Result<&'static str, AppError> {
        match self.stage {
            MangaStage::Masks => Ok("masks"),
            MangaStage::Inpainting => Ok("inpainting"),
            MangaStage::Lettering => Ok("lettering"),
            _ => Err(AppError::invalid("mangaStage")),
        }
    }
    fn check(
        &self,
        db: &mut Connection,
        run: &runs::RunRecord,
        stage: &str,
    ) -> Result<(), AppError> {
        ProjectRepository::new(db, ProjectKind::Manga)?;
        if stage != self.name()?
            || run.kind != format!("manga_{stage}")
            || run.snapshot.stages != [stage]
            || run.snapshot.prompt_version != format!("{}:{}", local::VERSION, self.model_hash)
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
    fn dependencies(&self, db: &Connection, page: &str) -> Result<serde_json::Value, AppError> {
        let (source, revision): (String, i64) = db
            .query_row(
                "SELECT original_asset_id,revision FROM manga_pages WHERE id=?1",
                [page],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(storage_error)?;
        let data = if self.stage == MangaStage::Masks {
            let recognized:bool=db.query_row("SELECT EXISTS(SELECT 1 FROM manga_results WHERE page_id=?1 AND stage='recognition')",[page],|r|r.get(0)).map_err(storage_error)?;
            if !recognized {
                return Err(AppError::invalid("mangaRecognitionRequired"));
            }
            serde_json::json!(regions::read(db, page)?
                .iter()
                .map(|r| (&r.id, &r.bounds))
                .collect::<Vec<_>>())
        } else if self.stage == MangaStage::Lettering {
            let ready: bool = db.query_row("SELECT EXISTS(SELECT 1 FROM manga_results r JOIN manga_pages p ON p.id=r.page_id JOIN project_settings s ON s.singleton=1 JOIN glossary_state g ON g.singleton=1 WHERE r.page_id=?1 AND r.stage='translation' AND r.validity='current' AND r.page_revision=p.revision AND r.settings_revision=s.revision AND r.glossary_revision=g.revision)",[page],|r|r.get(0)).map_err(storage_error)?;
            if !ready {
                return Err(AppError::invalid("mangaTranslationRequired"));
            }

            serde_json::json!({"cleaned":current_asset(db, page, "inpainting")?, "regions": regions::read(db,page)?.iter().map(|r| (&r.id,&r.bounds,&r.translated_text)).collect::<Vec<_>>()})
        } else {
            serde_json::json!(mask_asset(db, page)?)
        };
        Ok(
            serde_json::json!({"source":source,"revision":revision,"data":data,"model":self.model_hash,"version":local::VERSION}),
        )
    }
    fn hash(&self, db: &Connection, run: &runs::RunRecord, page: &str) -> Result<String, AppError> {
        let bytes = serde_json::to_vec(&(
            self.dependencies(db, page)?,
            &run.snapshot.settings_revision,
            &run.snapshot.glossary_revision,
        ))
        .map_err(|_| AppError::invalid("fingerprint"))?;
        Ok(format!("{:x}", Sha256::digest(bytes)))
    }
}
fn current_asset(db: &Connection, page: &str, stage: &str) -> Result<String, AppError> {
    db.query_row("SELECT r.output_asset_id FROM manga_results r JOIN manga_pages p ON p.id=r.page_id JOIN project_settings s ON s.singleton=1 JOIN glossary_state g ON g.singleton=1 WHERE r.page_id=?1 AND r.stage=?2 AND r.validity='current' AND r.page_revision=p.revision AND r.settings_revision=s.revision AND r.glossary_revision=g.revision ORDER BY r.revision DESC LIMIT 1",params![page,stage],|r|r.get(0)).map_err(|_|AppError::invalid("mangaImageRequired"))
}
fn mask_asset(db: &Connection, page: &str) -> Result<String, AppError> {
    current_asset(db, page, "masks")
}
fn asset(db: &Connection, root: &Path, id: &str) -> Result<Asset, AppError> {
    let relative: String = db
        .query_row("SELECT relative_path FROM assets WHERE id=?1", [id], |r| {
            r.get(0)
        })
        .map_err(storage_error)?;
    if id.len() != 64
        || !id.bytes().all(|b| b.is_ascii_hexdigit())
        || !["png", "jpg", "jpeg", "webp", "gif"]
            .iter()
            .any(|ext| relative == format!("assets/{id}.{ext}"))
    {
        return Err(AppError::invalid("assetPath"));
    }
    let directory = root.join("assets");
    if std::fs::symlink_metadata(&directory)
        .map_err(|_| AppError::invalid("assetPath"))?
        .file_type()
        .is_symlink()
    {
        return Err(AppError::invalid("assetPath"));
    }
    Ok(Asset {
        path: root.join(relative),
        sha256: id.into(),
    })
}
impl StepExecutor for ImagePipeline {
    type Output = ImageOutput;
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
            self.check(db, run, stage)?;
            self.hash(db, run, entity)
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
            let workspace = local::Workspace::new()?;
            let (request,mut output,dimensions)=lease.with_connection(|db,root|{
                self.check(db,run,stage)?;
                let (original,revision,width,height):(String,i64,u32,u32)=db.query_row("SELECT original_asset_id,revision,width,height FROM manga_pages WHERE id=?1",[entity],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).map_err(storage_error)?;
                let input_id=if self.stage==MangaStage::Lettering {current_asset(db,entity,"inpainting")?} else {original};
                let operation=if self.stage==MangaStage::Lettering {
                    let texts=regions::read(db,entity)?.iter().map(|r|{
                        r.bounds.validate(width,height)?;
                        let x=r.bounds.x.floor() as u32;let y=r.bounds.y.floor() as u32;
                        let text=r.translated_text.as_ref().filter(|t|!t.trim().is_empty()).ok_or_else(||AppError::invalid("mangaTranslationRequired"))?;
                        Ok(manga_inference::text::TextRegion{id:r.id.clone(),text:text.clone(),bounds:Crop{x,y,width:((r.bounds.x+r.bounds.width).ceil() as u32).min(width)-x,height:((r.bounds.y+r.bounds.height).ceil() as u32).min(height)-y}})
                    }).collect::<Result<Vec<_>,AppError>>()?;
                    Operation::Lettering{regions:texts}
                }else if self.stage==MangaStage::Masks {
                    let crops=regions::read(db,entity)?.iter().map(|r|{
                        r.bounds.validate(width,height)?;
                        let x=r.bounds.x.floor() as u32;let y=r.bounds.y.floor() as u32;
                        Ok(Crop{x,y,width:((r.bounds.x+r.bounds.width).ceil() as u32).min(width)-x,height:((r.bounds.y+r.bounds.height).ceil() as u32).min(height)-y})
                    }).collect::<Result<Vec<_>,AppError>>()?;
                    Operation::Masks{regions:crops,margin:2}
                }else{Operation::Inpainting{mask:asset(db,root,&mask_asset(db,entity)?)?}};
                let previous:Option<i64>=db.query_row("SELECT MAX(revision) FROM manga_results WHERE page_id=?1 AND stage=?2",params![entity,stage],|r|r.get(0)).map_err(storage_error)?;
                Ok((Request{version:1,runtime:self.runtime.clone(),model:self.model.clone(),input:asset(db,root,&input_id)?,output:workspace.root.join("result.png"),operation},ImageOutput{result:MangaResult{id:uuid::Uuid::new_v4().to_string(),page_id:entity.into(),stage:self.stage.clone(),inputs:InputVersions{source:Revision(revision.to_string()),settings:run.snapshot.settings_revision.clone(),glossary:run.snapshot.glossary_revision.clone()},expected_result:previous.map(|v|Revision(v.to_string())),fingerprint:self.hash(db,run,entity)?,provider_version:format!("{}:{}",local::VERSION,self.model_hash),output:MangaOutput::Image(String::new())},root:root.to_path_buf(),bytes:vec![],layouts:vec![]},(width,height)))
            })?;
            let response = self.worker.run(&request).await?;
            if (response.width, response.height) != dimensions {
                return Err(AppError::invalid("resultDimensions"));
            }
            if let Operation::Lettering { regions } = &request.operation {
                if response.layouts.len() != regions.len()
                    || response
                        .layouts
                        .iter()
                        .zip(regions)
                        .any(|(layout, region)| {
                            layout.id != region.id
                                || layout.font_sha256 != manga_inference::text::font_hash()
                                || !layout.font_size.is_finite()
                                || layout.font_size < 8.0
                                || layout.font_size > 96.0
                                || !layout.line_height.is_finite()
                                || layout.line_height <= 0.0
                        })
                {
                    return Err(AppError::invalid("mangaLetteringLayout"));
                }
                output.layouts = response.layouts;
            }
            // A private temporary result is read once, then published only after revision checks.
            let metadata = std::fs::symlink_metadata(&request.output)
                .map_err(|_| AppError::invalid("mangaWorkerOutput"))?;
            if !metadata.is_file() || metadata.len() > 32 * 1024 * 1024 {
                return Err(AppError::invalid("mangaWorkerOutput"));
            }
            std::fs::File::open(&request.output)
                .map_err(|_| AppError::invalid("mangaWorkerOutput"))?
                .take(32 * 1024 * 1024 + 1)
                .read_to_end(&mut output.bytes)
                .map_err(|_| AppError::invalid("mangaWorkerOutput"))?;
            if output.bytes.len() > 32 * 1024 * 1024 {
                return Err(AppError::invalid("mangaWorkerOutput"));
            }
            Ok(output)
        })
    }
    fn persist(&self, tx: &Transaction<'_>, mut output: Self::Output) -> Result<String, AppError> {
        let store = AssetStore::new(&output.root).map_err(|_| AppError::invalid("assetStore"))?;
        let asset = store
            .publish(tx, &output.bytes, "png")
            .map_err(|_| AppError::invalid("mangaWorkerOutput"))?;
        output.result.output = MangaOutput::Image(asset.clone());
        results::save_manga_result_in(tx, &output.result)?;
        for layout in &output.layouts {
            let style = serde_json::to_string(layout)
                .map_err(|_| AppError::invalid("mangaLetteringLayout"))?;
            tx.execute("UPDATE manga_regions SET style_json=?1,style_revision=style_revision+1 WHERE id=?2 AND page_id=?3",params![style,layout.id,output.result.page_id]).map_err(storage_error)?;
        }
        if self.stage == MangaStage::Masks {
            tx.execute(
                "DELETE FROM manga_masks WHERE page_id=?1",
                [&output.result.page_id],
            )
            .map_err(storage_error)?;
            tx.execute("INSERT INTO manga_masks(id,page_id,region_id,asset_id,geometry_revision) VALUES(?1,?2,NULL,?3,?4)",params![uuid::Uuid::new_v4().to_string(),output.result.page_id,asset,output.result.inputs.source.value()?]).map_err(storage_error)?;
        }
        Ok(output.result.id)
    }
}
