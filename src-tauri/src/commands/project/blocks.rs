//! Typed chapter content at import time: copy the book's images into the
//! project directory and record the blocks that point at them.
//!
//! Images are copied rather than read back out of the source on demand, so a
//! project keeps working when the `.epub` is deleted, moved, or sits on a
//! read-only mount — the same reason `progress.db` does not live next to the
//! book (see `session::db_path_for_project`).

use std::path::Path;

use crate::book::{asset_file_name, extract_epub_assets, AssetRef, LoadedBook};
use crate::session::{project_dir, read_manifest};
use crate::state::{AssetRow, Store};

/// Project sub-directory holding extracted images.
pub(crate) use crate::assets::store::DIRECTORY as ASSETS_DIR;

/// Copy `book`'s images into the project and record its typed chapters.
///
/// Best-effort: a picture that cannot be unpacked costs that picture, not the
/// import. The chapter text already holds the marker either way, so nothing
/// silently changes position.
pub(crate) fn persist(store: &Store, project_id: &str, source_path: &str, book: &LoadedBook) {
    if book.assets.is_empty() && book.blocks.is_empty() {
        return;
    }
    let Ok(dir) = project_dir(project_id) else {
        return;
    };
    let assets_dir = dir.join(ASSETS_DIR);
    if let Err(e) = extract_epub_assets(Path::new(source_path), &book.assets, &assets_dir) {
        tracing::warn!("extracting epub images failed: {e:#}");
    }
    if let Err(e) = store.save_assets(&asset_rows(&book.assets, &assets_dir)) {
        tracing::warn!("recording images failed: {e:#}");
    }
    if let Err(e) = store.init_chapter_blocks(&book.blocks) {
        tracing::warn!("recording chapter blocks failed: {e:#}");
    }
}

/// Give an EPUB project imported before typed content its blocks and images.
///
/// Skipped when the re-parse does not line up with what is stored: the old
/// importer dropped image-only pages outright, so such a book has shifted
/// chapter indices and can only be fixed by re-importing it — writing blocks
/// onto shifted indices would put pictures in the wrong chapters.
pub(crate) fn backfill(store: &Store, project_id: &str) {
    let Ok(metadata) = store.project_metadata() else {
        return;
    };
    if metadata.format.as_deref() != Some("Epub") {
        return;
    }
    if store.has_chapter_blocks().unwrap_or(true) {
        return;
    }
    let Ok(manifest) = read_manifest(project_id) else {
        return;
    };
    let path = Path::new(&manifest.source_path);
    if !path.exists() {
        return;
    }
    let Ok(book) = crate::book::load_book(path) else {
        return;
    };
    if book.blocks.is_empty() {
        return;
    }
    let Ok(stored) = store.list_chapters() else {
        return;
    };
    let same = stored.len() == book.chapters.len()
        && stored
            .iter()
            .zip(book.chapters.iter())
            .all(|(row, parsed)| row.title == parsed.title);
    if !same {
        tracing::warn!(
            project = project_id,
            "epub re-parse does not match the stored chapters; re-import the book to get its images"
        );
        return;
    }
    persist(store, project_id, &manifest.source_path, &book);
}

/// Asset rows for the files just written, with the dimensions the reader needs
/// to lay a page out before the image has loaded.
fn asset_rows(assets: &[AssetRef], assets_dir: &Path) -> Vec<AssetRow> {
    assets
        .iter()
        .map(|asset| {
            let name = asset_file_name(asset);
            let (width, height) = dimensions(&assets_dir.join(&name));
            AssetRow {
                id: asset.id.clone(),
                rel_path: format!("{ASSETS_DIR}/{name}"),
                content_type: asset.content_type.clone(),
                bytes: asset.bytes,
                width,
                height,
            }
        })
        .collect()
}

/// Pixel size from the file header, without decoding the image.
fn dimensions(path: &Path) -> (Option<u32>, Option<u32>) {
    let read = image::ImageReader::open(path)
        .ok()
        .and_then(|reader| reader.with_guessed_format().ok())
        .and_then(|reader| reader.into_dimensions().ok());
    match read {
        Some((w, h)) => (Some(w), Some(h)),
        None => (None, None),
    }
}
