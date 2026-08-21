//! Reference-book commands.

use std::path::Path;

use tauri::State;

use crate::dto::{err, RefInfo};
use crate::reference::{self};
use crate::session::{write_manifest, AppState};
use crate::state::Store;

use super::util::{client, import_reference_pending, persist_meta};

/// How much professional text is quoted as a style exemplar in prompts.
const STYLE_CHARS: usize = 600;

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
    let style = reference::style_exemplar(&reference, STYLE_CHARS);
    let annotation = reference.head.as_ref().and_then(|h| h.annotation.clone());
    let cover = reference.head.as_ref().and_then(|h| h.cover.clone());
    let ref_title = reference.meta.title.clone();
    let ref_author = reference.meta.author.clone();

    // Everything the app needs from a reference after this point is small:
    // the style exemplar and a few counters. Persist them, so opening the
    // project later restores them from the database instead of re-reading and
    // re-parsing the reference file (and the source book with it).
    persist_meta(&db, "ref_title", &info.title);
    persist_meta(&db, "ref_chapters", &info.chapters.to_string());
    persist_meta(
        &db,
        "ref_max_covered",
        &info.max_covered.map(|n| n.to_string()).unwrap_or_default(),
    );
    if let Some(style) = &style {
        persist_meta(&db, "ref_style", style);
    }

    let adopted = state.with(&project_id, |s| {
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
        (
            s.summary.clone(),
            s.title_translated.clone(),
            s.author_translated.clone(),
            s.cover.clone(),
        )
    });

    // What was adopted has to be persisted too: it used to survive only because
    // the reference was re-parsed on every activation.
    let (summary, title_translated, author_translated, cover) = adopted;
    if let Some(v) = &summary {
        persist_meta(&db, "summary", v);
    }
    if let Some(v) = &title_translated {
        persist_meta(&db, "title_translated", v);
    }
    if let Some(v) = &author_translated {
        persist_meta(&db, "author_translated", v);
    }
    if let Some(c) = &cover {
        persist_meta(&db, "cover_ct", &c.content_type);
        persist_meta(&db, "cover_b64", &c.base64);
    }
    Ok(info)
}

/// The reference attached to a project, read from the project's own database.
///
/// Loading a reference imports its chapters, its style excerpt and its metadata
/// into the database, so reopening the project needs no access to the reference
/// file at all. Activation used to call `load_reference` here, which re-read and
/// re-parsed the professional translation **and** the whole source book on every
/// open, for data that was already stored.
///
/// The counts come from the chapters themselves rather than from a remembered
/// number, so they stay true after a reset and are right for projects created
/// before the style excerpt was stored.
#[tauri::command]
pub async fn get_reference_info(
    project_id: String,
    state: State<'_, AppState>,
) -> Result<Option<RefInfo>, String> {
    let Some(db) = state.with(&project_id, |s| s.db_path.clone()) else {
        return Ok(None);
    };
    let store = Store::open(&db).map_err(err)?;
    let (chapters, max_covered) = store.reference_stats().map_err(err)?;
    if chapters == 0 {
        return Ok(None); // no reference has ever been applied to this project
    }
    let meta = |k: &str| store.get_meta(k).ok().flatten().filter(|v| !v.trim().is_empty());

    // The style exemplar feeds every translation prompt. Stored when the
    // reference was loaded; derived from the imported chapters (and stored) for
    // projects that predate that.
    let style = match meta("ref_style") {
        Some(style) => Some(style),
        None => {
            let derived = store.reference_style(STYLE_CHARS).map_err(err)?;
            if let Some(style) = &derived {
                persist_meta(&Some(db.clone()), "ref_style", style);
            }
            derived
        }
    };
    state.with(&project_id, |s| {
        if s.style.is_none() {
            s.style = style;
        }
    });

    Ok(Some(RefInfo {
        title: meta("ref_title").unwrap_or_default(),
        chapters,
        max_covered,
        // Importing happens when the reference is loaded, not when it is reopened.
        imported: 0,
    }))
}

/// Bootstrap a pinned glossary from `sample` aligned reference chapters.
#[tauri::command]
pub async fn bootstrap_glossary(
    project_id: String,
    sample: usize,
    state: State<'_, AppState>,
) -> Result<usize, String> {
    let db = state
        .with(&project_id, |s| s.db_path.clone())
        .ok_or("no_source")?;
    let store = Store::open(&db).map_err(err)?;

    // The aligned pairs are already in the database: loading a reference matched
    // its chapters to the source by number and stored them. Re-reading and
    // re-aligning the two books here was work done twice.
    let pairs = store.reference_pairs(sample).map_err(err)?;
    if pairs.is_empty() {
        return Err("no_reference".into());
    }

    let cl = client()?;
    let config = crate::config::Config::load();
    // Merge into whatever is already there so a re-bootstrap does not wipe
    // terms harvested from later machine-translated chapters.
    let mut glossary = store.load_glossary().map_err(err)?;
    let extracted = reference::bootstrap_glossary(&cl, &config, &pairs)
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
    let db = state
        .with(&project_id, |s| s.db_path.clone())
        .ok_or("no_source")?;
    let store = Store::open(&db).map_err(err)?;
    // A reset only changes status, so the professional text is still in the
    // database: re-seeding is restoring those chapters, not re-importing them.
    store.restore_reference_chapters().map_err(err)
}
