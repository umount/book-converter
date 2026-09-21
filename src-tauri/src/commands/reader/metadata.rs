//! Book metadata and cover commands.

use tauri::State;

use crate::config::Config;
use crate::dto::{err, BookDetails};
use crate::export::fb2::Cover;
use crate::session::AppState;

use super::super::ops;
use super::super::util::{client, cover_mime};

#[tauri::command]
pub async fn get_book_details(
    project_id: String,
    state: State<'_, AppState>,
) -> Result<BookDetails, String> {
    let store = ops::project_store(&state, &project_id)?;
    let metadata = store.project_metadata().map_err(err)?;
    let cover = match (metadata.cover_content_type, metadata.cover_base64) {
        (Some(content_type), Some(base64)) => Some(Cover {
            content_type,
            base64,
        }),
        _ => None,
    };
    Ok(BookDetails {
        title: metadata.title.unwrap_or_default(),
        author: metadata.author.unwrap_or_default(),
        title_translated: metadata.title_translated,
        author_translated: metadata.author_translated,
        summary: metadata.summary,
        cover: cover.as_ref().map(Cover::data_url),
        book_prompt: metadata.book_prompt,
    })
}

#[tauri::command]
pub async fn set_book_prompt(
    project_id: String,
    prompt: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    ops::translation::set_book_prompt(&state, &project_id, &prompt)
}

#[tauri::command]
pub async fn translate_title(
    project_id: String,
    state: State<'_, AppState>,
) -> Result<String, String> {
    let store = ops::project_store(&state, &project_id)?;
    let metadata = store.project_metadata().map_err(err)?;
    let config = Config::load();

    let translated = match metadata
        .title_translated
        .filter(|title| !title.trim().is_empty())
    {
        Some(title) => title,
        None => {
            let title = metadata
                .title
                .filter(|title| !title.trim().is_empty())
                .ok_or("no_title")?;
            let system = format!(
                "Translate this book title from {} to {}. Output only the translated title, nothing else.",
                config.source_lang, config.target_lang
            );
            let output = client()?.translate(&system, &title).await.map_err(err)?;
            let output = clean_model_value(&output);
            store.set_meta("title_translated", &output).map_err(err)?;
            output
        }
    };

    if metadata
        .author_translated
        .filter(|author| !author.trim().is_empty())
        .is_none()
    {
        if let Some(author) = metadata.author.filter(|author| !author.trim().is_empty()) {
            let system = format!(
                "Transliterate/translate this author name from {} to {}. Keep it a person's name (no extra words). Output only the name.",
                config.source_lang, config.target_lang
            );
            if let Ok(output) = client()?.translate(&system, &author).await {
                let output = clean_model_value(&output);
                if !output.is_empty() {
                    store.set_meta("author_translated", &output).map_err(err)?;
                }
            }
        }
    }

    Ok(translated)
}

#[tauri::command]
pub async fn set_summary(
    project_id: String,
    summary: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    ops::project_store(&state, &project_id)?
        .set_meta("summary", summary.trim())
        .map_err(err)
}

#[tauri::command]
pub async fn generate_summary(
    project_id: String,
    state: State<'_, AppState>,
) -> Result<String, String> {
    let store = ops::project_store(&state, &project_id)?;
    let metadata = store.project_metadata().map_err(err)?;
    let title = metadata
        .title
        .filter(|title| !title.trim().is_empty())
        .ok_or("no_title")?;
    let config = Config::load();

    let hint = metadata
        .title_translated
        .filter(|title| !title.trim().is_empty())
        .map(|title| format!(" (also known as \"{title}\")"))
        .unwrap_or_default();
    let author_line = metadata
        .author
        .filter(|author| !author.trim().is_empty())
        .map(|author| format!("\nAuthor: {author}"))
        .unwrap_or_default();

    let system = format!(
        "You are a librarian who writes concise book annotations in {}. \
         Write a 3 to 6 sentence annotation covering the premise, genre and tone, based on your knowledge of the \
         book and on what its title and author clearly convey. \
         Do not fabricate specific named characters or plot twists you have no basis for, but you may describe the evident premise and genre. \
         Only if the title is genuinely uninformative (for example just a personal name from which nothing can be said), \
         reply with exactly NOT_FOUND and nothing else. \
         Output only the annotation text, or NOT_FOUND: no heading, no quotes, no preamble.",
        config.target_lang
    );
    let user = format!("Title: {title}{hint}{author_line}");
    let output = client()?
        .translate(&system, &user)
        .await
        .map_err(err)?
        .trim()
        .to_string();
    if output.is_empty() || output.trim_start().to_uppercase().starts_with("NOT_FOUND") {
        return Err("book_not_found".into());
    }
    store.set_meta("summary", &output).map_err(err)?;
    Ok(output)
}

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
    ops::project_store(&state, &project_id)?
        .set_cover_meta(Some(&cover.content_type), Some(&cover.base64))
        .map_err(err)?;
    Ok(url)
}

fn clean_model_value(value: &str) -> String {
    value.trim().trim_matches('"').trim().to_string()
}
