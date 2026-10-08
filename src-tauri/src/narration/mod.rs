//! Local, checkpointed book narration. Models are global; jobs are project-owned.
pub mod contracts;
#[cfg(test)]
mod tests;
mod text;
mod worker;
use crate::{
    app::contracts::{AppError, ErrorCode, ProjectId},
    models::ModelManager,
    project::lifecycle::{ProjectLease, ProjectManager},
};
use contracts::*;
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
use tokio::sync::watch;
pub use worker::Runtime;

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ChapterInput {
    title: String,
    chunks: Vec<String>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Input {
    version: u32,
    language: String,
    voice: String,
    device: AudioDevice,
    chapters: Vec<ChapterInput>,
}
struct Active {
    project: ProjectId,
    id: String,
    cancel: watch::Sender<bool>,
}
pub struct Narration {
    root: PathBuf,
    active: Mutex<Option<Active>>,
}
pub(super) fn failure(reason: &str) -> AppError {
    AppError {
        code: ErrorCode::CapabilityUnavailable,
        message_key: "errors.audio".into(),
        params: [("reason".into(), reason.into())].into(),
        retryable: true,
    }
}
fn io_error(_: impl std::fmt::Display) -> AppError {
    failure("audioStorage")
}
fn valid_id(id: &str) -> Result<(), AppError> {
    if uuid::Uuid::parse_str(id).is_ok_and(|v| !v.is_nil() && v.to_string() == id) {
        Ok(())
    } else {
        Err(AppError::invalid("jobId"))
    }
}
fn real_dir(path: &Path) -> Result<(), AppError> {
    let meta = std::fs::symlink_metadata(path).map_err(io_error)?;
    if meta.is_dir() && !meta.file_type().is_symlink() {
        Ok(())
    } else {
        Err(failure("audioStorage"))
    }
}
fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T, AppError> {
    let meta = std::fs::symlink_metadata(path).map_err(io_error)?;
    if !meta.is_file() || meta.file_type().is_symlink() {
        return Err(failure("audioStorage"));
    }
    serde_json::from_reader(std::fs::File::open(path).map_err(io_error)?).map_err(io_error)
}
fn write_json(path: &Path, value: &impl Serialize) -> Result<(), AppError> {
    use std::io::Write;
    let temp = path.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| {
        let mut f = std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temp)
            .map_err(io_error)?;
        f.write_all(&serde_json::to_vec(value).map_err(io_error)?)
            .map_err(io_error)?;
        f.sync_all().map_err(io_error)?;
        std::fs::rename(&temp, path).map_err(io_error)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(temp);
    }
    result
}
impl Narration {
    pub fn remove_project(&self, project: &ProjectId) -> Result<(), AppError> {
        project.validate()?;
        let path = self.root.join(project.as_str());
        if path.exists() {
            real_dir(&self.root)?;
            real_dir(&path)?;
            std::fs::remove_dir_all(path).map_err(io_error)?;
        }
        Ok(())
    }
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            active: Mutex::new(None),
        }
    }
    fn project_dir(&self, project: &ProjectId) -> Result<PathBuf, AppError> {
        project.validate()?;
        std::fs::create_dir_all(&self.root).map_err(io_error)?;
        real_dir(&self.root)?;
        let path = self.root.join(project.as_str());
        std::fs::create_dir_all(&path).map_err(io_error)?;
        real_dir(&path)?;
        Ok(path)
    }
    fn directory(&self, args: &AudioJobArgs) -> Result<PathBuf, AppError> {
        valid_id(&args.job_id)?;
        let path = self.project_dir(&args.project_id)?.join(&args.job_id);
        real_dir(&path)?;
        Ok(path)
    }
    fn read_view(&self, args: &AudioJobArgs) -> Result<AudioJobView, AppError> {
        let path = self.directory(args)?.join("status.json");
        let mut view: AudioJobView = read_json(&path)?;
        if view.id != args.job_id || view.project_id != args.project_id {
            return Err(failure("audioStorage"));
        }
        let active = self.active.lock().unwrap();
        if view.state == AudioState::Running
            && !active
                .as_ref()
                .is_some_and(|a| a.id == view.id && a.project == view.project_id)
        {
            view.state = AudioState::Interrupted;
            write_json(&path, &view)?;
        }
        Ok(view)
    }
    pub fn list(&self, project: &ProjectId) -> Result<Vec<AudioJobView>, AppError> {
        let mut views = Vec::new();
        for entry in std::fs::read_dir(self.project_dir(project)?).map_err(io_error)? {
            let entry = entry.map_err(io_error)?;
            let id = entry.file_name().to_string_lossy().into_owned();
            if valid_id(&id).is_ok() && entry.path().join("status.json").exists() {
                views.push(self.read_view(&AudioJobArgs {
                    project_id: project.clone(),
                    job_id: id,
                })?);
            }
        }
        views.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        Ok(views)
    }
    fn reserve(&self, project: &ProjectId, id: &str) -> Result<watch::Receiver<bool>, AppError> {
        let mut active = self.active.lock().unwrap();
        if active.is_some() {
            return Err(failure("audioBusy"));
        }
        let (cancel, receiver) = watch::channel(false);
        *active = Some(Active {
            project: project.clone(),
            id: id.into(),
            cancel,
        });
        Ok(receiver)
    }
    pub fn cancel(&self, args: &AudioJobArgs) -> Result<(), AppError> {
        args.project_id.validate()?;
        valid_id(&args.job_id)?;
        if let Some(active) = self
            .active
            .lock()
            .unwrap()
            .as_ref()
            .filter(|a| a.project == args.project_id && a.id == args.job_id)
        {
            let _ = active.cancel.send(true);
        }
        Ok(())
    }
    pub async fn start(
        self: &Arc<Self>,
        manager: Arc<ProjectManager>,
        models: Arc<ModelManager>,
        runtime: worker::Runtime,
        args: AudioStartArgs,
    ) -> Result<AudioJobView, AppError> {
        let lease = manager.lease(&args.project_id)?;
        let (lease, input, args) = tokio::task::spawn_blocking(move || {
            text::snapshot(&lease, &args).map(|input| (lease, input, args))
        })
        .await
        .map_err(io_error)??;
        if models
            .list()
            .await
            .map_err(|_| failure("audioModels"))?
            .iter()
            .any(|v| v.status != crate::models::ModelStatus::Downloaded)
        {
            return Err(failure("audioModels"));
        }
        let id = uuid::Uuid::new_v4().to_string();
        let cancel = self.reserve(&args.project_id, &id)?;
        let view = AudioJobView {
            id,
            project_id: args.project_id,
            state: AudioState::Running,
            voice: input.voice.clone(),
            device: input.device.clone(),
            text: args.text,
            language: input.language.clone(),
            completed_chunks: 0,
            total_chunks: input.chapters.iter().map(|c| c.chunks.len() as u32).sum(),
            completed_chapters: 0,
            total_chapters: input.chapters.len() as u32,
            current_chapter: String::new(),
            error: None,
            created_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis()
                .to_string(),
        };
        let saved = (|| {
            let dir = self.project_dir(&view.project_id)?.join(&view.id);
            std::fs::create_dir(&dir).map_err(io_error)?;
            write_json(&dir.join("input.json"), &input)?;
            write_json(&dir.join("status.json"), &view)?;
            Ok::<_, AppError>(dir)
        })();
        let dir = match saved {
            Ok(dir) => dir,
            Err(e) => {
                *self.active.lock().unwrap() = None;
                return Err(e);
            }
        };
        self.launch(lease, models, runtime, dir, view.clone(), cancel);
        Ok(view)
    }
    pub async fn resume(
        self: &Arc<Self>,
        manager: Arc<ProjectManager>,
        models: Arc<ModelManager>,
        runtime: worker::Runtime,
        args: AudioJobArgs,
    ) -> Result<AudioJobView, AppError> {
        let lease = manager.lease(&args.project_id)?;
        let mut view = self.read_view(&args)?;
        if view.state == AudioState::Succeeded {
            return Ok(view);
        }
        let dir = self.directory(&args)?;
        let input: Input = read_json(&dir.join("input.json"))?;
        if input.version != 1 {
            return Err(failure("audioVersion"));
        }
        let cancel = self.reserve(&args.project_id, &args.job_id)?;
        view.state = AudioState::Running;
        view.error = None;
        if let Err(e) = write_json(&dir.join("status.json"), &view) {
            *self.active.lock().unwrap() = None;
            return Err(e);
        }
        self.launch(lease, models, runtime, dir, view.clone(), cancel);
        Ok(view)
    }
    fn launch(
        self: &Arc<Self>,
        lease: ProjectLease,
        models: Arc<ModelManager>,
        runtime: worker::Runtime,
        dir: PathBuf,
        mut view: AudioJobView,
        cancel: watch::Receiver<bool>,
    ) {
        let service = self.clone();
        tokio::spawn(async move {
            let result = worker::run(&lease, models, runtime, &dir, &mut view, cancel).await;
            match result {
                Ok(true) => view.state = AudioState::Succeeded,
                Ok(false) => view.state = AudioState::Paused,
                Err(e) => {
                    view.state = AudioState::Failed;
                    view.error = Some(e);
                }
            }
            // Keep the reservation until terminal state is persisted.
            if let Err(e) = write_json(&dir.join("status.json"), &view) {
                tracing::error!(?e, "Could not persist narration status");
            }
            *service.active.lock().unwrap() = None;
        });
    }
    pub fn export(
        &self,
        manager: &ProjectManager,
        args: &AudioExportArgs,
    ) -> Result<String, AppError> {
        let _lease = manager.lease(&args.project_id)?;
        let job = AudioJobArgs {
            project_id: args.project_id.clone(),
            job_id: args.job_id.clone(),
        };
        let view = self.read_view(&job)?;
        if view.state != AudioState::Succeeded {
            return Err(failure("audioNotFinished"));
        }
        let parent = Path::new(&args.destination)
            .canonicalize()
            .map_err(io_error)?;
        manager.validate_export_directory(&parent)?;
        if parent.starts_with(self.root.canonicalize().map_err(io_error)?) {
            return Err(AppError::invalid("destination"));
        }
        let output = parent.join(format!("audiobook-{}", &view.id[..8]));
        let dir = self.directory(&job)?;
        let input: Input = read_json(&dir.join("input.json"))?;
        if input.chapters.len() != view.total_chapters as usize {
            return Err(failure("audioStorage"));
        }
        std::fs::create_dir(&output).map_err(|e| {
            if e.kind() == std::io::ErrorKind::AlreadyExists {
                AppError::invalid("destinationExists")
            } else {
                io_error(e)
            }
        })?;
        let result = (|| {
            for i in 0..view.total_chapters {
                let filename = format!("{:05}.mp3", i + 1);
                let source = dir.join("audio").join(&filename);
                let meta = std::fs::symlink_metadata(&source).map_err(io_error)?;
                if !meta.is_file() || meta.file_type().is_symlink() || meta.len() == 0 {
                    return Err(failure("audioStorage"));
                }
                let receipt: serde_json::Value = read_json(&source.with_extension("json"))?;
                use sha2::{Digest, Sha256};
                let mut hash = Sha256::new();
                std::io::copy(
                    &mut std::fs::File::open(&source).map_err(io_error)?,
                    &mut hash,
                )
                .map_err(io_error)?;
                if receipt["sha256"].as_str() != Some(format!("{:x}", hash.finalize()).as_str()) {
                    return Err(failure("audioStorage"));
                }
                std::fs::copy(source, output.join(filename)).map_err(io_error)?;
            }
            let playlist = format!(
                "#EXTM3U\n{}",
                input
                    .chapters
                    .iter()
                    .enumerate()
                    .map(|(i, c)| format!(
                        "#EXTINF:-1,{}\n{:05}.mp3\n",
                        c.title.replace(['\n', '\r'], " "),
                        i + 1
                    ))
                    .collect::<String>()
            );
            std::fs::write(output.join("book.m3u8"), playlist).map_err(io_error)?;
            Ok(output.to_string_lossy().into_owned())
        })();
        if result.is_err() {
            let _ = std::fs::remove_dir_all(&output);
        }
        result
    }
}
