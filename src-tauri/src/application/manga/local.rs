//! Isolated CPU worker integration. Native executables come from app resources;
//! model paths come from the fixed download catalog, never from a webview request.
use crate::app::contracts::AppError;
use manga_inference::protocol::{Request, Response};
use std::{
    future::Future,
    path::{Path, PathBuf},
    pin::Pin,
    sync::Arc,
    time::Duration,
};

pub const VERSION: &str = "manga-local-v1";
pub const MASK_MODEL: &str = "comic-text-mask-resnet18";
pub const CLEAN_MODEL: &str = "lama-onnx-fp32";

pub trait ImageWorker: Send + Sync {
    fn run<'a>(
        &'a self,
        request: &'a Request,
    ) -> Pin<Box<dyn Future<Output = Result<Response, AppError>> + Send + 'a>>;
}
pub struct NativeWorker {
    pub executable: PathBuf,
}
impl ImageWorker for NativeWorker {
    fn run<'a>(
        &'a self,
        request: &'a Request,
    ) -> Pin<Box<dyn Future<Output = Result<Response, AppError>> + Send + 'a>> {
        Box::pin(async move {
            manga_inference::worker::execute(&self.executable, request, Duration::from_secs(300))
                .await
                .map_err(|_| AppError::invalid("mangaLocalProcessing"))
        })
    }
}
#[derive(Clone)]
pub struct NativeFiles {
    pub executable: PathBuf,
    pub runtime: PathBuf,
}
impl NativeFiles {
    pub fn discover(resources: &Path) -> Result<Self, AppError> {
        let root = resources.join("manga-runtime");
        let executable = root.join(if cfg!(windows) {
            "manga-inference.exe"
        } else {
            "manga-inference"
        });
        let runtime = root.join(if cfg!(windows) {
            "onnxruntime.dll"
        } else if cfg!(target_os = "macos") {
            "libonnxruntime.1.22.0.dylib"
        } else {
            "libonnxruntime.so.1.22.0"
        });
        for path in [&root, &executable, &runtime] {
            let metadata = std::fs::symlink_metadata(path)
                .map_err(|_| AppError::invalid("mangaRuntimeMissing"))?;
            if metadata.file_type().is_symlink()
                || (path != &root && !metadata.is_file())
                || (path == &root && !metadata.is_dir())
            {
                return Err(AppError::invalid("mangaRuntimeMissing"));
            }
        }
        Ok(Self {
            executable,
            runtime,
        })
    }
}
pub struct Workspace {
    pub root: PathBuf,
}
impl Workspace {
    pub fn new() -> Result<Self, AppError> {
        let root =
            std::env::temp_dir().join(format!("book-converter-manga-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).map_err(|_| AppError::invalid("mangaTemporaryDirectory"))?;
        Ok(Self { root })
    }
}
impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

pub fn pipeline(
    models: &Arc<crate::models::ModelManager>,
    files: NativeFiles,
    stage: crate::app::contracts::MangaStage,
) -> Result<super::image_pipeline::ImagePipeline, AppError> {
    use crate::app::contracts::MangaStage;
    let id = match stage {
        MangaStage::Masks => MASK_MODEL,
        MangaStage::Inpainting => CLEAN_MODEL,
        _ => return Err(AppError::invalid("mangaStage")),
    };
    let (spec, model) = models
        .downloaded_artifact(id)
        .ok_or_else(|| AppError::invalid("mangaModelMissing"))?;
    Ok(super::image_pipeline::ImagePipeline {
        worker: Arc::new(NativeWorker {
            executable: files.executable,
        }),
        runtime: files.runtime,
        model,
        stage,
        model_hash: spec.sha256,
    })
}

pub fn create_run(
    db: &mut rusqlite::Connection,
    id: &str,
    args: &crate::app::requests::StartMangaStageArgs,
    model_hash: &str,
) -> Result<(), AppError> {
    use crate::{
        app::contracts::{MangaStage, ProjectKind},
        storage::{
            repository::{storage_error, ProjectRepository},
            runs, shared,
        },
    };
    ProjectRepository::new(db, ProjectKind::Manga)?;
    if args.options.max_pages == 0 {
        return Err(AppError::invalid("maxPages"));
    }
    let (stage, prerequisite) = match args.stage {
        MangaStage::Masks => ("masks", "recognition"),
        MangaStage::Inpainting => ("inpainting", "masks"),
        _ => return Err(AppError::invalid("mangaStage")),
    };
    let settings = shared::settings(db)?;
    let glossary = shared::glossary_revision(db)?;
    let rows = {
        let mut query=db.prepare("SELECT p.id,EXISTS(SELECT 1 FROM manga_results r WHERE r.page_id=p.id AND r.stage=?1 AND r.validity='current' AND r.page_revision=p.revision AND r.settings_revision=?3 AND r.glossary_revision=?4),EXISTS(SELECT 1 FROM manga_results r WHERE r.page_id=p.id AND r.stage=?2 AND (?2='recognition' OR (r.validity='current' AND r.page_revision=p.revision AND r.settings_revision=?3 AND r.glossary_revision=?4))) FROM manga_pages p JOIN manga_volumes v ON v.id=p.volume_id ORDER BY v.position,p.position").map_err(storage_error)?;
        let rows = query
            .query_map(
                rusqlite::params![
                    stage,
                    prerequisite,
                    settings.revision.value()?,
                    glossary.value()?
                ],
                |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, bool>(1)?,
                        r.get::<_, bool>(2)?,
                    ))
                },
            )
            .map_err(storage_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(storage_error)?;
        rows
    };
    let ordered = rows.iter().map(|r| r.0.clone()).collect::<Vec<_>>();
    let eligible = rows
        .into_iter()
        .filter(|(_, current, ready)| *ready && (args.options.force || !*current))
        .map(|r| r.0)
        .collect::<std::collections::HashSet<_>>();
    let selected = args
        .selection
        .resolve(&ordered)?
        .into_iter()
        .filter(|id| eligible.contains(id))
        .take(args.options.max_pages as usize)
        .collect::<Vec<_>>();
    if selected.is_empty() {
        return Err(AppError::invalid("noEligiblePages"));
    }
    runs::create_run(
        db,
        id,
        &format!("manga_{stage}"),
        &runs::RunSnapshot {
            retarget: None,
            settings: settings.choices,
            settings_revision: settings.revision,
            glossary_revision: glossary,
            selected_ids: selected,
            prompt_version: format!("{VERSION}:{model_hash}"),
            stages: vec![stage.into()],
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
