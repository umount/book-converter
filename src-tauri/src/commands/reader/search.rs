//! Book-wide search and deterministic replacement commands.

use tauri::State;

use crate::dto::{err, SearchChapter, SearchHit};
use crate::session::AppState;
use crate::state::Store;

#[tauri::command]
pub async fn replace_in_book(
    project_id: String,
    find: String,
    replace: String,
    match_case: bool,
    whole_word: bool,
    regex: bool,
    state: State<'_, AppState>,
) -> Result<usize, String> {
    if find.is_empty() {
        return Ok(0);
    }
    let db = state
        .with(&project_id, |session| session.db_path.clone())
        .ok_or("no_source")?;
    let expand = regex;
    let regex = build_regex(&find, match_case, whole_word, expand)?;
    blocking(move || {
        Store::open(&db)?.replace_in_translations(&regex, &replace, expand)
    })
    .await
}

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
    if query.trim().is_empty() {
        return Ok(Vec::new());
    }
    let db = state
        .with(&project_id, |session| session.db_path.clone())
        .ok_or("no_source")?;
    let regex = build_regex(&query, match_case, whole_word, regex)?;
    blocking(move || search_all(&db, &regex, in_source)).await
}

fn build_regex(
    query: &str,
    match_case: bool,
    whole_word: bool,
    regex_mode: bool,
) -> Result<regex::Regex, String> {
    let mut pattern = if regex_mode {
        query.to_string()
    } else {
        regex::escape(query)
    };
    if whole_word {
        pattern = format!(r"\b{pattern}\b");
    }
    regex::RegexBuilder::new(&pattern)
        .case_insensitive(!match_case)
        .build()
        .map_err(err)
}

fn search_all(
    db: &str,
    regex: &regex::Regex,
    in_source: bool,
) -> anyhow::Result<Vec<SearchChapter>> {
    const MAX_HITS_PER_CHAPTER: usize = 30;
    const MAX_CHAPTERS: usize = 300;
    const PREVIEW: usize = 160;

    let store = Store::open(db)?;
    let mut output = Vec::new();
    for chapter in store.searchable_chapters(in_source)? {
        let mut hits = Vec::new();
        let mut count = 0usize;
        for (line_index, line) in chapter.text.lines().enumerate() {
            let Some(first_match) = regex.find(line) else {
                continue;
            };
            count += regex.find_iter(line).count();
            if hits.len() < MAX_HITS_PER_CHAPTER {
                hits.push(SearchHit {
                    line: line_index + 1,
                    preview: clip_around(line, first_match.start(), PREVIEW),
                });
            }
        }
        if count > 0 {
            output.push(SearchChapter {
                idx: chapter.idx,
                number: chapter.number,
                title: chapter.title,
                count,
                hits,
            });
            if output.len() >= MAX_CHAPTERS {
                break;
            }
        }
    }
    Ok(output)
}

async fn blocking<T, F>(work: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce() -> anyhow::Result<T> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|error| err(anyhow::anyhow!("background task failed: {error}")))?
        .map_err(err)
}

fn clip_around(line: &str, at: usize, width: usize) -> String {
    let line = line.trim();
    if line.chars().count() <= width {
        return line.to_string();
    }
    let head = line.char_indices().take_while(|(index, _)| *index < at).count();
    let start = head.saturating_sub(width / 3);
    let clipped: String = line.chars().skip(start).take(width).collect();
    let prefix = if start > 0 { "…" } else { "" };
    let suffix = if start + width < line.chars().count() {
        "…"
    } else {
        ""
    };
    format!("{prefix}{clipped}{suffix}")
}
