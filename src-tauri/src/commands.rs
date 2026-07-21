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

/// Managed app state.
pub struct AppState(pub Mutex<Session>);

impl AppState {
    pub fn new() -> Self {
        AppState(Mutex::new(Session::default()))
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
    app_data_dir().join("settings.db")
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

/// Where the resumable progress DB lives for a given source file.
///
/// Kept in the app data directory (not next to the book) so the source can live
/// on a read-only or permission-restricted mount (USB, /media/…) without breaking.
fn db_path_for(source: &str) -> String {
    use std::hash::{Hash, Hasher};

    let dir = app_data_dir();
    let _ = std::fs::create_dir_all(&dir);

    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    source.hash(&mut hasher);
    let stem = std::path::Path::new(source)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("book");
    let safe: String = stem
        .chars()
        .take(40)
        .map(|c| if c.is_alphanumeric() { c } else { '_' })
        .collect();

    dir.join(format!("{safe}-{:016x}.progress.db", hasher.finish()))
        .to_string_lossy()
        .into_owned()
}

/// Load a source book (TXT/FB2), open/create its progress DB, and register chapters.
#[tauri::command]
pub async fn load_source(path: String, state: State<'_, AppState>) -> Result<BookInfo, String> {
    let book = load_book(Path::new(&path)).map_err(err)?;
    let db = db_path_for(&path);
    let store = Store::open(&db).map_err(err)?;
    store.init_chapters(&book.chapters).map_err(err)?;

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

    {
        // Switching projects: reset session, then load this project's state.
        let mut s = state.0.lock().unwrap();
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
        s.cover = saved_cover.or_else(|| head.as_ref().and_then(|h| h.cover.clone()));
    }

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
pub async fn load_reference(path: String, state: State<'_, AppState>) -> Result<RefInfo, String> {
    let reference = reference::load_reference(Path::new(&path)).map_err(err)?;

    // Seed pending chapters from the reference, if a source book is open.
    let (db, source_path) = {
        let s = state.0.lock().unwrap();
        (s.db_path.clone(), s.source_path.clone())
    };
    let imported = match (&db, &source_path) {
        (Some(db), Some(sp)) => import_reference_pending(db, sp, &reference).map_err(err)?,
        _ => 0,
    };

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

    let mut s = state.0.lock().unwrap();
    s.zipped_input |= crate::book::source::is_zip(Path::new(&path));
    // A reference is a translation, so its title/summary/cover are already in the
    // target language — adopt them unless the user has set their own.
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
    Ok(info)
}

/// Bootstrap a pinned glossary from `sample` aligned reference chapters.
#[tauri::command]
pub async fn bootstrap_glossary(
    sample: usize,
    state: State<'_, AppState>,
) -> Result<usize, String> {
    let (db, source_path, reference) = {
        let s = state.0.lock().unwrap();
        (s.db_path.clone(), s.source_path.clone(), s.reference.clone())
    };
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
pub async fn use_reference_as_base(state: State<'_, AppState>) -> Result<usize, String> {
    let (db, source_path, reference) = {
        let s = state.0.lock().unwrap();
        (s.db_path.clone(), s.source_path.clone(), s.reference.clone())
    };
    let db = db.ok_or("no source loaded")?;
    let source_path = source_path.ok_or("no source loaded")?;
    let reference = reference.ok_or("no reference loaded")?;
    import_reference_pending(&db, &source_path, &reference).map_err(err)
}

/// Start translating pending chapters (up to `limit`) on a background thread.
#[tauri::command]
pub fn start_translation(
    limit: Option<usize>,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let (db, style, cancel) = {
        let mut s = state.0.lock().unwrap();
        if s.running {
            return Err("a translation is already running".into());
        }
        let db = s.db_path.clone().ok_or("no source loaded")?;
        let cancel = Arc::new(AtomicBool::new(false));
        s.cancel = Some(cancel.clone());
        s.running = true;
        (db, s.style.clone(), cancel)
    };

    let app2 = app.clone();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("current-thread runtime");
        let result = rt.block_on(run_job(&db, style, limit, &cancel, &app2));

        if let Some(st) = app2.try_state::<AppState>() {
            st.0.lock().unwrap().running = false;
        }
        match result {
            Ok(()) => {
                let _ = app2.emit("done", ());
            }
            Err(e) => {
                let _ = app2.emit("job_error", e.to_string());
            }
        }
    });

    Ok(())
}

async fn run_job(
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
pub fn pause_translation(state: State<'_, AppState>) -> Result<(), String> {
    let s = state.0.lock().unwrap();
    if let Some(c) = &s.cancel {
        c.store(true, Ordering::Relaxed);
    }
    Ok(())
}

/// Current progress.
#[tauri::command]
pub async fn get_progress(state: State<'_, AppState>) -> Result<Progress, String> {
    let (db, running) = {
        let s = state.0.lock().unwrap();
        (s.db_path.clone(), s.running)
    };
    let db = db.ok_or("no source loaded")?;
    let store = Store::open(&db).map_err(err)?;
    let st = store.stats().map_err(err)?;
    Ok(Progress {
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
    from_index: Option<usize>,
    state: State<'_, AppState>,
) -> Result<usize, String> {
    let (db, running) = {
        let s = state.0.lock().unwrap();
        (s.db_path.clone(), s.running)
    };
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
pub async fn get_glossary(state: State<'_, AppState>) -> Result<Vec<TermDto>, String> {
    let db = state
        .0
        .lock()
        .unwrap()
        .db_path
        .clone()
        .ok_or("no source loaded")?;
    let store = Store::open(&db).map_err(err)?;
    let mut terms = store.load_glossary().map_err(err)?;
    terms.sort_by(|a, b| b.frequency.cmp(&a.frequency));
    Ok(terms.into_iter().map(term_to_dto).collect())
}

/// Manually edit / pin a term.
#[tauri::command]
pub async fn update_term(term: TermDto, state: State<'_, AppState>) -> Result<(), String> {
    let db = state
        .0
        .lock()
        .unwrap()
        .db_path
        .clone()
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
pub async fn delete_term(source: String, state: State<'_, AppState>) -> Result<(), String> {
    let db = state
        .0
        .lock()
        .unwrap()
        .db_path
        .clone()
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
    let (db, cancel) = {
        let mut s = state.0.lock().unwrap();
        if s.running {
            return Err("a job is already running".into());
        }
        let db = s.db_path.clone().ok_or("no source loaded")?;
        let cancel = Arc::new(AtomicBool::new(false));
        s.cancel = Some(cancel.clone());
        s.running = true;
        (db, cancel)
    };

    let app2 = app.clone();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("current-thread runtime");
        let result = rt.block_on(run_retarget(&db, &changes, &cancel, &app2));

        if let Some(st) = app2.try_state::<AppState>() {
            st.0.lock().unwrap().running = false;
        }
        match result {
            Ok(n) => {
                let _ = app2.emit("retarget_done", n);
            }
            Err(e) => {
                let _ = app2.emit("job_error", e.to_string());
            }
        }
    });
    Ok(())
}

async fn run_retarget(
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

        let new_title = apply_changes(&cl, lang, changes, &title, app).await;
        let mut out_lines: Vec<String> = Vec::with_capacity(body.lines().count());
        for line in body.lines() {
            out_lines.push(apply_changes(&cl, lang, changes, line, app).await);
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
                        format!("{} → {}: {e}", c.old_target, c.new_target),
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
pub async fn export_book(out_path: String, state: State<'_, AppState>) -> Result<String, String> {
    let (db, source_path, title, title_translated, author, author_translated, summary, cover, zipped_input) = {
        let s = state.0.lock().unwrap();
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
    };
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
pub async fn list_chapters(state: State<'_, AppState>) -> Result<Vec<ChapterRow>, String> {
    let db = state.0.lock().unwrap().db_path.clone().ok_or("no source loaded")?;
    let store = Store::open(&db).map_err(err)?;
    let rows = store.list_chapters().map_err(err)?;
    Ok(rows
        .into_iter()
        .map(|(idx, number, title, status, origin)| ChapterRow { idx, number, title, status, origin })
        .collect())
}

/// Original + translation for one chapter.
#[tauri::command]
pub async fn get_chapter(index: usize, state: State<'_, AppState>) -> Result<ChapterView, String> {
    let db = state.0.lock().unwrap().db_path.clone().ok_or("no source loaded")?;
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
pub async fn get_book_details(state: State<'_, AppState>) -> Result<BookDetails, String> {
    let s = state.0.lock().unwrap();
    Ok(BookDetails {
        title: s.title.clone().unwrap_or_default(),
        author: s.author.clone().unwrap_or_default(),
        title_translated: s.title_translated.clone(),
        author_translated: s.author_translated.clone(),
        summary: s.summary.clone(),
        cover: s.cover.as_ref().map(|c| c.data_url()),
    })
}

/// Translate the source book title and author (auto). Keeps reference-provided
/// values. Translating the author matters for PDF, whose Latin/Cyrillic-only font
/// renders an untranslated CJK name as boxes.
#[tauri::command]
pub async fn translate_title(state: State<'_, AppState>) -> Result<String, String> {
    let (title, author, existing_title, existing_author) = {
        let s = state.0.lock().unwrap();
        (
            s.title.clone(),
            s.author.clone(),
            s.title_translated.clone(),
            s.author_translated.clone(),
        )
    };
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
            let db = {
                let mut s = state.0.lock().unwrap();
                s.title_translated = Some(out.clone());
                s.db_path.clone()
            };
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
                    let db = {
                        let mut s = state.0.lock().unwrap();
                        s.author_translated = Some(out.clone());
                        s.db_path.clone()
                    };
                    persist_meta(&db, "author_translated", &out);
                }
            }
        }
    }

    Ok(translated)
}

/// Set / replace the annotation (summary).
#[tauri::command]
pub async fn set_summary(summary: String, state: State<'_, AppState>) -> Result<(), String> {
    let db = {
        let mut s = state.0.lock().unwrap();
        s.summary = if summary.trim().is_empty() { None } else { Some(summary.clone()) };
        s.db_path.clone()
    };
    persist_meta(&db, "summary", &summary);
    Ok(())
}

/// Generate a book annotation (summary) from the title + author via the model,
/// then persist it. Useful when the source file carries no annotation.
#[tauri::command]
pub async fn generate_summary(state: State<'_, AppState>) -> Result<String, String> {
    let (title, title_tr, author) = {
        let s = state.0.lock().unwrap();
        (s.title.clone(), s.title_translated.clone(), s.author.clone())
    };
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
         Given a title and author, write a 3 to 6 sentence annotation covering the premise, genre and tone. \
         If you do not know the exact book, infer a plausible annotation from the meaning of the title. \
         Output only the annotation text: no heading, no quotes, no preamble.",
        cfg.target_lang
    );
    let user = format!("Title: {title}{hint}{author_line}");
    let out = client()?.translate(&system, &user).await.map_err(err)?;
    let out = out.trim().to_string();
    if out.is_empty() {
        return Err("the model returned an empty summary".into());
    }
    let db = {
        let mut s = state.0.lock().unwrap();
        s.summary = Some(out.clone());
        s.db_path.clone()
    };
    persist_meta(&db, "summary", &out);
    Ok(out)
}

/// Replace the cover image from a file; returns its `data:` URL for preview.
#[tauri::command]
pub async fn set_cover(path: String, state: State<'_, AppState>) -> Result<String, String> {
    use base64::Engine as _;
    let bytes = std::fs::read(&path).map_err(err)?;
    let cover = Cover {
        content_type: cover_mime(&path),
        base64: base64::engine::general_purpose::STANDARD.encode(&bytes),
    };
    let url = cover.data_url();
    let db = {
        let mut s = state.0.lock().unwrap();
        s.cover = Some(cover.clone());
        s.db_path.clone()
    };
    if let Some(db) = db {
        if let Ok(store) = Store::open(&db) {
            let _ = store.set_meta("cover_ct", &cover.content_type);
            let _ = store.set_meta("cover_b64", &cover.base64);
        }
    }
    Ok(url)
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

