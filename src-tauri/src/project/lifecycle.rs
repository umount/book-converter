//! Staged imports and project leases. Published directories are the catalog authority.
use super::{Manifest, FORMAT_VERSION};
use crate::app::{
    contracts::{
        AppError, ErrorCode, ImportId, ProjectDescriptor, ProjectId, ProjectKind, SourceDescriptor,
    },
    requests::{DomainProgress, ImportPreview, ProjectChoices, ProjectSummary},
};
use crate::storage;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Condvar, Mutex,
    },
};

pub(super) fn storage_error(_: impl std::fmt::Display) -> AppError {
    AppError {
        code: ErrorCode::Storage,
        message_key: "errors.storage".into(),
        params: Default::default(),
        retryable: false,
    }
}
fn not_found() -> AppError {
    AppError {
        code: ErrorCode::NotFound,
        message_key: "errors.projectNotFound".into(),
        params: Default::default(),
        retryable: false,
    }
}
pub(super) fn now() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .to_string()
}
fn valid_id(id: &str) -> Result<(), AppError> {
    let parsed = uuid::Uuid::parse_str(id).map_err(|_| AppError::invalid("id"))?;
    if parsed.is_nil() || parsed.hyphenated().to_string() != id {
        return Err(AppError::invalid("id"));
    }
    Ok(())
}
pub(super) fn real_directory(path: &Path) -> Result<(), AppError> {
    let meta = std::fs::symlink_metadata(path).map_err(storage_error)?;
    if !meta.is_dir() || meta.file_type().is_symlink() {
        return Err(AppError::invalid("projectDirectory"));
    }
    Ok(())
}

#[derive(Default)]
struct GateState {
    active: usize,
    deleting: bool,
}
#[derive(Default)]
struct Gate {
    state: Mutex<GateState>,
    idle: Condvar,
    cancel: AtomicBool,
}

/// Hold this lease for the full lifetime of a job, including network work and writes.
pub struct ProjectLease {
    gate: Arc<Gate>,
    directory: PathBuf,
}
impl ProjectLease {
    pub fn cancelled(&self) -> bool {
        self.gate.cancel.load(Ordering::Acquire)
    }
    pub fn with_connection<T>(
        &self,
        work: impl FnOnce(&mut rusqlite::Connection, &Path) -> Result<T, AppError>,
    ) -> Result<T, AppError> {
        if self.cancelled() {
            return Err(AppError::invalid("projectClosing"));
        }
        let mut db = storage::open(&self.directory.join("project.db")).map_err(storage_error)?;
        work(&mut db, &self.directory)
    }
}
impl Drop for ProjectLease {
    fn drop(&mut self) {
        let mut state = self.gate.state.lock().unwrap_or_else(|p| p.into_inner());
        state.active -= 1;
        self.gate.idle.notify_all();
    }
}

pub struct ProjectManager {
    pub(super) root: PathBuf,
    gates: Mutex<HashMap<String, Arc<Gate>>>,
    pub(super) imports: Mutex<()>,
}
#[derive(Serialize, Deserialize)]
pub(super) struct Staged {
    pub manifest: Manifest,
    pub preview: ImportPreview,
}

impl ProjectManager {
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            gates: Mutex::new(HashMap::new()),
            imports: Mutex::new(()),
        }
    }
    pub(super) fn prepare(&self) -> Result<(), AppError> {
        std::fs::create_dir_all(&self.root).map_err(storage_error)?;
        real_directory(&self.root)?;
        for name in ["projects", "staging"] {
            let dir = self.root.join(name);
            std::fs::create_dir_all(&dir).map_err(storage_error)?;
            real_directory(&dir)?;
        }
        Ok(())
    }
    fn gate(&self, id: &str) -> Arc<Gate> {
        self.gates
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .entry(id.into())
            .or_default()
            .clone()
    }
    pub fn lease(&self, id: &ProjectId) -> Result<ProjectLease, AppError> {
        id.validate()?;
        let gate = self.gate(id.as_str());
        let mut state = gate.state.lock().unwrap_or_else(|p| p.into_inner());
        if state.deleting {
            return Err(AppError::invalid("projectClosing"));
        }
        let directory = self.root.join("projects").join(id.as_str());
        real_directory(&directory)?;
        state.active += 1;
        drop(state);
        Ok(ProjectLease { gate, directory })
    }
    pub fn inspect_source(
        &self,
        kind: ProjectKind,
        path: &Path,
    ) -> Result<ImportPreview, AppError> {
        self.prepare()?;
        let id = ProjectId::new();
        let import_id = uuid::Uuid::new_v4().to_string();
        let directory = self.root.join("staging").join(&import_id);
        std::fs::create_dir(&directory).map_err(storage_error)?;
        let result = (|| {
            let mut db = storage::create(&directory.join("project.db"), kind, "und")
                .map_err(storage_error)?;
            let (language, warnings) = super::import::normalize(kind, path, &directory, &mut db)?;
            db.execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")
                .map_err(storage_error)?;
            drop(db);
            let display_name = path
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("Source")
                .to_string();
            let source = SourceDescriptor {
                format: path
                    .extension()
                    .and_then(|s| s.to_str())
                    .unwrap_or("directory")
                    .to_ascii_lowercase(),
                display_name: display_name.clone(),
                original_path: Some(path.to_string_lossy().into_owned()),
            };
            let preview = ImportPreview {
                import_id: ImportId(import_id.clone()),
                kind,
                source: source.clone(),
                suggested_name: path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or(&display_name)
                    .to_string(),
                detected_language: language,
                warnings,
            };
            let manifest = Manifest {
                format_version: FORMAT_VERSION,
                id,
                kind,
                name: preview.suggested_name.clone(),
                created_at: now(),
                source,
                import_id: Some(import_id.clone()),
            };
            std::fs::write(
                directory.join("import.json"),
                serde_json::to_vec(&Staged {
                    manifest,
                    preview: preview.clone(),
                })
                .map_err(storage_error)?,
            )
            .map_err(storage_error)?;
            Ok(preview)
        })();
        if result.is_err() {
            let _ = std::fs::remove_dir_all(&directory);
        }
        result
    }
    pub fn create(
        &self,
        import_id: &str,
        choices: &ProjectChoices,
    ) -> Result<ProjectDescriptor, AppError> {
        valid_id(import_id)?;
        self.prepare()?;
        let _guard = self.imports.lock().unwrap_or_else(|p| p.into_inner());
        // The import receipt travels with the directory, so retry also survives a crash after rename.
        for entry in std::fs::read_dir(self.root.join("projects")).map_err(storage_error)? {
            let entry = entry.map_err(storage_error)?;
            if !entry.file_type().map_err(storage_error)?.is_dir() {
                continue;
            }
            if let Ok(bytes) = std::fs::read(entry.path().join("project.json")) {
                if let Ok(manifest) = Manifest::parse(&bytes) {
                    if manifest.import_id.as_deref() == Some(import_id) {
                        return self.open(&manifest.id);
                    }
                }
            }
        }
        if choices.name.trim().is_empty() || choices.languages.target.trim().is_empty() {
            return Err(AppError::invalid("choices"));
        }
        let directory = self.root.join("staging").join(import_id);
        real_directory(&directory)?;
        let mut staged: Staged = serde_json::from_slice(
            &std::fs::read(directory.join("import.json")).map_err(|_| not_found())?,
        )
        .map_err(storage_error)?;
        let mut db = storage::open(&directory.join("project.db")).map_err(storage_error)?;
        let mut settings = storage::shared::settings(&db)?.choices;
        settings.source_language = choices
            .languages
            .source
            .clone()
            .or(staged.preview.detected_language);
        settings.target_language = choices.languages.target.clone();
        match staged.manifest.kind {
            ProjectKind::Book => {
                settings.book_translation_profile = choices.processing_profile_id.clone()
            }
            ProjectKind::Manga => {
                settings.manga_recognition_profile = choices.processing_profile_id.clone();
                settings.manga_translation_profile = choices.processing_profile_id.clone();
            }
        }
        let revision = storage::shared::settings(&db)?.revision;
        storage::shared::update_settings(&mut db, &revision, &settings)?;
        db.execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")
            .map_err(storage_error)?;
        drop(db);
        staged.manifest.name = choices.name.clone();
        let bytes = serde_json::to_vec(&staged.manifest).map_err(storage_error)?;
        Manifest::parse(&bytes)?;
        std::fs::write(directory.join("project.json"), bytes).map_err(storage_error)?;
        let published = self.root.join("projects").join(staged.manifest.id.as_str());
        if published.exists() {
            return Err(AppError::invalid("projectExists"));
        }
        std::fs::rename(&directory, &published).map_err(storage_error)?;
        // Keep only the manifest's receipt, not the import preview containing temporary decisions.
        let _ = std::fs::remove_file(published.join("import.json"));
        Ok(staged.manifest.descriptor())
    }
    pub fn cancel_import(&self, id: &str) -> Result<(), AppError> {
        valid_id(id)?;
        let _guard = self.imports.lock().unwrap_or_else(|p| p.into_inner());
        let directory = self.root.join("staging").join(id);
        if directory.exists() {
            real_directory(&directory)?;
            std::fs::remove_dir_all(directory).map_err(storage_error)?;
        }
        Ok(())
    }
    pub fn open(&self, id: &ProjectId) -> Result<ProjectDescriptor, AppError> {
        let lease = self.lease(id)?;
        let descriptor = super::inspect_manifest(&lease.directory.join("project.json"))?;
        if descriptor.id != *id {
            return Err(AppError::invalid("projectIdentity"));
        }
        lease.with_connection(|db, _| {
            storage::repository::ProjectRepository::new(db, descriptor.kind)?;
            Ok(())
        })?;
        Ok(descriptor)
    }
    pub fn catalog(&self) -> Result<Vec<ProjectSummary>, AppError> {
        let root = self.root.join("projects");
        if !root.exists() {
            return Ok(vec![]);
        }
        real_directory(&root)?;
        let mut result = Vec::new();
        for entry in std::fs::read_dir(root).map_err(storage_error)? {
            let entry = entry.map_err(storage_error)?;
            if !entry.file_type().map_err(storage_error)?.is_dir() {
                continue;
            }
            let Ok(id) = serde_json::from_value::<ProjectId>(serde_json::Value::String(
                entry.file_name().to_string_lossy().into_owned(),
            )) else {
                continue;
            };
            let Ok(descriptor) = self.open(&id) else {
                continue;
            };
            let lease = self.lease(&id)?;
            let progress=lease.with_connection(|db,_|{
                let count=|sql:&str|db.query_row(sql,[],|r|r.get::<_,u32>(0)).map_err(storage_error);
                Ok(match descriptor.kind {
                    ProjectKind::Book=>DomainProgress::Book{chapters:count("SELECT COUNT(*) FROM book_chapters")?,translated:count("SELECT COUNT(DISTINCT chapter_id) FROM book_translations WHERE status='ready'")?},
                    ProjectKind::Manga=>DomainProgress::Manga{pages:count("SELECT COUNT(*) FROM manga_pages")?,lettered:count("SELECT COUNT(DISTINCT page_id) FROM manga_results WHERE stage='lettering' AND validity='current'")?,approved:count("SELECT COUNT(DISTINCT page_id) FROM manga_results JOIN manga_reviews ON manga_results.id=manga_reviews.result_id WHERE stage='lettering' AND validity='current' AND state='approved'")?},
                })
            })?;
            result.push(ProjectSummary {
                descriptor,
                progress,
            });
        }
        result.sort_by(|a, b| {
            a.descriptor
                .name
                .cmp(&b.descriptor.name)
                .then_with(|| a.descriptor.id.as_str().cmp(b.descriptor.id.as_str()))
        });
        Ok(result)
    }
    /// Public exports belong outside application storage, including other projects.
    pub fn validate_export_directory(&self, directory: &Path) -> Result<(), AppError> {
        let directory = directory.canonicalize().map_err(storage_error)?;
        let root = self.root.canonicalize().map_err(storage_error)?;
        if directory.starts_with(root) { return Err(AppError::invalid("destination")); }
        Ok(())
    }

    pub fn delete(&self, id: &ProjectId) -> Result<(), AppError> {
        id.validate()?;
        let gate = self.gate(id.as_str());
        let mut state = gate.state.lock().unwrap_or_else(|p| p.into_inner());
        state.deleting = true;
        gate.cancel.store(true, Ordering::Release);
        while state.active > 0 {
            state = gate.idle.wait(state).unwrap_or_else(|p| p.into_inner());
        }
        let directory = self.root.join("projects").join(id.as_str());
        if directory.exists() {
            real_directory(&directory)?;
            std::fs::remove_dir_all(directory).map_err(storage_error)?;
        }
        Ok(())
    }
}
