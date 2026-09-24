//! Normalize source containers into the independent Book/Manga stores.
use crate::{
    app::{
        contracts::{
            AppError, AssetId, BookBlockContent, BookBlockView, ChapterId, PageId, ProjectKind,
            Revision, VolumeId,
        },
        requests::{ChapterSummary, PageSummary},
    },
    assets::store::AssetStore,
    storage::repository::ProjectRepository,
};
use rusqlite::Connection;
use std::{collections::HashMap, io::Read, path::Path};
const MAX_IMAGE_BYTES: u64 = 64 * 1024 * 1024;

pub(super) fn normalize(
    kind: ProjectKind,
    path: &Path,
    directory: &Path,
    db: &mut Connection,
) -> Result<(Option<String>, Vec<String>), AppError> {
    match kind {
        ProjectKind::Book => book(path, directory, db),
        ProjectKind::Manga => comic(path, directory, db).map(|_| (None, vec![])),
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
    let store = AssetStore::new(directory).map_err(fail)?;
    let mut assets = HashMap::new();
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
    for (position, chapter) in loaded.chapters.iter().enumerate() {
        let chapter_id = uuid();
        let typed = loaded
            .blocks
            .iter()
            .find(|blocks| blocks.chapter_index == chapter.index);
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
                id: ChapterId(chapter_id),
                position: position as u32,
                title: chapter.title.clone(),
                revision: Revision("0".into()),
            },
            &blocks,
        )?;
    }
    Ok((language, warnings))
}

fn is_image(name: &str) -> bool {
    Path::new(name)
        .extension()
        .and_then(|x| x.to_str())
        .is_some_and(|x| {
            ["png", "jpg", "jpeg", "gif", "webp"].contains(&x.to_ascii_lowercase().as_str())
        })
}

/// Compare digit runs numerically without integer overflow, then use the original name as a tie-break.
pub(super) fn natural_cmp(left: &str, right: &str) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    let a = left.as_bytes();
    let b = right.as_bytes();
    let (mut i, mut j) = (0, 0);
    while i < a.len() && j < b.len() {
        if a[i].is_ascii_digit() && b[j].is_ascii_digit() {
            let (start_a, start_b) = (i, j);
            while i < a.len() && a[i].is_ascii_digit() {
                i += 1;
            }
            while j < b.len() && b[j].is_ascii_digit() {
                j += 1;
            }
            let sa = left[start_a..i].trim_start_matches('0');
            let sb = right[start_b..j].trim_start_matches('0');
            let order = sa.len().cmp(&sb.len()).then_with(|| sa.cmp(sb));
            if order != Ordering::Equal {
                return order;
            }
        } else {
            let order = a[i].cmp(&b[j]);
            if order != Ordering::Equal {
                return order;
            }
            i += 1;
            j += 1;
        }
    }
    (a.len() - i)
        .cmp(&(b.len() - j))
        .then_with(|| left.cmp(right))
}

fn comic(path: &Path, directory: &Path, db: &mut Connection) -> Result<(), AppError> {
    let extension = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if !["zip", "cbz"].contains(&extension.as_str()) {
        return Err(AppError::invalid("comicFormat"));
    }
    let mut archive =
        zip::ZipArchive::new(std::fs::File::open(path).map_err(fail)?).map_err(fail)?;
    if archive.len() > 100_000 {
        return Err(AppError::invalid("archiveEntries"));
    }
    let mut entries = Vec::new();
    for index in 0..archive.len() {
        let entry = archive.by_index(index).map_err(fail)?;
        if entry.is_dir() || !is_image(entry.name()) {
            continue;
        }
        if entry.enclosed_name().is_none()
            || entry.name().contains('\\')
            || entry
                .unix_mode()
                .is_some_and(|mode| mode & 0o170000 == 0o120000)
        {
            return Err(AppError::invalid("archivePath"));
        }
        entries.push((entry.name().to_string(), index));
    }
    if entries.is_empty() {
        return Err(AppError::invalid("emptyComic"));
    }
    entries.sort_by(|a, b| natural_cmp(&a.0, &b.0));
    let store = AssetStore::new(directory).map_err(fail)?;
    let mut volumes: HashMap<String, (String, u32)> = HashMap::new();
    for (name, index) in entries {
        let parent = Path::new(&name)
            .parent()
            .unwrap_or(Path::new(""))
            .to_string_lossy()
            .into_owned();
        if !volumes.contains_key(&parent) {
            let id = uuid();
            ProjectRepository::new(db, ProjectKind::Manga)?.insert_volume(
                &id,
                volumes.len() as u32,
                &parent,
                true,
            )?;
            volumes.insert(parent.clone(), (id, 0));
        }
        let (volume, position) = volumes.get_mut(&parent).expect("inserted volume");
        let bytes = read_image(archive.by_index(index).map_err(fail)?)?;
        let asset = store
            .publish(db, &bytes, self::extension(&bytes)?)
            .map_err(fail)?;
        let (width, height) = image::ImageReader::new(std::io::Cursor::new(&bytes))
            .with_guessed_format()
            .map_err(fail)?
            .into_dimensions()
            .map_err(fail)?;
        ProjectRepository::new(db, ProjectKind::Manga)?.insert_page(&PageSummary {
            id: PageId(uuid()),
            volume_id: VolumeId(volume.clone()),
            position: *position,
            original_asset_id: AssetId(asset),
            width,
            height,
            revision: Revision("0".into()),
        })?;
        *position += 1;
    }
    Ok(())
}
