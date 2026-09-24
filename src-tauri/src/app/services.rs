//! Composition boundary. Services can be exercised without a webview or Tauri.
use super::contracts::{AppError, ProjectDescriptor};
use std::{path::Path, sync::Arc};

pub trait ProjectService: Send + Sync {
    fn inspect_manifest(&self, path: &Path) -> Result<ProjectDescriptor, AppError>;
}

pub struct FilesystemProjectService;

impl ProjectService for FilesystemProjectService {
    fn inspect_manifest(&self, path: &Path) -> Result<ProjectDescriptor, AppError> {
        crate::project::inspect_manifest(path)
    }
}

pub struct AppContext {
    pub models: Arc<crate::models::ModelManager>,
    pub projects: Arc<dyn ProjectService>,
    pub manager: Arc<crate::project::lifecycle::ProjectManager>,
    pub book_edits: Arc<crate::application::book_edit::EditPreviews>,
    pub book_jobs: Arc<crate::application::runtime::BookRuntime>,
}

impl Default for AppContext {
    fn default() -> Self {
        Self {
            models: Arc::new(crate::models::ModelManager::new(crate::paths::app_data_dir().join("models"))),
            projects: Arc::new(FilesystemProjectService),
            book_edits: Arc::new(crate::application::book_edit::EditPreviews::default()),
            book_jobs: Arc::new(crate::application::runtime::BookRuntime::default()),
            manager: Arc::new(crate::project::lifecycle::ProjectManager::new(
                crate::paths::app_data_dir(),
            )),
        }
    }
}

impl AppContext {
    pub fn inspect_manifest(&self, path: &str) -> Result<ProjectDescriptor, AppError> {
        if path.trim().is_empty() {
            return Err(AppError::invalid("path"));
        }
        self.projects.inspect_manifest(Path::new(path))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct FakeProjects;
    impl ProjectService for FakeProjects {
        fn inspect_manifest(&self, path: &Path) -> Result<ProjectDescriptor, AppError> {
            assert_eq!(path, Path::new("example/manifest.json"));
            Err(AppError::unsupported_version())
        }
    }

    #[test]
    fn injected_service_preserves_structured_errors() {
        let context = AppContext {
            projects: Arc::new(FakeProjects),
            ..AppContext::default()
        };
        assert_eq!(
            context.inspect_manifest("example/manifest.json"),
            Err(AppError::unsupported_version())
        );
        assert_eq!(
            context.inspect_manifest(" "),
            Err(AppError::invalid("path"))
        );
    }
}
