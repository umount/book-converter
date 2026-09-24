//! Explicit one-shot removal of positively identified obsolete test projects.
//! The application must stop and seal legacy writers before calling execute_reset.
use super::lifecycle::{real_directory, storage_error, ProjectManager};
use crate::app::contracts::AppError;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResetReport {
    pub removed_project_ids: Vec<String>,
    pub removed_files: Vec<PathBuf>,
    pub already_completed: bool,
}
const MARKER: &str = "legacy-reset-v1.json";

impl ProjectManager {
    pub fn reset_candidates(&self) -> Result<ResetReport, AppError> {
        if self.root.join(MARKER).exists() {
            return Ok(ResetReport {
                removed_project_ids: vec![],
                removed_files: vec![],
                already_completed: true,
            });
        }
        let mut report = ResetReport {
            removed_project_ids: vec![],
            removed_files: vec![],
            already_completed: false,
        };
        let projects = self.root.join("projects");
        if projects.exists() {
            real_directory(&projects)?;
            for entry in std::fs::read_dir(projects).map_err(storage_error)? {
                let entry = entry.map_err(storage_error)?;
                if !entry.file_type().map_err(storage_error)?.is_dir() {
                    continue;
                }
                let id = entry.file_name().to_string_lossy().into_owned();
                if crate::paths::validate_project_id(&id).is_err() {
                    continue;
                }
                let Ok(bytes) = std::fs::read(entry.path().join("project.json")) else {
                    continue;
                };
                let Ok(manifest) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
                    continue;
                };
                if manifest.get("format_version").is_some()
                    || manifest
                        .get("source_path")
                        .and_then(|v| v.as_str())
                        .is_none()
                {
                    continue;
                }
                let old = entry.path().join("progress.db");
                if std::fs::symlink_metadata(old)
                    .is_ok_and(|m| m.is_file() && !m.file_type().is_symlink())
                {
                    report.removed_project_ids.push(id);
                    report.removed_files.push(entry.path());
                }
            }
        }
        if self.root.exists() {
            real_directory(&self.root)?;
            for entry in std::fs::read_dir(&self.root).map_err(storage_error)? {
                let entry = entry.map_err(storage_error)?;
                if entry.file_type().map_err(storage_error)?.is_file()
                    && entry
                        .file_name()
                        .to_string_lossy()
                        .ends_with(".progress.db")
                {
                    report.removed_files.push(entry.path());
                    for suffix in ["-wal", "-shm"] {
                        let mut sidecar = entry.path().into_os_string();
                        sidecar.push(suffix);
                        let path = PathBuf::from(sidecar);
                        if std::fs::symlink_metadata(&path)
                            .is_ok_and(|m| m.is_file() && !m.file_type().is_symlink())
                        {
                            report.removed_files.push(path);
                        }
                    }
                }
            }
        }
        report.removed_project_ids.sort();
        report.removed_files.sort();
        Ok(report)
    }

    /// `quiesce` must cancel and join jobs/assistant tasks and prevent new legacy writes.
    /// No file is deleted if that barrier fails. Source files outside app data are never considered.
    pub fn execute_reset(
        &self,
        quiesce: impl FnOnce(&[String]) -> Result<(), AppError>,
    ) -> Result<ResetReport, AppError> {
        self.prepare()?;
        let _guard = self.imports.lock().unwrap_or_else(|p| p.into_inner());
        let report = self.reset_candidates()?;
        if report.already_completed {
            return Ok(report);
        }
        quiesce(&report.removed_project_ids)?;
        // Recheck the candidates after quiescence; do not delete a directory replaced by another format.
        let current = self.reset_candidates()?;
        if current.removed_files != report.removed_files {
            return Err(AppError::invalid("resetChanged"));
        }
        for path in &report.removed_files {
            let meta = std::fs::symlink_metadata(path).map_err(storage_error)?;
            if meta.file_type().is_symlink() {
                return Err(AppError::invalid("resetPath"));
            }
            if meta.is_dir() {
                std::fs::remove_dir_all(path).map_err(storage_error)?;
            } else {
                std::fs::remove_file(path).map_err(storage_error)?;
            }
        }
        let temporary = self
            .root
            .join(format!(".reset-{}.tmp", uuid::Uuid::new_v4()));
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(storage_error)?;
        file.write_all(&serde_json::to_vec(&report).map_err(storage_error)?)
            .map_err(storage_error)?;
        file.sync_all().map_err(storage_error)?;
        drop(file);
        std::fs::rename(&temporary, self.root.join(MARKER)).map_err(storage_error)?;
        Ok(report)
    }
}
