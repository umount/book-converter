//! Tauri commands — the bridge between the React frontend and the Rust core.
//!
//! Session state (the current book's DB path, the loaded reference, the running
//! flag / cancel signal) lives in a managed `AppState`. Long translation runs on
//! a dedicated OS thread with its own current-thread runtime — this keeps the
//! non-`Sync` SQLite connection off the async executor — and streams progress to
//! the UI via `progress` / `done` / `job_error` events.

use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};

use crate::book::load_book;
use crate::config::Config;
use crate::export::fb2::Cover;
use crate::export::{self, OutputFormat, OutputMeta, TranslatedChapter};
use crate::glossary::{Term, TermKind};
use crate::orchestrator::Orchestrator;
use crate::reference::{self, Reference};
use crate::state::Store;
use crate::translator::DeepSeekClient;

/// Per-window session state.
#[derive(Default)]
pub struct Session {
    db_path: Option<String>,
    source_path: Option<String>,
    title: Option<String>,
    author: Option<String>,
    reference: Option<Reference>,
    style: Option<String>,
    cancel: Option<Arc<AtomicBool>>,
    running: bool,
    /// Source or reference was a `.zip` → default to a zipped output.
    zipped_input: bool,
    /// Translated book title shown in the UI and written to output.
    title_translated: Option<String>,
    /// Translated / transliterated author, so a Chinese name is not rendered as
    /// boxes in the Latin/Cyrillic-only PDF font.
    author_translated: Option<String>,
    /// Annotation / summary (auto from a source FB2, or edited by the user).
    summary: Option<String>,
    /// Cover image (auto from a source FB2, or replaced by the user).
    cover: Option<Cover>,
}

/// Managed app state: one `Session` per open project, keyed by project id, so
/// projects are isolated and can translate in parallel.
pub struct AppState(pub Mutex<HashMap<String, Session>>);

impl AppState {
    pub fn new() -> Self {
        AppState(Mutex::new(HashMap::new()))
    }

    /// Run `f` with the session for `id`, creating an empty one if absent.
    fn with<R>(&self, id: &str, f: impl FnOnce(&mut Session) -> R) -> R {
        let mut map = self.0.lock().unwrap();
        f(map.entry(id.to_string()).or_default())
    }
}

// --- DTOs ---

#[derive(Serialize)]
pub struct BookInfo {
    pub title: String,
    pub author: String,
    pub total_chapters: usize,
    pub format: String,
    pub encoding: String,
    pub needs_delimiter: bool,
    pub missing: usize,
    pub duplicates: usize,
}

#[derive(Serialize)]
pub struct RefInfo {
    pub title: String,
    pub chapters: usize,
    pub max_covered: Option<usize>,
    /// How many still-pending chapters were seeded from this reference.
    pub imported: usize,
}

#[derive(Serialize, Clone)]
pub struct Progress {
    pub project: String,
    pub done: usize,
    pub total: usize,
    pub failed: usize,
    pub pending: usize,
    pub running: bool,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct TermDto {
    pub source: String,
    pub target: String,
    pub kind: String,
    pub frequency: u32,
    pub pinned: bool,
}

fn err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

fn client() -> Result<DeepSeekClient, String> {
    DeepSeekClient::new(Config::load()).map_err(err)
}

/// App data directory for working files (progress DBs), per XDG.
fn app_data_dir() -> std::path::PathBuf {
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".local/share")))
        .unwrap_or_else(std::env::temp_dir);
    base.join("book-converter")
}

/// Global settings DB (app-wide, survives restarts).
fn settings_db() -> std::path::PathBuf {
    crate::settings::db_path()
}

/// Read a persisted app setting (e.g. the UI language).
#[tauri::command]
pub async fn get_setting(key: String) -> Result<Option<String>, String> {
    crate::settings::get(&settings_db(), &key).map_err(err)
}

/// Persist an app setting.
#[tauri::command]
pub async fn set_setting(key: String, value: String) -> Result<(), String> {
    crate::settings::set(&settings_db(), &key, &value).map_err(err)
}

/// A project's own data directory (`<app_data>/projects/<id>/`).
fn project_dir(id: &str) -> std::path::PathBuf {
    app_data_dir().join("projects").join(id)
}

/// The resumable progress DB for a project. Kept in the app data directory (not
/// next to the book) so the source can live on a read-only mount without breaking.
fn db_path_for_project(id: &str) -> String {
    let dir = project_dir(id);
    let _ = std::fs::create_dir_all(&dir);
    dir.join("progress.db").to_string_lossy().into_owned()
}

/// Remove legacy flat `*.progress.db` files from before the per-project layout.
pub fn cleanup_legacy_data() {
    if let Ok(entries) = std::fs::read_dir(app_data_dir()) {
        for e in entries.flatten() {
            if e.file_name().to_string_lossy().ends_with(".progress.db") {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
}

/// Project manifest, stored as `project.json` and bundled into an archive so a
/// project is self-describing.
#[derive(Serialize, Deserialize, Default)]
struct Manifest {
    name: String,
    source_path: String,
    ref_path: Option<String>,
}

fn write_manifest(id: &str, source_path: &str, ref_path: Option<&str>) {
    let _ = std::fs::create_dir_all(project_dir(id));
    let name = Path::new(source_path)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("book")
        .to_string();
    let m = Manifest {
        name,
        source_path: source_path.to_string(),
        ref_path: ref_path.map(|s| s.to_string()),
    };
    if let Ok(bytes) = serde_json::to_vec_pretty(&m) {
        let _ = std::fs::write(project_dir(id).join("project.json"), bytes);
    }
}

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
        s.summary = saved_summary.or_else(|| head.as_ref().and_then(|h| h.annotation.clone()));
        s.cover = saved_cover
            .or_else(|| head.as_ref().and_then(|h| h.cover.clone()))
            .or(pdf_cover);
    });

    Ok(BookInfo {
        title,
        author,
        total_chapters: book.chapters.len(),
        format: format!("{:?}", book.format),
        encoding: book.encoding,
        needs_delimiter: book.needs_delimiter,
        missing: book.report.missing_numbers.len(),
        duplicates: book.report.duplicate_numbers.len(),
    })
}

/// When no chapter pattern matched, split the book intelligently: a PDF's table of
/// contents (outline) first, then a model-inferred delimiter, and finally the whole
/// book as one chapter so its text is never lost. Best-effort.
async fn ensure_chapters(path: &str, book: &mut crate::book::LoadedBook) {
    // 0. PDF with a table of contents (bookmarks): the most accurate split.
    if path.to_lowercase().ends_with(".pdf") {
        if let Ok(bytes) = std::fs::read(path) {
            if let Some(toc) = crate::book::extract_pdf_toc_chapters(&bytes) {
                if toc.len() >= 2 {
                    book.chapters = toc
                        .into_iter()
                        .enumerate()
                        .map(|(i, (title, body))| crate::book::Chapter {
                            index: i + 1,
                            number: None,
                            title: title.replace('_', " "),
                            body,
                        })
                        .collect();
                    book.report = crate::book::validate(&book.chapters, &book.meta);
                    book.needs_delimiter = false;
                    return;
                }
            }
        }
    }

    let text = match crate::book::read_book_file(Path::new(path)) {
        Ok(d) => d.text,
        Err(_) => return,
    };

    // 1. Ask the model to infer a chapter-heading regex for this layout.
    if let Ok(cl) = DeepSeekClient::new(Config::load()) {
        let (system, user) = crate::book::build_delimiter_prompt(&text);
        if let Ok(reply) = cl.translate(&system, &user).await {
            if let Ok(re) = crate::book::parse_inferred_pattern(&reply) {
                let chapters = crate::book::parse_chapters_with(&text, &re);
                if chapters.len() >= 2 {
                    book.report = crate::book::validate(&chapters, &book.meta);
                    book.chapters = chapters;
                    book.needs_delimiter = false;
                    return;
                }
            }
        }
    }

    // 2. Fallback: a single chapter with the whole text (still translatable).
    let title = book.meta.title.clone().unwrap_or_else(|| "Book".to_string());
    let body = text.trim().to_string();
    if !body.is_empty() {
        book.chapters = vec![crate::book::Chapter { index: 1, number: Some(1), title, body }];
        book.report = crate::book::validate(&book.chapters, &book.meta);
        book.needs_delimiter = false;
    }
}

/// Seed still-`pending` chapters from a reference translation (aligned by chapter
/// number), marking them `done` with `origin = 'reference'`. Never overwrites work
/// already done. Returns how many chapters were filled.
fn import_reference_pending(
    db: &str,
    source_path: &str,
    reference: &crate::reference::Reference,
) -> anyhow::Result<usize> {
    let source = load_book(Path::new(source_path))?;
    let idx_by_number: HashMap<usize, usize> = source
        .chapters
        .iter()
        .filter_map(|c| c.number.map(|n| (n, c.index)))
        .collect();
    let store = Store::open(db)?;
    let mut count = 0;
    for rc in &reference.chapters {
        if let Some(&idx) = rc.number.and_then(|n| idx_by_number.get(&n)) {
            if store.save_reference_chapter(idx, &rc.title, &rc.body)? {
                count += 1;
            }
        }
    }
    Ok(count)
}

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
    let db = db.ok_or("no source loaded")?;
    let source_path = source_path.ok_or("no source loaded")?;
    let reference = reference.ok_or("no reference loaded")?;

    let source = load_book(Path::new(&source_path)).map_err(err)?;
    let cl = client()?;
    let glossary = reference::bootstrap_glossary(&cl, &source.chapters, &reference, sample)
        .await
        .map_err(err)?;
    let store = Store::open(&db).map_err(err)?;
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
    let db = db.ok_or("no source loaded")?;
    let source_path = source_path.ok_or("no source loaded")?;
    let reference = reference.ok_or("no reference loaded")?;
    import_reference_pending(&db, &source_path, &reference).map_err(err)
}

/// Start translating pending chapters (up to `limit`) on a background thread.
#[tauri::command]
pub fn start_translation(
    project_id: String,
    limit: Option<usize>,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let res: Result<_, String> = state.with(&project_id, |s| {
        if s.running {
            return Err("a translation is already running".to_string());
        }
        let db = s.db_path.clone().ok_or("no source loaded")?;
        let cancel = Arc::new(AtomicBool::new(false));
        s.cancel = Some(cancel.clone());
        s.running = true;
        Ok((db, s.style.clone(), cancel))
    });
    let (db, style, cancel) = res?;

    let app2 = app.clone();
    let pid = project_id.clone();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("current-thread runtime");
        let result = rt.block_on(run_job(&pid, &db, style, limit, &cancel, &app2));

        if let Some(st) = app2.try_state::<AppState>() {
            st.with(&pid, |s| s.running = false);
        }
        match result {
            Ok(()) => {
                let _ = app2.emit("done", serde_json::json!({ "project": pid }));
            }
            Err(e) => {
                let _ = app2.emit("job_error", serde_json::json!({ "project": pid, "message": e.to_string() }));
            }
        }
    });

    Ok(())
}

async fn run_job(
    project_id: &str,
    db: &str,
    style: Option<String>,
    limit: Option<usize>,
    cancel: &AtomicBool,
    app: &AppHandle,
) -> anyhow::Result<()> {
    let config = Config::load();
    let cl = DeepSeekClient::new(config.clone())?;
    let store = Store::open(db)?;
    let mut orch = Orchestrator::new(&cl, &store, &config, style)?;
    orch.run(limit, cancel, |st| {
        let _ = app.emit(
            "progress",
            Progress {
                project: project_id.to_string(),
                done: st.done,
                total: st.total,
                failed: st.failed,
                pending: st.pending,
                running: true,
            },
        );
    })
    .await
}

/// Request a pause: the run stops after the current chapter.
#[tauri::command]
pub fn pause_translation(project_id: String, state: State<'_, AppState>) -> Result<(), String> {
    state.with(&project_id, |s| {
        if let Some(c) = &s.cancel {
            c.store(true, Ordering::Relaxed);
        }
    });
    Ok(())
}

/// Current progress.
#[tauri::command]
pub async fn get_progress(
    project_id: String,
    state: State<'_, AppState>,
) -> Result<Progress, String> {
    let (db, running) = state.with(&project_id, |s| (s.db_path.clone(), s.running));
    let db = db.ok_or("no source loaded")?;
    let store = Store::open(&db).map_err(err)?;
    let st = store.stats().map_err(err)?;
    Ok(Progress {
        project: project_id,
        done: st.done,
        total: st.total,
        failed: st.failed,
        pending: st.pending,
        running,
    })
}

/// Reset translated chapters back to `pending` for a fresh run with the current
/// glossary. `from_index` (0-based) limits it to that chapter onward; `None` resets
/// the whole book and also clears the rolling context summary. Returns how many
/// chapters were reset. The caller then calls `start_translation` to re-run them.
#[tauri::command]
pub async fn reset_translation(
    project_id: String,
    from_index: Option<usize>,
    state: State<'_, AppState>,
) -> Result<usize, String> {
    let (db, running) = state.with(&project_id, |s| (s.db_path.clone(), s.running));
    if running {
        return Err("a job is already running".into());
    }
    let db = db.ok_or("no source loaded")?;
    let store = Store::open(&db).map_err(err)?;
    let n = store.reset_from(from_index).map_err(err)?;
    // A full reset rebuilds context from scratch, so drop the rolling summary.
    if from_index.is_none() || from_index == Some(0) {
        let _ = store.set_meta("running_summary", "");
    }
    Ok(n)
}

/// The whole glossary (most frequent first).
#[tauri::command]
pub async fn get_glossary(
    project_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<TermDto>, String> {
    let db = state
        .with(&project_id, |s| s.db_path.clone())
        .ok_or("no source loaded")?;
    let store = Store::open(&db).map_err(err)?;
    let mut terms = store.load_glossary().map_err(err)?;
    terms.sort_by(|a, b| b.frequency.cmp(&a.frequency));
    Ok(terms.into_iter().map(term_to_dto).collect())
}

/// Manually edit / pin a term.
#[tauri::command]
pub async fn update_term(
    project_id: String,
    term: TermDto,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let db = state
        .with(&project_id, |s| s.db_path.clone())
        .ok_or("no source loaded")?;
    let store = Store::open(&db).map_err(err)?;
    let mut glossary = store.load_glossary().map_err(err)?;
    let updated = Term {
        source: term.source.clone(),
        target: term.target,
        kind: TermKind::from_label(&term.kind),
        frequency: term.frequency.max(1),
        pinned: true,
    };
    match glossary.iter_mut().find(|t| t.source == term.source) {
        Some(existing) => *existing = updated,
        None => glossary.push(updated),
    }
    store.save_glossary(&glossary).map_err(err)?;
    Ok(())
}

/// Remove a term from the glossary by its source form.
#[tauri::command]
pub async fn delete_term(
    project_id: String,
    source: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let db = state
        .with(&project_id, |s| s.db_path.clone())
        .ok_or("no source loaded")?;
    let store = Store::open(&db).map_err(err)?;
    store.delete_term(source.trim()).map_err(err)?;
    Ok(())
}

/// One rename to propagate into the existing translation.
#[derive(Deserialize)]
pub struct RenameChange {
    pub old_target: String,
    pub new_target: String,
    pub kind: String,
}

/// Propagate one or more renames into the already-translated text: rewrite (via
/// the model) only the paragraphs that mention an old rendering, replacing every
/// inflected form with the new one and fixing gender/case agreement. Runs on a
/// background thread; emits `retarget_progress` and finally `retarget_done`
/// (chapters changed).
#[tauri::command]
pub fn retarget_terms(
    project_id: String,
    changes: Vec<RenameChange>,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let changes: Vec<RenameChange> = changes
        .into_iter()
        .filter(|c| !c.old_target.trim().is_empty() && c.new_target.trim() != c.old_target.trim())
        .collect();
    if changes.is_empty() {
        return Err("nothing to update".into());
    }
    let res: Result<_, String> = state.with(&project_id, |s| {
        if s.running {
            return Err("a job is already running".to_string());
        }
        let db = s.db_path.clone().ok_or("no source loaded")?;
        let cancel = Arc::new(AtomicBool::new(false));
        s.cancel = Some(cancel.clone());
        s.running = true;
        Ok((db, cancel))
    });
    let (db, cancel) = res?;

    let app2 = app.clone();
    let pid = project_id.clone();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("current-thread runtime");
        let result = rt.block_on(run_retarget(&pid, &db, &changes, &cancel, &app2));

        if let Some(st) = app2.try_state::<AppState>() {
            st.with(&pid, |s| s.running = false);
        }
        match result {
            Ok(n) => {
                let _ = app2.emit("retarget_done", serde_json::json!({ "project": pid, "changed": n }));
            }
            Err(e) => {
                let _ = app2.emit("job_error", serde_json::json!({ "project": pid, "message": e.to_string() }));
            }
        }
    });
    Ok(())
}

async fn run_retarget(
    project_id: &str,
    db: &str,
    changes: &[RenameChange],
    cancel: &AtomicBool,
    app: &AppHandle,
) -> anyhow::Result<usize> {
    use crate::retarget::paragraph_mentions;

    let config = Config::load();
    let cl = DeepSeekClient::new(config.clone())?;
    let store = Store::open(db)?;
    let lang = &config.target_lang;

    let mentions_any = |text: &str| changes.iter().any(|c| paragraph_mentions(text, &c.old_target));

    // Only chapters whose title or a body line mentions one of the old renderings.
    let chapters = store.translated_chapters()?;
    let jobs: Vec<(usize, String, String)> = chapters
        .into_iter()
        .filter(|(_, title, body)| mentions_any(title) || body.lines().any(mentions_any))
        .collect();
    let total = jobs.len();

    let mut changed = 0usize;
    for (i, (idx, title, body)) in jobs.into_iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            break;
        }

        let new_title = apply_changes(project_id, &cl, lang, changes, &title, app).await;
        let mut out_lines: Vec<String> = Vec::with_capacity(body.lines().count());
        for line in body.lines() {
            out_lines.push(apply_changes(project_id, &cl, lang, changes, line, app).await);
        }
        let new_body = out_lines.join("\n");

        let did_change = new_title != title || new_body != body;
        if did_change {
            store.save_translation(idx, &new_title, &new_body)?;
            changed += 1;
        }
        let _ = app.emit(
            "retarget_progress",
            serde_json::json!({
                "project": project_id,
                "done": i + 1,
                "total": total,
                "title": new_title,
                "changed": did_change,
            }),
        );
    }
    Ok(changed)
}

/// Apply every relevant rename to one paragraph, in sequence (a paragraph that
/// mentions two renamed terms is rewritten once per term, each on the prior result).
/// A model failure on one paragraph is surfaced (a `retarget_warn` event) and the
/// original text is kept, so one bad paragraph never aborts the whole job.
async fn apply_changes(
    project_id: &str,
    cl: &DeepSeekClient,
    lang: &str,
    changes: &[RenameChange],
    text: &str,
    app: &AppHandle,
) -> String {
    use crate::retarget::paragraph_mentions;
    let mut cur = text.to_string();
    for c in changes {
        if paragraph_mentions(&cur, &c.old_target) {
            match rewrite_paragraph(cl, lang, &c.kind, &c.old_target, &c.new_target, &cur).await {
                Ok(Some(r)) => cur = r,
                Ok(None) => {}
                Err(e) => {
                    tracing::warn!("retarget rewrite failed: {e:#}");
                    let _ = app.emit(
                        "retarget_warn",
                        serde_json::json!({
                            "project": project_id,
                            "message": format!("{} -> {}: {e}", c.old_target, c.new_target),
                        }),
                    );
                }
            }
        }
    }
    cur
}

/// Rewrite one paragraph via the model, applying the rename. `Ok(None)` means an
/// empty response (keep original); `Err` means the request itself failed.
async fn rewrite_paragraph(
    cl: &DeepSeekClient,
    lang: &str,
    kind: &str,
    old_target: &str,
    new_target: &str,
    text: &str,
) -> anyhow::Result<Option<String>> {
    let (sys, user) = crate::retarget::rewrite_prompt(lang, kind, old_target, new_target, text);
    let out = cl.translate(&sys, &user).await?;
    let t = out.trim().trim_matches('"').trim().to_string();
    Ok((!t.is_empty()).then_some(t))
}

/// Export the translated chapters to `out_path` (format inferred from extension).
#[tauri::command]
pub async fn export_book(
    project_id: String,
    out_path: String,
    state: State<'_, AppState>,
) -> Result<String, String> {
    let (db, source_path, title, title_translated, author, author_translated, summary, cover, zipped_input) =
        state.with(&project_id, |s| {
            (
                s.db_path.clone(),
                s.source_path.clone(),
                s.title.clone(),
                s.title_translated.clone(),
                s.author.clone(),
                s.author_translated.clone(),
                s.summary.clone(),
                s.cover.clone(),
                s.zipped_input,
            )
        });
    let db = db.ok_or("no source loaded")?;
    let source_path = source_path.ok_or("no source loaded")?;

    // Decide zip vs plain, and the inner format. Zip when the path ends in .zip
    // or the input was itself zipped ("zip in → zip out").
    let out = OutputTarget::resolve(&out_path, zipped_input)?;

    let store = Store::open(&db).map_err(err)?;
    let rows = store.translated_chapters().map_err(err)?;
    if rows.is_empty() {
        return Err("nothing translated yet".into());
    }

    // Attach chapter numbers (for correct ordering / continuation).
    let source = load_book(Path::new(&source_path)).map_err(err)?;
    let num_by_idx: HashMap<usize, Option<usize>> =
        source.chapters.iter().map(|c| (c.index, c.number)).collect();
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
        export::export_zip(&chapters, out.format, &meta, &out.inner_name, Path::new(&out.path))
            .map_err(err)?;
    } else {
        export::export(&chapters, out.format, &meta, Path::new(&out.path)).map_err(err)?;
    }
    Ok(out.path)
}

/// Resolved output: final path, format, whether to zip, and the inner file name.
struct OutputTarget {
    path: String,
    format: OutputFormat,
    zipped: bool,
    inner_name: String,
}

impl OutputTarget {
    fn resolve(out_path: &str, zipped_input: bool) -> Result<Self, String> {
        let ends_zip = out_path.to_ascii_lowercase().ends_with(".zip");
        // The book file part (path without a trailing .zip).
        let inner_path = if ends_zip {
            out_path[..out_path.len() - 4].to_string()
        } else {
            out_path.to_string()
        };
        let format = OutputFormat::from_path(Path::new(&inner_path)).unwrap_or(OutputFormat::Fb2);
        // Binary formats (EPUB/PDF) are written directly, never re-zipped.
        let zipped = (ends_zip || zipped_input) && !format.is_binary();

        let ext = format.ext();
        let inner_stem = Path::new(&inner_path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("book");
        let inner_name = if inner_stem.to_ascii_lowercase().ends_with(&format!(".{ext}")) {
            inner_stem.to_string()
        } else {
            format!("{inner_stem}.{ext}")
        };
        let path = if !zipped {
            out_path.to_string()
        } else if ends_zip {
            out_path.to_string()
        } else {
            format!("{out_path}.zip")
        };

        Ok(OutputTarget {
            path,
            format,
            zipped,
            inner_name,
        })
    }
}

/// A chapter row for the reader's chapter list.
#[derive(Serialize)]
pub struct ChapterRow {
    pub idx: usize,
    pub number: Option<usize>,
    pub title: String,
    pub status: String,
    /// Where the translation came from: `"reference"`, `"model"`, or none.
    pub origin: Option<String>,
}

/// Full chapter view: original + translation.
#[derive(Serialize)]
pub struct ChapterView {
    pub idx: usize,
    pub number: Option<usize>,
    pub source_title: String,
    pub source: String,
    pub translated_title: Option<String>,
    pub translated: Option<String>,
    pub status: String,
    pub origin: Option<String>,
}

/// List chapters of the active project (for the reader).
#[tauri::command]
pub async fn list_chapters(
    project_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<ChapterRow>, String> {
    let db = state
        .with(&project_id, |s| s.db_path.clone())
        .ok_or("no source loaded")?;
    let store = Store::open(&db).map_err(err)?;
    let rows = store.list_chapters().map_err(err)?;
    Ok(rows
        .into_iter()
        .map(|(idx, number, title, status, origin)| ChapterRow { idx, number, title, status, origin })
        .collect())
}

/// Original + translation for one chapter.
#[tauri::command]
pub async fn get_chapter(
    project_id: String,
    index: usize,
    state: State<'_, AppState>,
) -> Result<ChapterView, String> {
    let db = state
        .with(&project_id, |s| s.db_path.clone())
        .ok_or("no source loaded")?;
    let store = Store::open(&db).map_err(err)?;
    let (number, source_title, source, status, translated_title, translated, origin) = store
        .chapter_full(index)
        .map_err(err)?
        .ok_or("chapter not found")?;
    Ok(ChapterView {
        idx: index,
        number,
        source_title,
        source,
        translated_title,
        translated,
        status,
        origin,
    })
}

/// Book cover + metadata for the UI.
#[derive(Serialize)]
pub struct BookDetails {
    pub title: String,
    pub author: String,
    pub title_translated: Option<String>,
    pub author_translated: Option<String>,
    pub summary: Option<String>,
    /// Cover as a `data:` URL, if any.
    pub cover: Option<String>,
}

/// Current book details for display (cover, summary, translated title).
#[tauri::command]
pub async fn get_book_details(
    project_id: String,
    state: State<'_, AppState>,
) -> Result<BookDetails, String> {
    Ok(state.with(&project_id, |s| BookDetails {
        title: s.title.clone().unwrap_or_default(),
        author: s.author.clone().unwrap_or_default(),
        title_translated: s.title_translated.clone(),
        author_translated: s.author_translated.clone(),
        summary: s.summary.clone(),
        cover: s.cover.as_ref().map(|c| c.data_url()),
    }))
}

/// Translate the source book title and author (auto). Keeps reference-provided
/// values. Translating the author matters for PDF, whose Latin/Cyrillic-only font
/// renders an untranslated CJK name as boxes.
#[tauri::command]
pub async fn translate_title(
    project_id: String,
    state: State<'_, AppState>,
) -> Result<String, String> {
    let (title, author, existing_title, existing_author) = state.with(&project_id, |s| {
        (
            s.title.clone(),
            s.author.clone(),
            s.title_translated.clone(),
            s.author_translated.clone(),
        )
    });
    let cfg = Config::load();

    // --- title ---
    let translated = match existing_title.filter(|t| !t.trim().is_empty()) {
        Some(t) => t,
        None => {
            let title =
                title.filter(|t| !t.trim().is_empty()).ok_or("no book title to translate")?;
            let system = format!(
                "Translate this book title from {} to {}. Output only the translated title, nothing else.",
                cfg.source_lang, cfg.target_lang
            );
            let out = client()?.translate(&system, &title).await.map_err(err)?;
            let out = out.trim().trim_matches('"').trim().to_string();
            let db = state.with(&project_id, |s| {
                s.title_translated = Some(out.clone());
                s.db_path.clone()
            });
            persist_meta(&db, "title_translated", &out);
            out
        }
    };

    // --- author (best-effort; a failure here must not fail the title) ---
    if existing_author.filter(|a| !a.trim().is_empty()).is_none() {
        if let Some(author) = author.filter(|a| !a.trim().is_empty()) {
            let system = format!(
                "Transliterate/translate this author name from {} to {}. Keep it a person's name (no extra words). Output only the name.",
                cfg.source_lang, cfg.target_lang
            );
            if let Ok(out) = client()?.translate(&system, &author).await {
                let out = out.trim().trim_matches('"').trim().to_string();
                if !out.is_empty() {
                    let db = state.with(&project_id, |s| {
                        s.author_translated = Some(out.clone());
                        s.db_path.clone()
                    });
                    persist_meta(&db, "author_translated", &out);
                }
            }
        }
    }

    Ok(translated)
}

/// Set / replace the annotation (summary).
#[tauri::command]
pub async fn set_summary(
    project_id: String,
    summary: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let db = state.with(&project_id, |s| {
        s.summary = if summary.trim().is_empty() { None } else { Some(summary.clone()) };
        s.db_path.clone()
    });
    persist_meta(&db, "summary", &summary);
    Ok(())
}

/// Generate a book annotation (summary) from the title + author via the model,
/// then persist it. Useful when the source file carries no annotation.
#[tauri::command]
pub async fn generate_summary(
    project_id: String,
    state: State<'_, AppState>,
) -> Result<String, String> {
    let (title, title_tr, author) = state.with(&project_id, |s| {
        (s.title.clone(), s.title_translated.clone(), s.author.clone())
    });
    let title = title.filter(|t| !t.trim().is_empty()).ok_or("no book title")?;
    let cfg = Config::load();

    let hint = title_tr
        .filter(|t| !t.trim().is_empty())
        .map(|t| format!(" (also known as \"{t}\")"))
        .unwrap_or_default();
    let author_line = author
        .filter(|a| !a.trim().is_empty())
        .map(|a| format!("\nAuthor: {a}"))
        .unwrap_or_default();

    let system = format!(
        "You are a librarian who writes concise book annotations in {}. \
         Write a 3 to 6 sentence annotation covering the premise, genre and tone, based on your knowledge of the \
         book and on what its title and author clearly convey. \
         Do not fabricate specific named characters or plot twists you have no basis for, but you may describe the evident premise and genre. \
         Only if the title is genuinely uninformative (for example just a personal name from which nothing can be said), \
         reply with exactly NOT_FOUND and nothing else. \
         Output only the annotation text, or NOT_FOUND: no heading, no quotes, no preamble.",
        cfg.target_lang
    );
    let user = format!("Title: {title}{hint}{author_line}");
    let out = client()?.translate(&system, &user).await.map_err(err)?;
    let out = out.trim().to_string();
    // The model signals an unknown book with NOT_FOUND (we told it not to invent one).
    if out.is_empty() || out.trim_start().to_uppercase().starts_with("NOT_FOUND") {
        return Err("book_not_found".into());
    }
    let db = state.with(&project_id, |s| {
        s.summary = Some(out.clone());
        s.db_path.clone()
    });
    persist_meta(&db, "summary", &out);
    Ok(out)
}

/// Replace the cover image from a file; returns its `data:` URL for preview.
#[tauri::command]
pub async fn set_cover(
    project_id: String,
    path: String,
    state: State<'_, AppState>,
) -> Result<String, String> {
    use base64::Engine as _;
    let bytes = std::fs::read(&path).map_err(err)?;
    let cover = Cover {
        content_type: cover_mime(&path),
        base64: base64::engine::general_purpose::STANDARD.encode(&bytes),
    };
    let url = cover.data_url();
    let db = state.with(&project_id, |s| {
        s.cover = Some(cover.clone());
        s.db_path.clone()
    });
    if let Some(db) = db {
        if let Ok(store) = Store::open(&db) {
            let _ = store.set_meta("cover_ct", &cover.content_type);
            let _ = store.set_meta("cover_b64", &cover.base64);
        }
    }
    Ok(url)
}

/// Lowercase file extension of a path, or `"bin"`.
fn ext_of(path: &str) -> String {
    std::path::Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("bin")
        .to_lowercase()
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

/// Save a project to a self-contained `.bcproj` archive (manifest + progress DB +
/// a copy of the source book), so it is portable and can be re-opened elsewhere.
#[tauri::command]
pub async fn export_project(
    project_id: String,
    out_path: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    use std::io::Write as _;

    let dir = project_dir(&project_id);
    let manifest_bytes = std::fs::read(dir.join("project.json")).map_err(err)?;
    let db_bytes = std::fs::read(dir.join("progress.db")).map_err(err)?;
    let source_path = state
        .with(&project_id, |s| s.source_path.clone())
        .or_else(|| serde_json::from_slice::<Manifest>(&manifest_bytes).ok().map(|m| m.source_path))
        .ok_or("no source loaded")?;
    let book_bytes =
        std::fs::read(&source_path).map_err(|e| format!("reading book {source_path}: {e}"))?;
    let book_name = format!("book.{}", ext_of(&source_path));

    let file = std::fs::File::create(&out_path).map_err(err)?;
    let mut zip = zip::ZipWriter::new(file);
    let opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    let entries: [(&str, &Vec<u8>); 3] = [
        ("project.json", &manifest_bytes),
        ("progress.db", &db_bytes),
        (book_name.as_str(), &book_bytes),
    ];
    for (name, bytes) in entries {
        zip.start_file(name, opts).map_err(err)?;
        zip.write_all(bytes).map_err(err)?;
    }
    zip.finish().map_err(err)?;
    Ok(())
}

/// Result of importing a `.bcproj` archive: enough for the frontend to register a
/// project row and open it.
#[derive(Serialize)]
pub struct ImportedProject {
    pub name: String,
    pub source_path: String,
}

/// Import a `.bcproj` archive into a new project directory and report its name and
/// the extracted book path (the frontend then loads it like any other project).
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
    let mut source_path: Option<String> = None;
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).map_err(err)?;
        let ename = entry.name().to_string();
        let mut buf = Vec::new();
        entry.read_to_end(&mut buf).map_err(err)?;
        if ename == "project.json" {
            if let Ok(m) = serde_json::from_slice::<Manifest>(&buf) {
                name = m.name;
            }
            std::fs::write(dir.join("project.json"), &buf).map_err(err)?;
        } else if ename == "progress.db" {
            std::fs::write(dir.join("progress.db"), &buf).map_err(err)?;
        } else if ename.starts_with("book.") {
            let p = dir.join(&ename);
            std::fs::write(&p, &buf).map_err(err)?;
            source_path = Some(p.to_string_lossy().into_owned());
        }
    }
    let source_path = source_path.ok_or("archive has no book file")?;
    write_manifest(&project_id, &source_path, None);
    Ok(ImportedProject { name, source_path })
}

/// Persist a single meta value to the current project's DB (best-effort).
fn persist_meta(db: &Option<String>, key: &str, value: &str) {
    if let Some(db) = db {
        if let Ok(store) = Store::open(db) {
            let _ = store.set_meta(key, value);
        }
    }
}

fn cover_mime(path: &str) -> String {
    let ext = std::path::Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "png" => "image/png",
        "gif" => "image/gif",
        "webp" => "image/webp",
        _ => "image/jpeg",
    }
    .to_string()
}

fn term_to_dto(t: Term) -> TermDto {
    TermDto {
        source: t.source,
        target: t.target,
        kind: t.kind.label().to_string(),
        frequency: t.frequency,
        pinned: t.pinned,
    }
}

