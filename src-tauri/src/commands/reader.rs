//! Reader / book-details commands.

use tauri::State;

use crate::config::Config;
use crate::dto::{err, BookDetails, ChapterRow, ChapterView, SearchChapter, SearchHit};
use crate::export::fb2::Cover;
use crate::session::AppState;
use crate::state::Store;

use super::util::{client, cover_mime, persist_meta};

/// List chapters of the active project (for the reader).
#[tauri::command]
pub async fn list_chapters(
    project_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<ChapterRow>, String> {
    let db = state
        .with(&project_id, |s| s.db_path.clone())
        .ok_or("no_source")?;
    let store = Store::open(&db).map_err(err)?;
    let rows = store.list_chapters().map_err(err)?;
    Ok(rows
        .into_iter()
        .map(|(idx, number, title, translated_title, status, origin)| ChapterRow {
            idx,
            number,
            title,
            translated_title,
            status,
            origin,
        })
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
        .ok_or("no_source")?;
    let store = Store::open(&db).map_err(err)?;
    let (number, source_title, source, status, translated_title, translated, origin, user_prompt) =
        store
            .chapter_full(index)
            .map_err(err)?
            .ok_or("chapter not found")?;
    let (rolling_summary, prev_tail) = store.context_before(index).map_err(err)?;
    Ok(ChapterView {
        idx: index,
        number,
        source_title,
        source,
        translated_title,
        translated,
        status,
        origin,
        user_prompt,
        rolling_summary: if rolling_summary.trim().is_empty() {
            None
        } else {
            Some(rolling_summary)
        },
        prev_tail,
    })
}

/// Literal find/replace across every stored translation in the project. Returns
/// the number of chapters changed. Deterministic counterpart to the glossary
/// retarget (which rewrites paragraphs via the model).
#[tauri::command]
pub async fn replace_in_book(
    project_id: String,
    find: String,
    replace: String,
    match_case: bool,
    whole_word: bool,
    // The find bar's regex mode: the query is a pattern, not a literal.
    regex: bool,
    state: State<'_, AppState>,
) -> Result<usize, String> {
    if find.is_empty() {
        return Ok(0);
    }
    let db = state
        .with(&project_id, |s| s.db_path.clone())
        .ok_or("no_source")?;
    let mut pat = if regex { find.clone() } else { regex::escape(&find) };
    if whole_word {
        pat = format!(r"\b{pat}\b");
    }
    let re = regex::RegexBuilder::new(&pat)
        .case_insensitive(!match_case)
        .build()
        .map_err(err)?;
    let store = Store::open(&db).map_err(err)?;
    store.replace_in_translations(&re, &replace, regex).map_err(err)
}

/// Book-wide search, grouped per chapter like an IDE's search view. Searches the
/// translation by default, the original with `in_source`. Results are clipped
/// (see the constants below) so a common word cannot flood the UI.
#[tauri::command]
pub async fn search_book(
    project_id: String,
    query: String,
    match_case: bool,
    whole_word: bool,
    regex: bool,
    in_source: bool,
    state: State<'_, AppState>,
) -> Result<Vec<SearchChapter>, String> {
    /// Matching lines kept per chapter.
    const MAX_HITS_PER_CHAPTER: usize = 30;
    /// Chapters reported, at most.
    const MAX_CHAPTERS: usize = 300;
    /// Characters kept around a match in the preview.
    const PREVIEW: usize = 160;

    if query.trim().is_empty() {
        return Ok(Vec::new());
    }
    let db = state
        .with(&project_id, |s| s.db_path.clone())
        .ok_or("no_source")?;
    let mut pat = if regex { query.clone() } else { regex::escape(&query) };
    if whole_word {
        pat = format!(r"\b{pat}\b");
    }
    let re = regex::RegexBuilder::new(&pat)
        .case_insensitive(!match_case)
        .build()
        .map_err(err)?;

    let store = Store::open(&db).map_err(err)?;
    let mut out = Vec::new();
    for (idx, number, title, text) in store.searchable_chapters(in_source).map_err(err)? {
        let mut hits = Vec::new();
        let mut count = 0usize;
        for (n, line) in text.lines().enumerate() {
            let Some(m) = re.find(line) else { continue };
            count += re.find_iter(line).count();
            if hits.len() < MAX_HITS_PER_CHAPTER {
                hits.push(SearchHit {
                    line: n + 1,
                    preview: clip_around(line, m.start(), PREVIEW),
                });
            }
        }
        if count > 0 {
            out.push(SearchChapter { idx, number, title, count, hits });
            if out.len() >= MAX_CHAPTERS {
                break;
            }
        }
    }
    Ok(out)
}

/// Keep `width` characters around `at`, on character boundaries, with ellipses
/// where the line was cut.
fn clip_around(line: &str, at: usize, width: usize) -> String {
    let line = line.trim();
    if line.chars().count() <= width {
        return line.to_string();
    }
    // Character index of the match (byte offsets shift once the line is trimmed,
    // so locate it by counting characters up to `at` in the untrimmed line).
    let head = line.char_indices().take_while(|(i, _)| *i < at).count();
    let start = head.saturating_sub(width / 3);
    let clipped: String = line.chars().skip(start).take(width).collect();
    let prefix = if start > 0 { "…" } else { "" };
    let suffix = if start + width < line.chars().count() { "…" } else { "" };
    format!("{prefix}{clipped}{suffix}")
}

/// Set or clear the per-chapter user instruction (empty string clears it).
/// Used before re-translating a chapter with custom guidance.
#[tauri::command]
pub async fn set_chapter_prompt(
    project_id: String,
    index: usize,
    prompt: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let db = state
        .with(&project_id, |s| s.db_path.clone())
        .ok_or("no_source")?;
    let store = Store::open(&db).map_err(err)?;
    store
        .set_chapter_user_prompt(index, &prompt)
        .map_err(err)?;
    Ok(())
}

/// Edit the rolling continuity context used when translating this chapter
/// (story synopsis + previous-chapter tail).
#[tauri::command]
pub async fn set_chapter_context(
    project_id: String,
    index: usize,
    summary: String,
    prev_tail: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let db = state
        .with(&project_id, |s| s.db_path.clone())
        .ok_or("no_source")?;
    let store = Store::open(&db).map_err(err)?;
    store
        .set_context_before(index, &summary, &prev_tail)
        .map_err(err)?;
    Ok(())
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
            let title = title.filter(|t| !t.trim().is_empty()).ok_or("no_title")?;
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
        s.summary = if summary.trim().is_empty() {
            None
        } else {
            Some(summary.clone())
        };
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
    let title = title.filter(|t| !t.trim().is_empty()).ok_or("no_title")?;
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
