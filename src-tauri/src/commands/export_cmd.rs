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
    let (db, title, title_translated, author, author_translated, summary, cover, zipped_input) =
        state.with(&project_id, |s| {
            (
                s.db_path.clone(),
                s.title.clone(),
                s.title_translated.clone(),
                s.author.clone(),
                s.author_translated.clone(),
                s.summary.clone(),
                s.cover.clone(),
                s.zipped_input,
            )
        });
    let db = db.ok_or("no_source")?;

    // Decide zip vs plain, and the inner format. Zip when the path ends in .zip
    // or the input was itself zipped ("zip in → zip out").
    let out = OutputTarget::resolve(&out_path, zipped_input)?;

    let store = Store::open(&db).map_err(err)?;
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
        .map(|(idx, number, ..)| (idx, number))
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
    let meta = OutputMeta {
        title: title_translated
            .filter(|t| !t.trim().is_empty())
            .or(title)
            .unwrap_or_else(|| crate::i18n::label(&config.target_lang, "untitled")),
        author: author_translated
            .filter(|a| !a.trim().is_empty())
            .or(author)
            .unwrap_or_else(|| crate::i18n::label(&config.target_lang, "unknown_author")),
        lang: crate::i18n::lang_code(&config.target_lang),
        annotation: summary,
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
