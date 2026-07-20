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

/// Where the resumable progress DB lives for a given source file.
fn db_path_for(source: &str) -> String {
    format!("{source}.progress.db")
}

/// Load a source book (TXT/FB2), open/create its progress DB, and register chapters.
#[tauri::command]
pub fn load_source(path: String, state: State<AppState>) -> Result<BookInfo, String> {
    let book = load_book(Path::new(&path)).map_err(err)?;
    let db = db_path_for(&path);
    let store = Store::open(&db).map_err(err)?;
    store.init_chapters(&book.chapters).map_err(err)?;

    let title = book.meta.title.clone().unwrap_or_default();
    let author = book.meta.author.clone().unwrap_or_default();

    {
        let mut s = state.0.lock().unwrap();
        s.zipped_input |= crate::book::source::is_zip(Path::new(&path));
        s.db_path = Some(db);
        s.source_path = Some(path);
        s.title = book.meta.title.clone();
        s.author = book.meta.author.clone();
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

/// Load a reference translation (used for canon/style and "continue" mode).
#[tauri::command]
pub fn load_reference(path: String, state: State<AppState>) -> Result<RefInfo, String> {
    let reference = reference::load_reference(Path::new(&path)).map_err(err)?;
    let info = RefInfo {
        title: reference.meta.title.clone().unwrap_or_default(),
        chapters: reference.chapters.len(),
        max_covered: reference::max_covered_number(&reference),
    };
    let style = reference::style_exemplar(&reference, 600);
    let mut s = state.0.lock().unwrap();
    s.zipped_input |= crate::book::source::is_zip(Path::new(&path));
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

/// "Continue" mode: mark chapters the reference already covers as done, using the
/// professional text, so only the remaining chapters get machine-translated.
#[tauri::command]
pub fn use_reference_as_base(state: State<AppState>) -> Result<usize, String> {
    let (db, source_path, reference) = {
        let s = state.0.lock().unwrap();
        (s.db_path.clone(), s.source_path.clone(), s.reference.clone())
    };
    let db = db.ok_or("no source loaded")?;
    let source_path = source_path.ok_or("no source loaded")?;
    let reference = reference.ok_or("no reference loaded")?;

    let source = load_book(Path::new(&source_path)).map_err(err)?;
    let idx_by_number: HashMap<usize, usize> = source
        .chapters
        .iter()
        .filter_map(|c| c.number.map(|n| (n, c.index)))
        .collect();

    let store = Store::open(&db).map_err(err)?;
    let mut count = 0;
    for rc in &reference.chapters {
        if let (Some(n), Some(&idx)) = (rc.number, rc.number.and_then(|n| idx_by_number.get(&n))) {
            let _ = n;
            store.save_translation(idx, &rc.title, &rc.body).map_err(err)?;
            count += 1;
        }
    }
    Ok(count)
}

/// Start translating pending chapters (up to `limit`) on a background thread.
#[tauri::command]
pub fn start_translation(
    limit: Option<usize>,
    app: AppHandle,
    state: State<AppState>,
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
pub fn pause_translation(state: State<AppState>) -> Result<(), String> {
    let s = state.0.lock().unwrap();
    if let Some(c) = &s.cancel {
        c.store(true, Ordering::Relaxed);
    }
    Ok(())
}

/// Current progress.
#[tauri::command]
pub fn get_progress(state: State<AppState>) -> Result<Progress, String> {
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

/// The whole glossary (most frequent first).
#[tauri::command]
pub fn get_glossary(state: State<AppState>) -> Result<Vec<TermDto>, String> {
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
pub fn update_term(term: TermDto, state: State<AppState>) -> Result<(), String> {
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

/// Export the translated chapters to `out_path` (format inferred from extension).
#[tauri::command]
pub fn export_book(out_path: String, state: State<AppState>) -> Result<String, String> {
    let (db, source_path, title, author, head, zipped_input) = {
        let s = state.0.lock().unwrap();
        (
            s.db_path.clone(),
            s.source_path.clone(),
            s.title.clone(),
            s.author.clone(),
            s.reference.as_ref().and_then(|r| r.head.clone()),
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
    export::normalize_titles(&mut chapters, chapter_label(&config.target_lang));
    let meta = OutputMeta {
        title: title.unwrap_or_else(|| "Untitled".into()),
        author: author.unwrap_or_else(|| "Unknown".into()),
        lang: lang_code(&config.target_lang),
    };

    if out.zipped {
        export::export_zip(
            &chapters,
            out.format,
            &meta,
            head.as_ref(),
            &out.inner_name,
            Path::new(&out.path),
        )
        .map_err(err)?;
    } else {
        export::export(&chapters, out.format, &meta, head.as_ref(), Path::new(&out.path))
            .map_err(err)?;
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
        // EPUB is already a zip container, so never re-zip it.
        let zipped = (ends_zip || zipped_input) && !format.is_container();

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

fn term_to_dto(t: Term) -> TermDto {
    TermDto {
        source: t.source,
        target: t.target,
        kind: t.kind.label().to_string(),
        frequency: t.frequency,
        pinned: t.pinned,
    }
}

/// Chapter-heading label in the target language (for normalizing output titles).
fn chapter_label(target_lang: &str) -> &'static str {
    let l = target_lang.to_lowercase();
    if l.contains("russ") || l.contains("рус") {
        "Глава"
    } else {
        "Chapter"
    }
}

/// Best-effort BCP-47-ish language code for FB2 `<lang>`.
fn lang_code(target_lang: &str) -> String {
    let l = target_lang.to_lowercase();
    if l.contains("russ") || l.contains("рус") {
        "ru".into()
    } else if l.contains("engl") || l.contains("англ") {
        "en".into()
    } else if l.contains("chin") || l.contains("кит") {
        "zh".into()
    } else {
        "und".into()
    }
}
