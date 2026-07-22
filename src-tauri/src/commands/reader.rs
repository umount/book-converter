//! Reader / book-details commands.

use tauri::State;

use crate::config::Config;
use crate::dto::{err, BookDetails, ChapterRow, ChapterView};
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
        .map(|(idx, number, title, status, origin)| ChapterRow {
            idx,
            number,
            title,
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
