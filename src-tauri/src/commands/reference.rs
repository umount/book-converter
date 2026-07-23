//! Reference-book commands.

use std::path::Path;

use tauri::State;

use crate::book::load_book;
use crate::dto::{err, RefInfo};
use crate::reference::{self};
use crate::session::{write_manifest, AppState};
use crate::state::Store;

use super::util::{client, import_reference_pending};

/// Load a reference translation: seed pending chapters from it (so they appear in
/// the reader, labeled as coming from the reference) and adopt it for canon/style.
#[tauri::command]
pub async fn load_reference(
    project_id: String,
    path: String,
    state: State<'_, AppState>,
) -> Result<RefInfo, String> {
    let reference = reference::load_reference(Path::new(&path)).map_err(err)?;

    // Seed pending chapters from the reference, if a source book is open.
    let (db, source_path) =
        state.with(&project_id, |s| (s.db_path.clone(), s.source_path.clone()));
    let imported = match (&db, &source_path) {
        (Some(db), Some(sp)) => import_reference_pending(db, sp, &reference).map_err(err)?,
        _ => 0,
    };
    if let Some(sp) = &source_path {
        write_manifest(&project_id, sp, Some(&path));
    }

    let info = RefInfo {
        title: reference.meta.title.clone().unwrap_or_default(),
        chapters: reference.chapters.len(),
        max_covered: reference::max_covered_number(&reference),
        imported,
    };
    let style = reference::style_exemplar(&reference, 600);
    let annotation = reference.head.as_ref().and_then(|h| h.annotation.clone());
    let cover = reference.head.as_ref().and_then(|h| h.cover.clone());
    let ref_title = reference.meta.title.clone();
    let ref_author = reference.meta.author.clone();

    state.with(&project_id, |s| {
        s.zipped_input |= crate::book::source::is_zip(Path::new(&path));
        // A reference is a translation, so its title/summary/cover are already in the
        // target language: adopt them unless the user has set their own.
        if s.summary.is_none() {
            s.summary = annotation;
        }
        if s.cover.is_none() {
            s.cover = cover;
        }
        if s.title_translated.is_none() {
            s.title_translated = ref_title;
        }
        if s.author_translated.is_none() {
            s.author_translated = ref_author.filter(|a| !a.trim().is_empty());
        }
        s.reference = Some(reference);
        s.style = style;
    });
    Ok(info)
}

/// Bootstrap a pinned glossary from `sample` aligned reference chapters.
#[tauri::command]
pub async fn bootstrap_glossary(
    project_id: String,
    sample: usize,
    state: State<'_, AppState>,
) -> Result<usize, String> {
    let (db, source_path, reference) = state.with(&project_id, |s| {
        (s.db_path.clone(), s.source_path.clone(), s.reference.clone())
    });
    let db = db.ok_or("no_source")?;
    let source_path = source_path.ok_or("no_source")?;
    let reference = reference.ok_or("no reference loaded")?;

    let source = load_book(Path::new(&source_path)).map_err(err)?;
    let cl = client()?;
    let store = Store::open(&db).map_err(err)?;
    // Merge into whatever is already there so a re-bootstrap does not wipe
    // terms harvested from later machine-translated chapters.
    let mut glossary = store.load_glossary().map_err(err)?;
    let extracted = reference::bootstrap_glossary(&cl, &source.chapters, &reference, sample)
        .await
        .map_err(err)?;
    crate::glossary::merge(&mut glossary, extracted);
    store.save_glossary(&glossary).map_err(err)?;
    Ok(glossary.len())
}

/// "Continue" mode: seed the chapters the reference covers (that are still pending)
/// from the professional text, so only the remaining chapters get machine-translated.
/// Loading a reference already does this; kept for an explicit re-seed.
#[tauri::command]
pub async fn use_reference_as_base(
    project_id: String,
    state: State<'_, AppState>,
) -> Result<usize, String> {
    let (db, source_path, reference) = state.with(&project_id, |s| {
        (s.db_path.clone(), s.source_path.clone(), s.reference.clone())
    });
    let db = db.ok_or("no_source")?;
    let source_path = source_path.ok_or("no_source")?;
    let reference = reference.ok_or("no reference loaded")?;
    import_reference_pending(&db, &source_path, &reference).map_err(err)
}
