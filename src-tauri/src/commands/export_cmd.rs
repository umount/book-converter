//! Export translated book command.

use std::collections::HashMap;
use std::path::Path;

use tauri::State;

use crate::config::Config;
use crate::dto::err;
use crate::export::{self, OutputMeta, TranslatedChapter};
use crate::session::AppState;
use crate::state::Store;

use super::util::OutputTarget;

/// Export the translated chapters to `out_path` (format inferred from extension).
#[tauri::command]
pub async fn export_book(
    project_id: String,
    out_path: String,
    state: State<'_, AppState>,
) -> Result<String, String> {
    let db = state
        .with(&project_id, |s| s.db_path.clone())
        .ok_or("no_source")?;
    let manifest = crate::session::read_manifest(&project_id).map_err(err)?;
    let zipped_input = crate::session::zipped_input_for(
        Some(&manifest.source_path),
        manifest.ref_path.as_deref(),
    );

    // Decide zip vs plain, and the inner format. Zip when the path ends in .zip
    // or the input was itself zipped ("zip in → zip out").
    let out = OutputTarget::resolve(&out_path, zipped_input)?;

    let store = Store::open(&db).map_err(err)?;
    let metadata = store.project_metadata().map_err(err)?;
    let rows = store.translated_chapters().map_err(err)?;
    if rows.is_empty() {
        return Err("nothing_translated".into());
    }

    // Chapter numbers come from the DB (no need to re-read the source file, so a
    // project exported from an archive works without the original book present).
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

    let config = Config::load();
    let chapter_label = crate::i18n::label(&config.target_lang, "chapter");
    export::normalize_titles(&mut chapters, &chapter_label);
    let cover = match (
        metadata.cover_content_type,
        metadata.cover_base64,
    ) {
        (Some(content_type), Some(base64)) => {
            Some(crate::export::fb2::Cover { content_type, base64 })
        }
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
