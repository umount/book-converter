//! Project load / open / delete / archive commands.

use std::path::Path;

use tauri::State;

use crate::book::load_book;
use crate::dto::{err, BookInfo, ImportedProject, ProjectSummary};
use crate::export::fb2::Cover;
use crate::session::{
    db_path_for_project, project_dir, write_manifest, AppState, Manifest,
};
use crate::state::Store;

use super::util::ensure_chapters;

/// Load a source book (TXT/FB2) into a project, open/create its progress DB, and
/// register chapters. Each project has its own id and data directory.
#[tauri::command]
pub async fn load_source(
    project_id: String,
    path: String,
    state: State<'_, AppState>,
) -> Result<BookInfo, String> {
    let mut book = load_book(Path::new(&path)).map_err(err)?;
    // No chapter pattern matched (e.g. a technical PDF whose headings are not
    // "Chapter N"): ask the model to infer a delimiter, else keep the whole book as
    // a single chapter so its content still shows up.
    if book.chapters.is_empty() {
        ensure_chapters(&path, &mut book).await;
    }
    if book.chapters.is_empty() {
        return Err("no_text_extracted".into());
    }
    let db = db_path_for_project(&project_id);
    let store = Store::open(&db).map_err(err)?;
    store.init_chapters(&book.chapters).map_err(err)?;
    write_manifest(&project_id, &path, None);

    let title = book.meta.title.clone().unwrap_or_default();
    let author = book.meta.author.clone().unwrap_or_default();

    // If the source itself is FB2, pull its annotation + cover as defaults.
    let head = if book.format == crate::book::InputFormat::Fb2 {
        crate::book::read_book_file(Path::new(&path))
            .ok()
            .map(|d| crate::export::fb2::extract_head(&d.text))
    } else {
        None
    };

    // For a PDF, use the first-page cover image as the default cover.
    let pdf_cover = if path.to_lowercase().ends_with(".pdf") {
        use base64::Engine as _;
        std::fs::read(&path)
            .ok()
            .and_then(|b| crate::book::extract_pdf_cover(&b))
            .map(|(content_type, bytes)| Cover {
                content_type,
                base64: base64::engine::general_purpose::STANDARD.encode(&bytes),
            })
    } else {
        None
    };

    // Per-project details persisted in this book's DB (survive project switches).
    let saved_title = store.get_meta("title_translated").ok().flatten();
    let saved_author = store.get_meta("author_translated").ok().flatten();
    let saved_summary = store.get_meta("summary").ok().flatten();
    let saved_cover = match (
        store.get_meta("cover_ct").ok().flatten(),
        store.get_meta("cover_b64").ok().flatten(),
    ) {
        (Some(content_type), Some(base64)) => Some(Cover { content_type, base64 }),
        _ => None,
    };
    let summary = saved_summary.or_else(|| head.as_ref().and_then(|h| h.annotation.clone()));
    let cover = saved_cover
        .or_else(|| head.as_ref().and_then(|h| h.cover.clone()))
        .or(pdf_cover);

    // Persist the source metadata + resolved cover to the DB so a project is fully
    // self-contained (openable from its DB alone, and portable in an archive).
    let format = format!("{:?}", book.format);
    let _ = store.set_meta("title", &title);
    let _ = store.set_meta("author", &author);
    let _ = store.set_meta("format", &format);
    let _ = store.set_meta("encoding", &book.encoding);
    if let Some(s) = &summary {
        let _ = store.set_meta("summary", s);
    }
    if let Some(c) = &cover {
        let _ = store.set_meta("cover_ct", &c.content_type);
        let _ = store.set_meta("cover_b64", &c.base64);
    }

    state.with(&project_id, |s| {
        s.reference = None;
        s.style = None;
        s.cancel = None;
        s.running = false;
        s.zipped_input = crate::book::source::is_zip(Path::new(&path));
        s.db_path = Some(db);
        s.source_path = Some(path);
        s.title = book.meta.title.clone();
        s.author = book.meta.author.clone();
        s.title_translated = saved_title;
        s.author_translated = saved_author;
        s.summary = summary;
        s.cover = cover;
    });

    Ok(BookInfo {
        title,
        author,
        total_chapters: book.chapters.len(),
        format,
        encoding: book.encoding,
        needs_delimiter: book.needs_delimiter,
        missing: book.report.missing_numbers.len(),
        duplicates: book.report.duplicate_numbers.len(),
        had_errors: book.encoding_had_errors,
    })
}

/// Open an already-loaded project from its database alone (no source file needed):
/// chapters, glossary and translations already live in `progress.db`. Used when
/// switching back to a project, restoring after restart, or opening an archive.
#[tauri::command]
pub async fn open_project(
    project_id: String,
    state: State<'_, AppState>,
) -> Result<BookInfo, String> {
    let db = db_path_for_project(&project_id);
    if !Path::new(&db).exists() {
        return Err("no_source".into());
    }
    let store = Store::open(&db).map_err(err)?;
    // Activation is one of the two moments crash recovery is allowed to run: a
    // chapter left `in_progress` here belongs to a process that is gone.
    let _ = store.recover();
    let stats = store.stats().map_err(err)?;
    if stats.total == 0 {
        return Err("no_source".into());
    }
    let g = |k: &str| store.get_meta(k).ok().flatten();
    let title = g("title").unwrap_or_default();
    let author = g("author").unwrap_or_default();
    let format = g("format").unwrap_or_else(|| "-".into());
    let encoding = g("encoding").unwrap_or_else(|| "-".into());
    let cover = match (g("cover_ct"), g("cover_b64")) {
        (Some(content_type), Some(base64)) => Some(Cover { content_type, base64 }),
        _ => None,
    };
    let manifest: Option<Manifest> = std::fs::read(project_dir(&project_id).join("project.json"))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok());
    let source_path = manifest.as_ref().map(|m| m.source_path.clone());
    // "zip in, zip out" has to survive a restart. It used to be set as a side
    // effect of re-parsing the reference on every activation, so it was lost
    // when that stopped; the manifest records both inputs, so ask it.
    let zipped = crate::session::zipped_input_for(
        source_path.as_deref(),
        manifest.as_ref().and_then(|m| m.ref_path.as_deref()),
    );

    state.with(&project_id, |s| {
        s.reference = None;
        s.style = None;
        s.cancel = None;
        s.running = false;
        s.zipped_input = zipped;
        s.db_path = Some(db.clone());
        s.source_path = source_path;
        s.title = (!title.is_empty()).then(|| title.clone());
        s.author = (!author.is_empty()).then(|| author.clone());
        s.title_translated = g("title_translated");
        s.author_translated = g("author_translated");
        s.summary = g("summary");
        s.cover = cover;
    });

    Ok(BookInfo {
        title,
        author,
        total_chapters: stats.total,
        format,
        encoding,
        needs_delimiter: false,
        missing: 0,
        duplicates: 0,
        had_errors: false,
    })
}

/// Every project that exists on disk, newest data first.
///
/// The UI keeps its project list in `localStorage`, which is a cache, not the
/// record: clearing it, or moving to another machine, used to strand the
/// project directories with no way back except importing a `.bcproj`. Each
/// project directory is self-describing (`project.json` beside `progress.db`),
/// so the real list can simply be read.
///
/// A directory without a readable database is skipped rather than reported as a
/// broken project: it is either a half-finished import or something that is not
/// a project at all.
#[tauri::command]
pub async fn list_projects() -> Result<Vec<ProjectSummary>, String> {
    let root = crate::paths::app_data_dir().join("projects");
    let Ok(entries) = std::fs::read_dir(&root) else {
        return Ok(Vec::new());
    };

    let mut out = Vec::new();
    for entry in entries.flatten() {
        if !entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            continue;
        }
        let id = entry.file_name().to_string_lossy().into_owned();
        let db = entry.path().join("progress.db");
        if !db.exists() {
            continue;
        }
        let Ok(store) = Store::open(&db.to_string_lossy()) else {
            continue;
        };
        let Ok(stats) = store.stats() else { continue };
        if stats.total == 0 {
            continue;
        }

        let manifest: Manifest = std::fs::read(entry.path().join("project.json"))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        // A project imported from an archive may have no manifest name; the
        // book's own title is the best fallback, then the directory id.
        let name = if manifest.name.trim().is_empty() {
            store
                .get_meta("title")
                .ok()
                .flatten()
                .filter(|t| !t.trim().is_empty())
                .unwrap_or_else(|| id.clone())
        } else {
            manifest.name.clone()
        };

        out.push(ProjectSummary {
            id,
            name,
            source_path: manifest.source_path,
            ref_path: manifest.ref_path,
            total: stats.total,
            done: stats.done,
        });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

/// Delete a project and all of its data (progress DB, manifest, extracted files).
#[tauri::command]
pub async fn delete_project(project_id: String, state: State<'_, AppState>) -> Result<(), String> {
    state.0.lock().unwrap().remove(&project_id);
    let dir = project_dir(&project_id);
    if dir.exists() {
        std::fs::remove_dir_all(&dir).map_err(err)?;
    }
    Ok(())
}

/// Save a project to a self-contained `.bcproj` archive (manifest + progress.db
/// only), so it is portable and can be re-opened elsewhere.
#[tauri::command]
pub async fn export_project(
    project_id: String,
    out_path: String,
    _state: State<'_, AppState>,
) -> Result<(), String> {
    use std::io::Write as _;

    // The database is self-contained (chapter source text, translations, glossary,
    // cover, metadata all live in it), so the archive needs only the manifest and the
    // DB — not a copy of the original book.
    let dir = project_dir(&project_id);
    let manifest_bytes = std::fs::read(dir.join("project.json")).map_err(err)?;
    let db_bytes = std::fs::read(dir.join("progress.db")).map_err(err)?;

    let file = std::fs::File::create(&out_path).map_err(err)?;
    let mut zip = zip::ZipWriter::new(file);
    let opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    for (name, bytes) in [("project.json", &manifest_bytes), ("progress.db", &db_bytes)] {
        zip.start_file(name, opts).map_err(err)?;
        zip.write_all(bytes).map_err(err)?;
    }
    zip.finish().map_err(err)?;
    Ok(())
}

/// Import a `.bcproj` archive into a new project directory (manifest + database).
/// The frontend then opens it from its DB with `open_project`.
#[tauri::command]
pub async fn import_project(
    project_id: String,
    archive_path: String,
) -> Result<ImportedProject, String> {
    use std::io::Read as _;

    let dir = project_dir(&project_id);
    std::fs::create_dir_all(&dir).map_err(err)?;
    let file = std::fs::File::open(&archive_path).map_err(err)?;
    let mut zip = zip::ZipArchive::new(file).map_err(err)?;

    let mut name = "Imported project".to_string();
    let mut source_path = String::new();
    let mut has_db = false;
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).map_err(err)?;
        let ename = entry.name().to_string();
        let mut buf = Vec::new();
        entry.read_to_end(&mut buf).map_err(err)?;
        if ename == "project.json" {
            if let Ok(m) = serde_json::from_slice::<Manifest>(&buf) {
                name = m.name;
                source_path = m.source_path;
            }
            std::fs::write(dir.join("project.json"), &buf).map_err(err)?;
        } else if ename == "progress.db" {
            std::fs::write(dir.join("progress.db"), &buf).map_err(err)?;
            has_db = true;
        }
    }
    if !has_db {
        return Err("archive_no_book".into());
    }
    Ok(ImportedProject { name, source_path })
}
