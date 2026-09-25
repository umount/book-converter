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
            let started = std::time::Instant::now();
            tracing::debug!(executable = %self.executable.display(), runtime = %request.runtime.display(),
                model = %request.model.display(), "Starting manga worker");
            let result = manga_inference::worker::execute(&self.executable, request, Duration::from_secs(300)).await;
            match &result {
                Ok(response) => tracing::debug!(elapsed_ms = started.elapsed().as_millis() as u64,
                    load_ms = response.load_millis, inference_ms = response.inference_millis,
                    "Manga worker completed"),
                Err(error) => tracing::error!(?error, elapsed_ms = started.elapsed().as_millis() as u64,
                    executable = %self.executable.display(), "Manga worker failed"),
            }
            result.map_err(|error| AppError::invalid(match error {manga_inference::Error::TextOverflow=>"mangaTextOverflow",manga_inference::Error::FontCoverage=>"mangaFontCoverage",manga_inference::Error::Timeout=>"mangaWorkerTimeout",_=>"mangaLocalProcessing"}))
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
        tracing::debug!(path = %root.display(), "Checking manga runtime pack");
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
                .map_err(|error| {
                    tracing::error!(path = %path.display(), %error, "Manga runtime file missing or inaccessible");
                    AppError::invalid("mangaRuntimeMissing")
                })?;
            if metadata.file_type().is_symlink()
                || (path != &root && !metadata.is_file())
                || (path == &root && !metadata.is_dir())
            {
                tracing::error!(path = %path.display(), "Manga runtime has an invalid file type");
                return Err(AppError::invalid("mangaRuntimeMissing"));
            }
        }
        verify_pack(&root, &executable, &runtime)?;
        tracing::debug!(path = %root.display(), "Manga runtime pack verified");
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
    if stage == MangaStage::Lettering {
        return Ok(super::image_pipeline::ImagePipeline {
            worker: Arc::new(NativeWorker {
                executable: files.executable.clone(),
            }),
            runtime: files.runtime,
            model: files.executable,
            stage,
            model_hash: format!(
                "{}:{}",
                manga_inference::text::VERSION,
                manga_inference::text::font_hash()
            ),
        });
    }
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
    create_run_mode(db,id,args,model_hash,false)
}
pub fn create_run_mode(db:&mut rusqlite::Connection,id:&str,args:&crate::app::requests::StartMangaStageArgs,model_hash:&str,rebuild:bool)->Result<(),AppError>{
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
        MangaStage::Lettering => ("lettering", "inpainting"),
        _ => return Err(AppError::invalid("mangaStage")),
    };
    let settings = shared::settings(db)?;
    let glossary = shared::glossary_revision(db)?;
    let rows = {
        let mut query=db.prepare("SELECT p.id,EXISTS(SELECT 1 FROM manga_results r WHERE r.page_id=p.id AND r.stage=?1 AND r.validity='current' AND r.page_revision=p.revision AND r.settings_revision=?3 AND r.glossary_revision=?4),EXISTS(SELECT 1 FROM manga_results r WHERE r.page_id=p.id AND r.stage=?2 AND (?2='recognition' OR (r.validity='current' AND r.page_revision=p.revision AND r.settings_revision=?3 AND r.glossary_revision=?4))) FROM manga_pages p JOIN manga_volumes v ON v.id=p.volume_id WHERE (?1!='lettering' OR EXISTS(SELECT 1 FROM manga_results t WHERE t.page_id=p.id AND t.stage='translation' AND t.validity='current' AND t.page_revision=p.revision AND t.settings_revision=?3 AND t.glossary_revision=?4)) ORDER BY v.position,p.position").map_err(storage_error)?;
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
        &if rebuild {"manga_rebuild".into()} else {format!("manga_{stage}")},
        &runs::RunSnapshot {
            manga: None,
            retarget: None,
            settings: settings.choices,
            settings_revision: settings.revision,
            glossary_revision: glossary,
            selected_ids: selected,
            prompt_version: format!("{VERSION}:{model_hash}"),
            stages: if rebuild {vec!["masks".into(),"inpainting".into(),"lettering".into()]} else {vec![stage.into()]},
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

fn verify_pack(root: &Path, executable: &Path, runtime: &Path) -> Result<(), AppError> {
    use sha2::{Digest, Sha256};
    use std::io::Read;
    let invalid = || AppError::invalid("mangaRuntimeMissing");
    let bytes = std::fs::read(root.join("manifest.json")).map_err(|error| {
        tracing::error!(path = %root.display(), %error, "Manga runtime manifest missing or unreadable");
        invalid()
    })?;
    if bytes.len() > 16 * 1024 {
        tracing::error!(path = %root.display(), "Manga runtime manifest exceeds expected size");
        return Err(invalid());
    }
    let manifest: serde_json::Value = serde_json::from_slice(&bytes).map_err(|error| {
        tracing::error!(path = %root.display(), %error, "Invalid manga runtime manifest JSON");
        invalid()
    })?;
    let target = if cfg!(target_os = "windows") {
        "x86_64-pc-windows-msvc"
    } else if cfg!(target_os = "macos") {
        if cfg!(target_arch = "aarch64") {
            "aarch64-apple-darwin"
        } else {
            "x86_64-apple-darwin"
        }
    } else {
        "x86_64-unknown-linux-gnu"
    };
    if manifest["version"] != 1
        || manifest["onnxVersion"] != "1.22.0"
        || manifest["target"] != target
    {
        tracing::error!(path = %root.display(), expected_target = target,
            "Manga runtime manifest version or target mismatch");
        return Err(invalid());
    }
    let mut required = vec![
        executable
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(invalid)?,
        runtime
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(invalid)?,
    ];
    if cfg!(windows) {
        required.push("onnxruntime_providers_shared.dll");
    } else if cfg!(target_os = "linux") {
        required.push("libonnxruntime_providers_shared.so");
    }
    for name in required {
        let path = root.join(name);
        let info = std::fs::symlink_metadata(&path).map_err(|error| {
            tracing::error!(path = %path.display(), %error, "Required manga runtime library unavailable");
            invalid()
        })?;
        if !info.is_file()
            || info.len() > 128 * 1024 * 1024
            || manifest["files"][name]["bytes"].as_u64() != Some(info.len())
        {
            tracing::error!(path = %path.display(), bytes = info.len(), "Manga runtime file size or type mismatch");
            return Err(invalid());
        }
        let mut bytes = Vec::new();
        std::fs::File::open(&path)
            .map_err(|_| invalid())?
            .take(128 * 1024 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| invalid())?;
        if bytes.len() as u64 != info.len()
            || manifest["files"][name]["sha256"] != format!("{:x}", Sha256::digest(&bytes))
        {
            tracing::error!(path = %path.display(), "Manga runtime SHA-256 mismatch");
            return Err(invalid());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};
    #[test]
    fn damaged_or_wrong_target_runtime_pack_is_rejected() {
        let workspace = Workspace::new().unwrap();
        let root = workspace.root.join("manga-runtime");
        std::fs::create_dir(&root).unwrap();
        let executable = if cfg!(windows) {
            "manga-inference.exe"
        } else {
            "manga-inference"
        };
        let library = if cfg!(windows) {
            "onnxruntime.dll"
        } else if cfg!(target_os = "macos") {
            "libonnxruntime.1.22.0.dylib"
        } else {
            "libonnxruntime.so.1.22.0"
        };
        let target = if cfg!(windows) {
            "x86_64-pc-windows-msvc"
        } else if cfg!(target_os = "macos") {
            if cfg!(target_arch = "aarch64") {
                "aarch64-apple-darwin"
            } else {
                "x86_64-apple-darwin"
            }
        } else {
            "x86_64-unknown-linux-gnu"
        };
        let mut names = vec![executable, library];
        if cfg!(windows) {
            names.push("onnxruntime_providers_shared.dll");
        } else if cfg!(target_os = "linux") {
            names.push("libonnxruntime_providers_shared.so");
        }
        let mut files = serde_json::Map::new();
        for name in names {
            std::fs::write(root.join(name), b"fixture").unwrap();
            files.insert(
                name.into(),
                serde_json::json!({"bytes":7,"sha256":format!("{:x}",Sha256::digest(b"fixture"))}),
            );
        }
        let mut manifest =
            serde_json::json!({"version":1,"onnxVersion":"1.22.0","target":target,"files":files});
        let save = |value: &serde_json::Value| {
            std::fs::write(
                root.join("manifest.json"),
                serde_json::to_vec(value).unwrap(),
            )
            .unwrap()
        };
        save(&manifest);
        assert!(NativeFiles::discover(&workspace.root).is_ok());
        manifest["target"] = serde_json::json!("wrong-target");
        save(&manifest);
        assert!(NativeFiles::discover(&workspace.root).is_err());
        manifest["target"] = serde_json::json!(target);
        save(&manifest);
        std::fs::write(root.join(library), b"changed").unwrap();
        assert!(NativeFiles::discover(&workspace.root).is_err());
    }
}
