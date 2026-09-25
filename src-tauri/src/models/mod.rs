//! Explicit on-demand, resumable model downloads. Never executes downloaded code.
mod catalog;
mod transfer;
pub use catalog::ModelSpec;
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
use tokio::sync::{watch, OnceCell};
use ts_rs::TS;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, TS, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ModelStatus {
    Missing,
    Paused,
    Downloading,
    Verifying,
    Downloaded,
    Failed,
}
#[derive(Clone, Copy, Debug, Serialize, TS, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ModelFailure {
    UnknownModel,
    Busy,
    Network,
    SizeMismatch,
    ChecksumMismatch,
    Storage,
    UnsafePath,
}
#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ModelView {
    pub model: ModelSpec,
    pub status: ModelStatus,
    pub downloaded_bytes: u32,
    pub failure: Option<ModelFailure>,
}
#[derive(Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ModelArgs {
    pub model_id: String,
}
struct Active {
    id: String,
    cancel: watch::Sender<bool>,
}
struct Inner {
    views: Vec<ModelView>,
    active: Option<Active>,
}
pub struct ModelManager {
    root: PathBuf,
    inner: Mutex<Inner>,
    initialized: OnceCell<()>,
}
impl ModelManager {
    pub fn new(root: PathBuf) -> Self {
        Self::with_catalog(root, catalog::catalog())
    }
    fn with_catalog(root: PathBuf, specs: Vec<ModelSpec>) -> Self {
        Self {
            root,
            initialized: OnceCell::new(),
            inner: Mutex::new(Inner {
                active: None,
                views: specs
                    .into_iter()
                    .map(|model| ModelView {
                        model,
                        status: ModelStatus::Missing,
                        downloaded_bytes: 0,
                        failure: None,
                    })
                    .collect(),
            }),
        }
    }
    async fn initialize(self: &Arc<Self>) -> Result<(), ModelFailure> {
        self.initialized
            .get_or_try_init(|| async {
                let manager = self.clone();
                let views = tokio::task::spawn_blocking(move || {
                    let mut views = manager.inner.lock().unwrap().views.clone();
                    check_directory(&manager.root)?;
                    for view in &mut views {
                        let dir = manager.root.join(view.model.directory());
                        check_directory(&dir)?;
                        let installed = dir.join(view.model.artifact_name());
                        if regular_size(&installed)?.is_some() {
                            match transfer::verify(&installed, &view.model, None) {
                                Ok(()) => {
                                    view.status = ModelStatus::Downloaded;
                                    view.downloaded_bytes = view.model.bytes;
                                }
                                Err(error) => {
                                    view.status = ModelStatus::Failed;
                                    view.failure = Some(error);
                                }
                            }
                        } else if let Some(bytes) =
                            regular_size(&dir.join(view.model.partial_name()))?
                        {
                            view.status = ModelStatus::Paused;
                            view.downloaded_bytes = bytes.min(u64::from(view.model.bytes)) as u32;
                        }
                    }
                    Ok::<_, ModelFailure>(views)
                })
                .await
                .map_err(|_| ModelFailure::Storage)??;
                self.inner.lock().unwrap().views = views;
                Ok(())
            })
            .await
            .map(|_| ())
    }
    pub async fn list(self: &Arc<Self>) -> Result<Vec<ModelView>, ModelFailure> {
        self.initialize().await?;
        Ok(self.inner.lock().unwrap().views.clone())
    }
    /// Fast UI readiness only. Admission still initializes and verifies pinned hashes.
    pub fn artifact_present(&self, id: &str) -> bool {
        let Ok(inner)=self.inner.lock() else {return false};
        let Some(view)=inner.views.iter().find(|v|v.model.id==id) else {return false};
        if view.status==ModelStatus::Failed {return false;}
        let directory=self.root.join(view.model.directory());
        check_directory(&self.root).is_ok() && check_directory(&directory).is_ok()
            && regular_size(&directory.join(view.model.artifact_name())).ok().flatten()==Some(u64::from(view.model.bytes))
    }
    /// Admission only; the isolated worker rechecks pinned bytes/hash at model load.
    pub fn downloaded_artifact(&self, id: &str) -> Option<(ModelSpec, PathBuf)> {
        let inner = self.inner.lock().ok()?;
        let view = inner
            .views
            .iter()
            .find(|view| view.model.id == id && view.status == ModelStatus::Downloaded)?;
        Some((
            view.model.clone(),
            self.root
                .join(view.model.directory())
                .join(view.model.artifact_name()),
        ))
    }
    pub async fn start(self: &Arc<Self>, id: &str) -> Result<(), ModelFailure> {
        self.initialize().await?;
        let mut inner = self.inner.lock().unwrap();
        if inner.active.is_some() {
            return Err(ModelFailure::Busy);
        }
        let view = inner
            .views
            .iter_mut()
            .find(|v| v.model.id == id)
            .ok_or(ModelFailure::UnknownModel)?;
        if view.status == ModelStatus::Downloaded {
            return Ok(());
        }
        let spec = view.model.clone();
        view.status = ModelStatus::Downloading;
        view.failure = None;
        let (cancel, receiver) = watch::channel(false);
        inner.active = Some(Active {
            id: id.into(),
            cancel,
        });
        let manager = self.clone();
        tokio::spawn(async move {
            let result = transfer::download(manager.clone(), &spec, receiver, spec.url()).await;
            manager.finish(&spec.id, result);
        });
        Ok(())
    }
    pub fn pause(&self, id: &str) -> Result<(), ModelFailure> {
        let inner = self.inner.lock().unwrap();
        if !inner.views.iter().any(|v| v.model.id == id) {
            return Err(ModelFailure::UnknownModel);
        }
        if let Some(active) = inner.active.as_ref().filter(|a| a.id == id) {
            let _ = active.cancel.send(true);
        }
        Ok(())
    }
    pub async fn remove(self: &Arc<Self>, id: &str) -> Result<(), ModelFailure> {
        self.initialize().await?;
        let mut inner = self.inner.lock().unwrap();
        if inner.active.as_ref().is_some_and(|a| a.id == id) {
            return Err(ModelFailure::Busy);
        }
        let view = inner
            .views
            .iter_mut()
            .find(|v| v.model.id == id)
            .ok_or(ModelFailure::UnknownModel)?;
        let dir = self.root.join(view.model.directory());
        check_directory(&self.root)?;
        check_directory(&dir)?;
        for name in [
            view.model.artifact_name().to_string(),
            view.model.partial_name(),
        ] {
            let path = dir.join(name);
            if regular_size(&path)?.is_some() {
                std::fs::remove_file(path).map_err(|_| ModelFailure::Storage)?;
            }
        }
        view.status = ModelStatus::Missing;
        view.downloaded_bytes = 0;
        view.failure = None;
        Ok(())
    }
    fn progress(&self, id: &str, status: ModelStatus, bytes: u32) {
        let mut inner = self.inner.lock().unwrap();
        if let Some(view) = inner.views.iter_mut().find(|v| v.model.id == id) {
            view.status = status;
            view.downloaded_bytes = bytes;
        }
    }
    fn finish(&self, id: &str, result: Result<bool, ModelFailure>) {
        let mut inner = self.inner.lock().unwrap();
        if let Some(view) = inner.views.iter_mut().find(|v| v.model.id == id) {
            match result {
                Ok(true) => {
                    view.status = ModelStatus::Downloaded;
                    view.downloaded_bytes = view.model.bytes;
                }
                Ok(false) => view.status = ModelStatus::Paused,
                Err(error) => {
                    view.status = ModelStatus::Failed;
                    view.failure = Some(error);
                }
            }
        }
        inner.active = None;
    }
}
fn check_directory(path: &Path) -> Result<(), ModelFailure> {
    match std::fs::symlink_metadata(path) {
        Ok(meta) if meta.is_dir() && !meta.file_type().is_symlink() => Ok(()),
        Ok(_) => Err(ModelFailure::UnsafePath),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err(ModelFailure::Storage),
    }
}
fn regular_size(path: &Path) -> Result<Option<u64>, ModelFailure> {
    match std::fs::symlink_metadata(path) {
        Ok(meta) if meta.is_file() && !meta.file_type().is_symlink() => Ok(Some(meta.len())),
        Ok(_) => Err(ModelFailure::UnsafePath),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err(ModelFailure::Storage),
    }
}
pub fn typescript() -> String {
    let config = ts_rs::Config::default();
    [
        ModelSpec::decl(&config),
        ModelStatus::decl(&config),
        ModelFailure::decl(&config),
        ModelView::decl(&config),
        ModelArgs::decl(&config),
    ]
    .into_iter()
    .map(|v| format!("export {v}\n"))
    .collect()
}

#[cfg(test)]
mod tests;
