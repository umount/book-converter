//! Export the translated book to a chosen path.

use std::collections::HashMap;
use std::path::Path;

use crate::config::Config;
use crate::dto::err;
use crate::export::{self, ExportImage, OutputFormat, OutputMeta, TranslatedChapter};
use crate::session::AppState;
use crate::state::Store;

use super::super::util::OutputTarget;
use super::project_store;

pub(crate) fn sanitize_stem(title: &str) -> String {
    let stem: String = title
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' {
                c
            } else {
                '_'
            }
        })
        .take(80)
        .collect();
    let stem = stem.trim_matches('_').trim_matches('.');
    if stem.is_empty() {
        "book".into()
    } else {
        stem.to_string()
    }
}

pub(crate) fn planned_path(
    state: &AppState,
    project_id: &str,
    format: OutputFormat,
) -> Result<String, String> {
    let store = project_store(state, project_id)?;
    let meta = store.project_metadata().map_err(err)?;
    let title = meta
        .title_translated
        .filter(|t| !t.trim().is_empty())
        .or(meta.title)
        .unwrap_or_else(|| "book".into());
    let dir = crate::session::project_dir(project_id)
        .map_err(|e| e.to_string())?
        .join("export");
    Ok(dir
        .join(format!("{}.{}", sanitize_stem(&title), format.ext()))
        .to_string_lossy()
        .into_owned())
}

pub(crate) fn export(state: &AppState, project_id: &str, out_path: &str) -> Result<String, String> {
    let store = project_store(state, project_id)?;
    write_export(&store, project_id, out_path)
}

fn write_export(store: &Store, project_id: &str, out_path: &str) -> Result<String, String> {
    let manifest = crate::session::read_manifest(project_id).map_err(err)?;
    let zipped_input =
        crate::session::zipped_input_for(Some(&manifest.source_path), manifest.ref_path.as_deref());
    let out = OutputTarget::resolve(out_path, zipped_input)?;
    let metadata = store.project_metadata().map_err(err)?;
    // Pages of pictures come along even though they have no translation: they
    // are the book's actual content in an illustrated edition or a manga.
    let rows = store.chapters_for_export().map_err(err)?;
    if rows.is_empty() {
        return Err("nothing_translated".into());
    }

    let num_by_idx: HashMap<usize, Option<usize>> = store
        .list_chapters()
        .map_err(err)?
        .into_iter()
        .map(|row| (row.idx, row.number))
        .collect();
    let mut chapters: Vec<TranslatedChapter> = rows
        .into_iter()
        .map(|(idx, title, body)| TranslatedChapter {
            index: idx,
            number: num_by_idx.get(&idx).copied().flatten(),
            title,
            body,
        })
        .collect();

    let config = Config::load_for(store);
    let chapter_label = crate::i18n::label(&config.target_lang, "chapter");
    export::normalize_titles(&mut chapters, &chapter_label);
    let cover = match (metadata.cover_content_type, metadata.cover_base64) {
        (Some(content_type), Some(base64)) => Some(crate::export::fb2::Cover {
            content_type,
            base64,
        }),
        _ => None,
    };
    let meta = OutputMeta {
        title: metadata
            .title_translated
            .filter(|t| !t.trim().is_empty())
            .or(metadata.title)
            .unwrap_or_else(|| crate::i18n::label(&config.target_lang, "untitled")),
        author: metadata
            .author_translated
            .filter(|a| !a.trim().is_empty())
            .or(metadata.author)
            .unwrap_or_else(|| crate::i18n::label(&config.target_lang, "unknown_author")),
        lang: crate::i18n::lang_code(&config.target_lang),
        annotation: metadata.summary,
        cover,
        images: project_images(store, project_id),
    };

    if out.zipped {
        export::export_zip(
            &chapters,
            out.format,
            &meta,
            &out.inner_name,
            Path::new(&out.path),
        )
        .map_err(err)?;
    } else {
        export::export(&chapters, out.format, &meta, Path::new(&out.path)).map_err(err)?;
    }
    Ok(out.path)
}

/// The project's pictures, by asset id, for the writers that can embed them.
/// Whatever cannot be found is simply not embedded; a chapter's text keeps its
/// markers, and the formats that cannot hold a picture drop them.
fn project_images(store: &Store, project_id: &str) -> HashMap<String, ExportImage> {
    let Ok(dir) = crate::session::project_dir(project_id) else {
        return HashMap::new();
    };
    store
        .assets()
        .unwrap_or_default()
        .into_iter()
        .map(|asset| {
            (
                asset.id,
                ExportImage {
                    path: dir.join(&asset.rel_path),
                    content_type: asset.content_type,
                },
            )
        })
        .collect()
}
