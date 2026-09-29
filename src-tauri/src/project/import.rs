//! Normalize source containers into the independent book store.
use crate::{
    app::{
        contracts::{AppError, BookBlockContent, BookBlockView, ChapterId, ProjectKind, Revision},
        requests::ChapterSummary,
    },
    assets::store::AssetStore,
    storage::repository::{storage_error, ProjectRepository},
};
use rusqlite::Connection;
use std::{collections::HashMap, io::Read, path::Path};
const MAX_IMAGE_BYTES: u64 = 64 * 1024 * 1024;

pub(super) fn normalize(
    kind: ProjectKind,
    path: &Path,
    directory: &Path,
    db: &mut Connection,
    _progress: &mut dyn FnMut(crate::app::requests::ImportProgress),
) -> Result<(Option<String>, Vec<String>), AppError> {
    match kind {
        ProjectKind::Book => book(path, directory, db),
    }
}
fn fail(_: impl std::fmt::Display) -> AppError {
    AppError::invalid("source")
}
fn uuid() -> String {
    uuid::Uuid::new_v4().to_string()
}
fn extension(bytes: &[u8]) -> Result<&'static str, AppError> {
    Ok(match image::guess_format(bytes).map_err(fail)? {
        image::ImageFormat::Png => "png",
        image::ImageFormat::Jpeg => "jpg",
        image::ImageFormat::Gif => "gif",
        image::ImageFormat::WebP => "webp",
        _ => return Err(AppError::invalid("imageFormat")),
    })
}
fn read_image(mut input: impl Read) -> Result<Vec<u8>, AppError> {
    let mut bytes = Vec::new();
    input
        .by_ref()
        .take(MAX_IMAGE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(fail)?;
    if bytes.len() as u64 > MAX_IMAGE_BYTES {
        return Err(AppError::invalid("imageSize"));
    }
    Ok(bytes)
}
fn language_code(name: &str) -> Option<&'static str> {
    Some(match name {
        "English" => "en",
        "Russian" => "ru",
        "Chinese" => "zh",
        "Japanese" => "ja",
        "Korean" => "ko",
        "German" => "de",
        "French" => "fr",
        "Spanish" => "es",
        "Italian" => "it",
        "Portuguese" => "pt",
        _ => return None,
    })
}

fn book(
    path: &Path,
    directory: &Path,
    db: &mut Connection,
) -> Result<(Option<String>, Vec<String>), AppError> {
    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if !["txt", "fb2", "epub", "pdf", "zip"].contains(&ext.as_str()) {
        return Err(AppError::invalid("bookFormat"));
    }
    let mut loaded = crate::book::load_book(path).map_err(fail)?;
    tracing::debug!(format = ?loaded.format, encoding = %loaded.encoding, "book source decoded");
    let mut warnings = Vec::new();
    if loaded.encoding_had_errors {
        warnings.push("import.encodingUncertain".into());
    }
    if loaded.chapters.is_empty() {
        let decoded = crate::book::read_book_file(path).map_err(fail)?;
        if decoded.text.trim().is_empty() {
            return Err(AppError::invalid("emptySource"));
        }
        loaded.chapters.push(crate::book::Chapter {
            index: 1,
            number: None,
            title: loaded.meta.title.clone().unwrap_or_else(|| "1".into()),
            body: decoded.text,
        });
        warnings.push("import.singleChapter".into());
    }
    let sample = crate::language::sample_book(&loaded.chapters);
    let language = crate::language::detect_from_script(&sample)
        .or_else(|| crate::language::detect_from_words(&sample))
        .and_then(language_code)
        .map(String::from);
    db.execute(
        "INSERT INTO book_source_metadata(singleton,title,author,summary) VALUES(1,?1,?2,?3)",
        rusqlite::params![loaded.meta.title, loaded.meta.author, loaded.meta.summary],
    )
    .map_err(fail)?;
    let store = AssetStore::new(directory).map_err(fail)?;
    if let Some((_, bytes)) = &loaded.cover {
        let id = crate::application::book_presentation::publish_cover(db, directory, bytes)?;
        db.execute(
            "INSERT INTO book_presentation(singleton,cover_asset_id) VALUES(1,?1)",
            [id],
        )
        .map_err(fail)?;
    }

    let mut assets = HashMap::new();
    for (id, bytes) in &loaded.embedded_assets {
        assets.insert(
            id.clone(),
            store.publish(db, bytes, extension(bytes)?).map_err(fail)?,
        );
    }
    if !loaded.assets.is_empty() {
        let mut archive =
            zip::ZipArchive::new(std::fs::File::open(path).map_err(fail)?).map_err(fail)?;
        for asset in &loaded.assets {
            let bytes = read_image(archive.by_name(&asset.href).map_err(fail)?)?;
            assets.insert(
                asset.id.clone(),
                store
                    .publish(db, &bytes, extension(&bytes)?)
                    .map_err(fail)?,
            );
        }
    }
    let blocks_by_chapter: HashMap<_, _> = loaded
        .blocks
        .iter()
        .map(|blocks| (blocks.chapter_index, blocks))
        .collect();
    for (position, chapter) in loaded.chapters.iter().enumerate() {
        let chapter_id = uuid();
        let typed = blocks_by_chapter.get(&chapter.index);
        let content: Vec<BookBlockContent> = if let Some(typed) = typed {
            typed
                .blocks
                .iter()
                .map(|block| match block.kind {
                    crate::book::blocks::BlockKind::Image => Ok(BookBlockContent::Image {
                        asset_id: assets
                            .get(block.asset_id.as_deref().unwrap_or(""))
                            .cloned()
                            .ok_or_else(|| AppError::invalid("imageReference"))?,
                        alt: String::new(),
                    }),
                    crate::book::blocks::BlockKind::Text => Ok(BookBlockContent::Text {
                        text: block.text.clone(),
                    }),
                    crate::book::blocks::BlockKind::Caption => Ok(BookBlockContent::Caption {
                        text: block.text.clone(),
                    }),
                })
                .collect::<Result<_, AppError>>()?
        } else {
            vec![BookBlockContent::Text {
                text: chapter.body.clone(),
            }]
        };
        let blocks = content
            .into_iter()
            .enumerate()
            .map(|(position, content)| BookBlockView {
                id: uuid(),
                chapter_id: chapter_id.clone(),
                position: position as u32,
                revision: Revision("0".into()),
                content,
                translated_text: None,
            })
            .collect::<Vec<_>>();
        ProjectRepository::new(db, ProjectKind::Book)?.insert_chapter(
            &ChapterSummary {
                translated_volume: None,
                volume: crate::book::parser::chapter_volume(&chapter.title),
                translated_title: None,
                status: "pending".into(),
                origin: None,
                needs_review: false,
                id: ChapterId(chapter_id.clone()),
                position: position as u32,
                title: chapter.title.clone(),
                revision: Revision("0".into()),
            },
            &blocks,
        )?;
        db.execute(
            "UPDATE book_chapters SET display_number=?1 WHERE id=?2",
            rusqlite::params![chapter.number.map(|n| n as i64), chapter_id],
        )
        .map_err(storage_error)?;
    }
    Ok((language, warnings))
}

#[cfg(test)]
mod volume_import_tests {
    use super::*;
    #[test]
    fn txt_volume_headings_reach_the_chapter_reader() {
        let root = std::env::temp_dir().join(format!("volume-import-{}", uuid()));
        std::fs::create_dir_all(&root).unwrap();
        let source = root.join("book.txt");
        std::fs::write(
            &source,
            "书名\n作者：作者\n第01集 第一章 开始\n正文一\n第02集 第一章 继续\n正文二",
        )
        .unwrap();
        let mut db = crate::storage::tests::database(ProjectKind::Book);
        let (_, warnings) = book(&source, &root, &mut db).unwrap();
        assert!(!warnings.iter().any(|w| w == "import.singleChapter"));
        let ids = db
            .prepare("SELECT id FROM book_chapters ORDER BY position")
            .unwrap()
            .query_map([], |r| r.get::<_, String>(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(ids.len(), 2);
        let mut repository = ProjectRepository::new(&mut db, ProjectKind::Book).unwrap();
        for (id, volume) in ids.iter().zip(["第1集", "第2集"]) {
            let chapter = repository.chapter(id).unwrap();
            assert_eq!(chapter.chapter.volume.as_deref(), Some(volume));
        }
        std::fs::remove_dir_all(root).unwrap();
    }
}
