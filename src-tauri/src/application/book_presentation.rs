//! Human-owned book details are independent of generated metadata.
use crate::{
    app::{
        contracts::{AppError, Revision},
        requests::{BookPresentation, UpdateBookPresentationArgs},
    },
    assets::store::AssetStore,
    storage::repository::{conflict, storage_error},
};
use rusqlite::{Connection, OptionalExtension};
use std::{io::Read, path::Path};

pub fn read(db: &Connection) -> Result<BookPresentation, AppError> {
    let kind: String = db
        .query_row("SELECT kind FROM project_settings", [], |r| r.get(0))
        .map_err(storage_error)?;
    if kind != "book" {
        return Err(AppError::invalid("projectKind"));
    }
    let source = db.query_row("SELECT title,author,summary FROM book_source_metadata WHERE singleton=1",[],|r|Ok((r.get::<_,Option<String>>(0)?,r.get::<_,Option<String>>(1)?,r.get::<_,Option<String>>(2)?))).optional().map_err(storage_error)?.unwrap_or_default();
    let mut value = db.query_row("SELECT title,author,summary,instructions,cover_asset_id,revision FROM book_presentation WHERE singleton=1", [], |r| Ok(BookPresentation {
        source_title:None,source_author:None,source_summary:None,
        title:r.get(0)?, author:r.get(1)?, summary:r.get(2)?, instructions:r.get(3)?, cover_asset_id:r.get(4)?, revision:Revision(r.get::<_,i64>(5)?.to_string()),
    })).optional().map_err(storage_error)?.unwrap_or(BookPresentation {source_title:None,source_author:None,source_summary:None,title:None,author:None,summary:None,instructions:String::new(),cover_asset_id:None,revision:Revision("0".into())});
    value.source_title=source.0; value.source_author=source.1; value.source_summary=source.2;
    Ok(value)
}
fn ensure_row(db: &Connection) -> Result<(), AppError> {
    db.execute(
        "INSERT OR IGNORE INTO book_presentation(singleton) VALUES(1)",
        [],
    )
    .map_err(storage_error)?;
    Ok(())
}
pub fn update(
    db: &mut Connection,
    args: &UpdateBookPresentationArgs,
) -> Result<BookPresentation, AppError> {
    if args.title.as_ref().is_some_and(|s| s.len() > 4096)
        || args.author.as_ref().is_some_and(|s| s.len() > 4096)
        || args.summary.as_ref().is_some_and(|s| s.len() > 32768)
        || args.instructions.len() > 32768
    {
        return Err(AppError::invalid("bookDetails"));
    }
    let tx = db.transaction().map_err(storage_error)?;
    let old = read(&tx)?;
    args.expected_revision.value()?;
    if old.revision != args.expected_revision {
        return Err(conflict());
    }
    ensure_row(&tx)?;
    if old.instructions != args.instructions {
        tx.execute(
            "UPDATE project_settings SET revision=revision+1 WHERE singleton=1",
            [],
        )
        .map_err(storage_error)?;
        tx.execute(
            "UPDATE book_translations SET status='needs_review' WHERE status='ready' AND provenance!='reference'",
            [],
        )
        .map_err(storage_error)?;
    }
    tx.execute("UPDATE book_presentation SET title=?1,author=?2,summary=?3,instructions=?4,revision=revision+1 WHERE singleton=1",rusqlite::params![args.title,args.author,args.summary,args.instructions]).map_err(storage_error)?;
    let result = read(&tx)?;
    tx.commit().map_err(storage_error)?;
    Ok(result)
}
pub fn cover(
    db: &mut Connection,
    directory: &Path,
    path: Option<&str>,
    expected: &Revision,
) -> Result<BookPresentation, AppError> {
    let bytes = path
        .map(|path| -> Result<Vec<u8>, AppError> {
            let file = std::fs::File::open(path).map_err(|_| AppError::invalid("coverFile"))?;
            if !file
                .metadata()
                .map_err(|_| AppError::invalid("coverFile"))?
                .is_file()
            {
                return Err(AppError::invalid("coverFile"));
            }
            let mut bytes = Vec::new();
            file.take(20 * 1024 * 1024 + 1)
                .read_to_end(&mut bytes)
                .map_err(|_| AppError::invalid("coverFile"))?;
            if bytes.len() > 20 * 1024 * 1024 {
                return Err(AppError::invalid("coverSize"));
            }
            Ok(bytes)
        })
        .transpose()?;
    let tx = db.transaction().map_err(storage_error)?;
    expected.value()?;
    if read(&tx)?.revision != *expected {
        return Err(conflict());
    }
    let id = bytes
        .map(|b| publish_cover(&tx, directory, &b))
        .transpose()?;
    ensure_row(&tx)?;
    tx.execute(
        "UPDATE book_presentation SET cover_asset_id=?1,revision=revision+1 WHERE singleton=1",
        [id],
    )
    .map_err(storage_error)?;
    let result = read(&tx)?;
    tx.commit().map_err(storage_error)?;
    Ok(result)
}
pub fn publish_cover(db: &Connection, directory: &Path, bytes: &[u8]) -> Result<String, AppError> {
    let ext = match image::guess_format(bytes).map_err(|_| AppError::invalid("coverFormat"))? {
        image::ImageFormat::Png => "png",
        image::ImageFormat::Jpeg => "jpg",
        image::ImageFormat::WebP => "webp",
        image::ImageFormat::Gif => "gif",
        _ => return Err(AppError::invalid("coverFormat")),
    };
    AssetStore::new(directory)
        .and_then(|s| s.publish(db, bytes, ext))
        .map_err(|_| AppError::invalid("coverFile"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::contracts::{ProjectId, ProjectKind};
    #[test]
    fn manual_details_cover_and_prompt_are_guarded_and_reopenable() {
        let root = std::env::temp_dir().join(format!("presentation-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("project.db");
        let mut db = crate::storage::create(&path, ProjectKind::Book, "ru").unwrap();
        let args = UpdateBookPresentationArgs {
            project_id: ProjectId::new(),
            title: Some("Manual title".into()),
            author: Some("Author".into()),
            summary: Some("Summary".into()),
            instructions: "Keep names consistent".into(),
            expected_revision: Revision("0".into()),
        };
        let details = update(&mut db, &args).unwrap();
        assert_eq!(details.revision, Revision("1".into()));
        assert!(update(&mut db, &args).is_err());
        assert_eq!(
            crate::storage::shared::settings(&db).unwrap().revision,
            Revision("1".into())
        );
        let image_path = root.join("cover.png");
        image::RgbImage::new(2, 3).save(&image_path).unwrap();
        let original = std::fs::read(&image_path).unwrap();
        let with_cover = cover(&mut db, &root, image_path.to_str(), &details.revision).unwrap();
        assert!(with_cover.cover_asset_id.is_some());
        assert_eq!(std::fs::read(&image_path).unwrap(), original);
        assert!(cover(&mut db, &root, None, &details.revision).is_err());
        assert_eq!(
            crate::storage::shared::settings(&db).unwrap().revision,
            Revision("1".into())
        );
        drop(db);
        let mut reopened = crate::storage::open(&path).unwrap();
        assert_eq!(read(&reopened).unwrap(), with_cover);
        assert!(cover(&mut reopened, &root, None, &with_cover.revision)
            .unwrap()
            .cover_asset_id
            .is_none());
        // Opening a prior version-1 project installs the additive extension.
        reopened
            .execute_batch("DROP TABLE book_presentation")
            .unwrap();
        drop(reopened);
        let reopened = crate::storage::open(&path).unwrap();
        assert_eq!(read(&reopened).unwrap().revision, Revision("0".into()));
        drop(reopened);
        let manga =
            crate::storage::create(&root.join("manga.db"), ProjectKind::Manga, "ru").unwrap();
        assert!(read(&manga).is_err());
        drop(manga);
        std::fs::remove_dir_all(root).unwrap();
    }
}
