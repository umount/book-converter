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
    pub projects: Arc<dyn ProjectService>,
}

impl Default for AppContext {
    fn default() -> Self {
        Self {
            projects: Arc::new(FilesystemProjectService),
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
