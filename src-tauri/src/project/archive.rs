//! Portable version-1 archives: SQLite snapshot plus registered immutable assets only.
use super::{
    lifecycle::{now, real_directory, storage_error, ProjectManager},
    Manifest,
};
use crate::{
    app::contracts::{AppError, ProjectDescriptor, ProjectId},
    storage,
};
use sha2::{Digest, Sha256};
use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
};
const MAX_ARCHIVE_BYTES: u64 = 20 * 1024 * 1024 * 1024;

fn allowed(name: &str) -> bool {
    if ["project.json", "project.db"].contains(&name) {
        return true;
    }
    let Some(asset) = name.strip_prefix("assets/") else {
        return false;
    };
    let Some((hash, extension)) = asset.split_once('.') else {
        return false;
    };
    hash.len() == 64
        && hash
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        && ["png", "jpg", "webp", "gif"].contains(&extension)
}

/// Verify registered files before a project can be published from an archive.
pub(super) fn validate_assets(
    db: &rusqlite::Connection,
    directory: &Path,
) -> Result<Vec<String>, AppError> {
    let mut query = db
        .prepare("SELECT id,relative_path,byte_length FROM assets ORDER BY id")
        .map_err(storage_error)?;
    let entries = query
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, u64>(2)?,
            ))
        })
        .map_err(storage_error)?;
    let mut result = Vec::new();
    for entry in entries {
        let (id, path, size) = entry.map_err(storage_error)?;
        if !allowed(&path) || !path.starts_with(&format!("assets/{id}.")) {
            return Err(AppError::invalid("assetPath"));
        }
        let full = directory.join(&path);
        if std::fs::symlink_metadata(&full)
            .map_err(storage_error)?
            .file_type()
            .is_symlink()
        {
            return Err(AppError::invalid("assetPath"));
        }
        let mut file = std::fs::File::open(full).map_err(storage_error)?;
        if file.metadata().map_err(storage_error)?.len() != size {
            return Err(AppError::invalid("assetSize"));
        }
        let mut hash = Sha256::new();
        let mut buffer = [0u8; 64 * 1024];
        loop {
            let read = file.read(&mut buffer).map_err(storage_error)?;
            if read == 0 {
                break;
            }
            hash.update(&buffer[..read]);
        }
        if format!("{:x}", hash.finalize()) != id {
            return Err(AppError::invalid("assetHash"));
        }
        result.push(path);
    }
    Ok(result)
}

struct Cleanup(PathBuf);
impl Drop for Cleanup {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

impl ProjectManager {
    pub fn export_archive(&self, id: &ProjectId, destination: &Path) -> Result<(), AppError> {
        self.prepare()?;
        let lease = self.lease(id)?;
        let temp = self
            .root
            .join("staging")
            .join(uuid::Uuid::new_v4().to_string());
        std::fs::create_dir(&temp).map_err(storage_error)?;
        let _cleanup = Cleanup(temp.clone());
        let snapshot = temp.join("project.db");
        let (manifest, assets, directory) = lease.with_connection(|db, directory| {
            db.execute("VACUUM INTO ?1", [snapshot.to_string_lossy().as_ref()])
                .map_err(storage_error)?;
            let copy = storage::open(&snapshot).map_err(storage_error)?;
            let assets = validate_assets(&copy, directory)?;
            Ok((
                std::fs::read(directory.join("project.json")).map_err(storage_error)?,
                assets,
                directory.to_path_buf(),
            ))
        })?;
        Manifest::parse(&manifest)?;
        // A failed export cannot truncate an existing archive selected by the user.
        let parent = destination
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let temporary = parent.join(format!(".{}.bcproj.tmp", uuid::Uuid::new_v4()));
        let result = (|| {
            let file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temporary)
                .map_err(storage_error)?;
            let mut zip = zip::ZipWriter::new(file);
            let options = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated);
            zip.start_file("project.json", options)
                .map_err(storage_error)?;
            zip.write_all(&manifest).map_err(storage_error)?;
            zip.start_file("project.db", options)
                .map_err(storage_error)?;
            std::io::copy(
                &mut std::fs::File::open(&snapshot).map_err(storage_error)?,
                &mut zip,
            )
            .map_err(storage_error)?;
            for path in assets {
                zip.start_file(&path, options).map_err(storage_error)?;
                std::io::copy(
                    &mut std::fs::File::open(directory.join(path)).map_err(storage_error)?,
                    &mut zip,
                )
                .map_err(storage_error)?;
            }
            zip.finish()
                .map_err(storage_error)?
                .sync_all()
                .map_err(storage_error)?;
            std::fs::rename(&temporary, destination).map_err(storage_error)
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(temporary);
        }
        result
    }

    pub fn import_archive(&self, path: &Path) -> Result<ProjectDescriptor, AppError> {
        self.prepare()?;
        let _guard = self.imports.lock().unwrap_or_else(|p| p.into_inner());
        let directory = self
            .root
            .join("staging")
            .join(uuid::Uuid::new_v4().to_string());
        std::fs::create_dir(&directory).map_err(storage_error)?;
        let _cleanup = Cleanup(directory.clone());
        let mut zip = zip::ZipArchive::new(std::fs::File::open(path).map_err(storage_error)?)
            .map_err(storage_error)?;
        if zip.len() > 100_000 {
            return Err(AppError::invalid("archiveEntries"));
        }
        let mut names = std::collections::HashSet::new();
        let mut total = 0u64;
        for index in 0..zip.len() {
            let mut entry = zip.by_index(index).map_err(storage_error)?;
            let name = entry.name().to_string();
            if entry.is_dir()
                || !allowed(&name)
                || !names.insert(name.clone())
                || entry.unix_mode().is_some_and(|m| m & 0o170000 == 0o120000)
            {
                return Err(AppError::invalid("archiveEntry"));
            }
            total = total
                .checked_add(entry.size())
                .ok_or_else(|| AppError::invalid("archiveSize"))?;
            if total > MAX_ARCHIVE_BYTES {
                return Err(AppError::invalid("archiveSize"));
            }
            let target = directory.join(&name);
            std::fs::create_dir_all(target.parent().expect("staging parent"))
                .map_err(storage_error)?;
            let mut output = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(target)
                .map_err(storage_error)?;
            let declared = entry.size();
            let copied = std::io::copy(&mut (&mut entry).take(declared + 1), &mut output)
                .map_err(storage_error)?;
            if copied != declared {
                return Err(AppError::invalid("archiveSize"));
            }
            output.sync_all().map_err(storage_error)?;
        }
        let mut manifest = Manifest::parse(
            &std::fs::read(directory.join("project.json")).map_err(storage_error)?,
        )?;
        real_directory(&directory)?;
        let mut db = storage::open(&directory.join("project.db")).map_err(storage_error)?;
        storage::repository::ProjectRepository::new(&mut db, manifest.kind)?;
        let integrity: String = db
            .query_row("PRAGMA integrity_check", [], |r| r.get(0))
            .map_err(storage_error)?;
        if integrity != "ok"
            || db
                .prepare("PRAGMA foreign_key_check")
                .map_err(storage_error)?
                .exists([])
                .map_err(storage_error)?
        {
            return Err(AppError::invalid("archiveDatabase"));
        }
        validate_assets(&db, &directory)?;
        storage::runs::interrupt_running(&mut db, &now())?;
        db.execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")
            .map_err(storage_error)?;
        drop(db);
        manifest.id = ProjectId::new();
        manifest.import_id = None;
        std::fs::write(
            directory.join("project.json"),
            serde_json::to_vec(&manifest).map_err(storage_error)?,
        )
        .map_err(storage_error)?;
        std::fs::rename(
            &directory,
            self.root.join("projects").join(manifest.id.as_str()),
        )
        .map_err(storage_error)?;
        Ok(manifest.descriptor())
    }
}
