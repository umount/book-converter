//! Compact project snapshot injected into the assistant system prompt.

use anyhow::Result;

use crate::config::Config;
use crate::state::Store;

pub fn system_prompt(snapshot: &str) -> String {
    format!(
        "You are the in-app project assistant for Book Converter, a desktop tool \
         that translates long books via DeepSeek with a glossary and rolling context.\n\
         \n\
         Help the user run translation, inspect progress, find leftover foreign words, \
         fix glossary terms, edit chapter prompts/context, and export — by calling tools.\n\
         \n\
         Rules:\n\
         - Prefer tools over guessing. Never invent chapter text or glossary entries.\n\
         - Keep answers concise. Speak the user's language when they write in it.\n\
         - Mutating tools will ask the user to confirm; if denied, continue without that action.\n\
         - Do not ask for API keys or change settings.\n\
         - Chapter numbers in the UI are book numbers (第N章), not reading-order indices; \
           tools that take `index` want the reading-order idx from list_chapters.\n\
         \n\
         Current project snapshot:\n{snapshot}"
    )
}

pub fn build_snapshot(
    store: &Store,
    config: &Config,
    project_id: &str,
    open_chapter: Option<usize>,
    job_running: bool,
) -> Result<String> {
    let stats = store.stats()?;
    let next = store.next_pending()?;
    let max_n = store.max_chapter_number()?;
    let (ref_count, ref_max) = store.reference_stats().unwrap_or((0, None));
    let (glossary_total, sample) = store
        .glossary_page("", None, 0, 8)
        .unwrap_or((0, Vec::new()));

    let meta = store.project_metadata()?;
    let title = meta
        .title_translated
        .or(meta.title)
        .unwrap_or_else(|| "(untitled)".into());

    let mut lines = vec![
        format!("project_id: {project_id}"),
        format!("title: {title}"),
        format!(
            "languages: {} → {}",
            config.source_lang, config.target_lang
        ),
        format!("model: {}", config.model),
        format!(
            "progress: done={} pending={} failed={} total={} job_running={}",
            stats.done, stats.pending, stats.failed, stats.total, job_running
        ),
        format!("next_pending: {next:?}"),
        format!("max_chapter_number: {max_n:?}"),
        format!("glossary_terms: {glossary_total}"),
        format!("reference_chapters: {ref_count} (max_number={ref_max:?})"),
    ];

    if let Some(idx) = open_chapter {
        if let Some(ch) = store.chapter_full(idx)? {
            let issues = store
                .list_chapters()?
                .into_iter()
                .find(|r| r.idx == idx)
                .and_then(|r| r.lang_issues);
            lines.push(format!(
                "open_chapter: idx={idx} number={:?} status={} origin={:?} title={:?} lang_issues={issues:?}",
                ch.number, ch.status, ch.origin, ch.translated_title.or(Some(ch.source_title)),
            ));
        }
    }

    if !sample.is_empty() {
        lines.push("glossary_sample:".into());
        for t in sample {
            lines.push(format!(
                "  - {} → {} ({}, pinned={})",
                t.source,
                t.target,
                t.kind.label(),
                t.pinned
            ));
        }
    }

    // Chapters with leftover foreign words (capped).
    let flagged: Vec<_> = store
        .list_chapters()?
        .into_iter()
        .filter(|c| c.lang_issues.as_ref().is_some_and(|s| !s.is_empty()))
        .take(12)
        .collect();
    if !flagged.is_empty() {
        lines.push("chapters_with_lang_issues:".into());
        for c in flagged {
            lines.push(format!(
                "  - idx={} number={:?}: {}",
                c.idx,
                c.number,
                c.lang_issues.unwrap_or_default()
            ));
        }
    }

    Ok(lines.join("\n"))
}
