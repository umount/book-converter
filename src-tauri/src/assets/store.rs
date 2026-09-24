//! Immutable content-addressed image publication shared by both domains.
use sha2::{Digest, Sha256};
use std::{
    io::Write,
    path::{Path, PathBuf},
};

pub const DIRECTORY: &str = "assets";

pub struct AssetStore {
    directory: PathBuf,
}

impl AssetStore {
    pub fn new(project_directory: &Path) -> anyhow::Result<Self> {
        let directory = project_directory.join(DIRECTORY);
        std::fs::create_dir_all(&directory)?;
        anyhow::ensure!(
            !std::fs::symlink_metadata(&directory)?
                .file_type()
                .is_symlink(),
            "Asset directory cannot be a symlink"
        );
        Ok(Self { directory })
    }

    pub fn publish(
        &self,
        connection: &rusqlite::Connection,
        bytes: &[u8],
        extension: &str,
    ) -> anyhow::Result<String> {
        let mime = match extension {
            "png" => "image/png",
            "jpg" => "image/jpeg",
            "webp" => "image/webp",
            "gif" => "image/gif",
            _ => anyhow::bail!("Unsupported asset extension"),
        };
        anyhow::ensure!(!bytes.is_empty(), "Empty asset");
        let format = image::guess_format(bytes)?;
        anyhow::ensure!(
            format.to_mime_type() == mime,
            "Asset extension does not match image content"
        );
        let (width, height) = image::ImageReader::with_format(std::io::Cursor::new(bytes), format)
            .into_dimensions()?;
        anyhow::ensure!(
            width > 0 && height > 0 && u64::from(width) * u64::from(height) <= 100_000_000,
            "Image dimensions exceed the limit"
        );
        let id = format!("{:x}", Sha256::digest(bytes));
        let name = format!("{id}.{extension}");
        let target = self.directory.join(&name);
        let temporary = self.directory.join(format!("{}.tmp", uuid::Uuid::new_v4()));
        let result = (|| -> anyhow::Result<()> {
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temporary)?;
            file.write_all(bytes)?;
            file.sync_all()?;
            drop(file);
            // Hard-link publication is atomic and never replaces an existing asset.
            match std::fs::hard_link(&temporary, &target) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    anyhow::ensure!(
                        !std::fs::symlink_metadata(&target)?.file_type().is_symlink(),
                        "Asset cannot be a symlink"
                    );
                    anyhow::ensure!(
                        std::fs::read(&target)? == bytes,
                        "Existing asset content mismatch"
                    );
                }
                Err(error) => return Err(error.into()),
            }
            connection.execute("INSERT INTO assets(id,relative_path,mime,byte_length,width,height) VALUES(?1,?2,?3,?4,?5,?6) ON CONFLICT(id) DO NOTHING",
                (&id, format!("{DIRECTORY}/{name}"), mime, i64::try_from(bytes.len())?, width, height))?;
            Ok(())
        })();
        let _ = std::fs::remove_file(&temporary);
        result?;
        Ok(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn publication_is_immutable_and_deduplicated() {
        let root = std::env::temp_dir().join(format!("assets-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let db = crate::storage::create(
            &root.join("project.db"),
            crate::app::contracts::ProjectKind::Manga,
            "ru",
        )
        .unwrap();
        let store = AssetStore::new(&root).unwrap();
        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(include_bytes!(
            "../../../tests/fixtures/structural.epub"
        )))
        .unwrap();
        let mut bytes = Vec::new();
        std::io::Read::read_to_end(&mut archive.by_name("OEBPS/plate.png").unwrap(), &mut bytes)
            .unwrap();
        let first = store.publish(&db, &bytes, "png").unwrap();
        assert_eq!(store.publish(&db, &bytes, "png").unwrap(), first);
        assert!(store.publish(&db, b"x", "../png").is_err());
        assert_eq!(
            db.query_row("SELECT COUNT(*) FROM assets", [], |r| r.get::<_, u32>(0))
                .unwrap(),
            1
        );
        assert_eq!(std::fs::read_dir(root.join(DIRECTORY)).unwrap().count(), 1);
        drop(db);
        std::fs::remove_dir_all(root).unwrap();
    }
}
