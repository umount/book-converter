//! Book-wide search and deterministic replacement.

use tauri::AppHandle;

use crate::dto::{err, SearchChapter, SearchHit};
use crate::jobs;
use crate::session::AppState;
use crate::state::Store;

pub(crate) struct SearchLimits {
    pub(crate) max_hits_per_chapter: usize,
    pub(crate) max_chapters: usize,
    pub(crate) preview: usize,
}

impl SearchLimits {
    pub(crate) const UI: Self = SearchLimits {
        max_hits_per_chapter: 30,
        max_chapters: 300,
        preview: 160,
    };
    pub(crate) const ASSISTANT: Self = SearchLimits {
        max_hits_per_chapter: 5,
        max_chapters: 40,
        preview: 120,
    };
}

pub(crate) fn build_regex(
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

pub(crate) fn search(
    store: &Store,
    re: &regex::Regex,
    in_source: bool,
    limits: SearchLimits,
) -> anyhow::Result<Vec<SearchChapter>> {
    let mut output = Vec::new();
    for chapter in store.searchable_chapters(in_source)? {
        let mut hits = Vec::new();
        let mut count = 0usize;
        for (line_index, line) in chapter.text.lines().enumerate() {
            let Some(first_match) = re.find(line) else {
                continue;
            };
            count += re.find_iter(line).count();
            if hits.len() < limits.max_hits_per_chapter {
                hits.push(SearchHit {
                    line: line_index + 1,
                    preview: clip_around(line, first_match.start(), limits.preview),
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
            if output.len() >= limits.max_chapters {
                break;
            }
        }
    }
    Ok(output)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn replace(
    app: &AppHandle,
    state: &AppState,
    project_id: &str,
    find: &str,
    replace: &str,
    match_case: bool,
    whole_word: bool,
    regex_mode: bool,
) -> Result<usize, String> {
    if find.is_empty() {
        return Ok(0);
    }
    let _slot = jobs::lease(app, state, project_id)?;
    let expand = regex_mode;
    let regex = build_regex(find, match_case, whole_word, expand)?;
    let store = super::project_store(state, project_id)?;
    store
        .replace_in_translations(&regex, replace, expand)
        .map_err(err)
}

/// Dry-run: how many translation matches a replace would hit, plus a few samples.
pub(crate) fn replace_impact(
    store: &Store,
    re: &regex::Regex,
) -> anyhow::Result<(usize, usize, Vec<String>)> {
    const SAMPLE_CAP: usize = 5;
    let mut chapters = 0usize;
    let mut matches = 0usize;
    let mut samples = Vec::new();
    for chapter in store.searchable_chapters(false)? {
        let count = re.find_iter(&chapter.text).count();
        if count == 0 {
            continue;
        }
        chapters += 1;
        matches += count;
        if samples.len() < SAMPLE_CAP {
            let line = chapter.text.lines().find(|l| re.is_match(l)).unwrap_or("");
            let at = re.find(line).map(|m| m.start()).unwrap_or(0);
            let preview = clip_around(line, at, 80);
            let num = chapter
                .number
                .map(|n| n.to_string())
                .unwrap_or_else(|| chapter.idx.to_string());
            samples.push(format!("#{num} {preview}"));
        }
    }
    Ok((chapters, matches, samples))
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
