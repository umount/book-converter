//! Reference-book commands.

use std::path::Path;

use tauri::State;

use crate::dto::{err, RefInfo};
use crate::reference::{self};
use crate::session::{write_manifest, AppState};
use crate::state::Store;

use super::util::{client, import_reference_pending};

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

    let db = state
        .with(&project_id, |s| s.db_path.clone())
        .ok_or("no_source")?;
    let imported = import_reference_pending(&db, &reference).map_err(err)?;
    let manifest = crate::session::read_manifest(&project_id).map_err(err)?;
    write_manifest(&project_id, &manifest.source_path, Some(&path)).map_err(err)?;
    let store = Store::open(&db).map_err(err)?;

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
    store.set_meta("ref_title", &info.title).map_err(err)?;
    store
        .set_meta("ref_chapters", &info.chapters.to_string())
        .map_err(err)?;
    store
        .set_meta(
            "ref_max_covered",
            &info.max_covered.map(|n| n.to_string()).unwrap_or_default(),
        )
        .map_err(err)?;
    if let Some(style) = &style {
        store.set_meta("ref_style", style).map_err(err)?;
    }

    // A reference is already in the target language. Adopt only metadata the
    // user/project does not already have; SQLite remains the sole source of truth.
    let current = store.project_metadata().map_err(err)?;
    if current.summary.is_none() {
        if let Some(v) = &annotation {
            store.set_meta("summary", v).map_err(err)?;
        }
    }
    if current.title_translated.is_none() {
        if let Some(v) = &ref_title {
            store.set_meta("title_translated", v).map_err(err)?;
        }
    }
    if current.author_translated.is_none() {
        if let Some(v) = ref_author.filter(|a| !a.trim().is_empty()) {
            store.set_meta("author_translated", &v).map_err(err)?;
        }
    }
    if current.cover_base64.is_none() {
        if let Some(c) = &cover {
            store
                .set_cover_meta(Some(&c.content_type), Some(&c.base64))
                .map_err(err)?;
        }
    }
    store
        .set_meta(HEAD_IMPORTED, &HEAD_IMPORT_VERSION.to_string())
        .map_err(err)?;
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
    let non_empty_meta = |key: &str| -> Result<Option<String>, String> {
        store
            .get_meta(key)
            .map_err(err)
            .map(|value| value.filter(|v| !v.trim().is_empty()))
    };

    // The style exemplar feeds every translation prompt. Stored when the
    // reference was loaded; derived from the imported chapters (and stored) for
    // projects that predate that.
    match non_empty_meta("ref_style")? {
        Some(_) => {}
        None => {
            let derived = store.reference_style(STYLE_CHARS).map_err(err)?;
            if let Some(style) = &derived {
                store.set_meta("ref_style", style).map_err(err)?;
            }
        }
    }
    Ok(Some(RefInfo {
        title: non_empty_meta("ref_title")?.unwrap_or_default(),
        chapters,
        max_covered,
        // Importing happens when the reference is loaded, not when it is reopened.
        imported: 0,
    }))
}

/// Meta key recording which version of the reference head import has run.
const HEAD_IMPORTED: &str = "ref_head_imported";
/// Bump when the import starts reading a field it did not read before, so
/// projects that already ran an older version pick the new one up.
/// 1: cover + annotation. 2: also the reference's own title.
const HEAD_IMPORT_VERSION: u32 = 2;

/// One-time backfill of the annotation and cover a reference contributes.
///
/// Those used to live only in the session, restored by re-parsing the reference
/// file on every project activation; projects attached before that stopped have
/// them nowhere. This reads the reference once, writes what is missing, and
/// marks itself done so it never runs again, whether or not it found anything.
///
/// Returns true when something was written, so the caller can refresh. New
/// references never reach here: `load_reference` stores these directly.
#[tauri::command]
pub async fn backfill_reference_head(
    project_id: String,
    state: State<'_, AppState>,
) -> Result<bool, String> {
    let Some(db) = state.with(&project_id, |s| s.db_path.clone()) else {
        return Ok(false);
    };
    {
        let store = Store::open(&db).map_err(err)?;
        let done: u32 = store
            .get_meta(HEAD_IMPORTED)
            .map_err(err)?
            .and_then(|v| v.trim().parse().ok())
            .unwrap_or(0);
        if done >= HEAD_IMPORT_VERSION {
            return Ok(false);
        }
    }
    let Some(ref_path) = crate::session::manifest_ref_path(&project_id)
        .map_err(err)?
    else {
        Store::open(&db)
            .map_err(err)?
            .set_meta(HEAD_IMPORTED, &HEAD_IMPORT_VERSION.to_string())
            .map_err(err)?;
        return Ok(false);
    };

    // Reading and decoding a whole book, so off the async executor.
    let head = tauri::async_runtime::spawn_blocking(move || {
        reference::load_head(Path::new(&ref_path))
    })
    .await
    .map_err(|e| err(anyhow::anyhow!("backfill task failed: {e}")))?
    .map_err(err)?;

    let store = Store::open(&db).map_err(err)?;
    let missing = |k: &str| {
        store
            .get_meta(k)
            .ok()
            .flatten()
            .filter(|v| !v.trim().is_empty())
            .is_none()
    };

    let mut wrote = false;
    if let Some(head) = head {
        if let Some(cover) = head.cover.filter(|_| missing("cover_b64")) {
            store.set_meta("cover_ct", &cover.content_type).map_err(err)?;
            store.set_meta("cover_b64", &cover.base64).map_err(err)?;
            wrote = true;
        }
        if let Some(annotation) = head.annotation.filter(|_| missing("summary")) {
            store.set_meta("summary", &annotation).map_err(err)?;
            wrote = true;
        }
        // The reference's own title, shown in the overview's Reference panel.
        if let Some(title) = head.title.filter(|_| missing("ref_title")) {
            store.set_meta("ref_title", &title).map_err(err)?;
            wrote = true;
        }
    }
    store
        .set_meta(HEAD_IMPORTED, &HEAD_IMPORT_VERSION.to_string())
        .map_err(err)?;
    tracing::info!(project = %project_id, wrote, "reference head backfill done");
    Ok(wrote)
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
