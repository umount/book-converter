//! Thin dispatch: typed args → shared ops / Store.

use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use tauri::{AppHandle, Manager};

use crate::commands::ops;
use crate::dto::{RenameChange, TermDto};
use crate::export::OutputFormat;
use crate::session::AppState;
use crate::state::Store;

use super::args::{
    BootstrapGlossaryArgs, ChapterTermsArgs, DeleteTermArgs, ExportBookArgs, GetChapterArgs,
    GlossaryPageArgs, HarvestGlossaryArgs, ListChaptersArgs, ReplaceInBookArgs, ResetTranslationArgs,
    RetargetTermsArgs, SearchBookArgs, SetChapterContextArgs, SetChapterPromptArgs,
    StartTranslationArgs, TranslateChapterArgs, UpdateChapterTranslationArgs, UpdateTermArgs,
};
use super::tools::{ToolDef, ToolPolicy};

fn state(app: &AppHandle) -> Result<tauri::State<'_, AppState>> {
    app.try_state::<AppState>()
        .ok_or_else(|| anyhow!("app state missing"))
}

fn parse<T: serde::de::DeserializeOwned>(args: &Value) -> Result<T> {
    serde_json::from_value(args.clone()).map_err(|e| anyhow!("invalid arguments: {e}"))
}

pub(crate) fn clip_text(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let clipped: String = s.chars().take(max).collect();
    format!("{clipped}…")
}

pub(crate) fn confirm_preview(
    app: &AppHandle,
    project_id: &str,
    def: &ToolDef,
    args: &Value,
) -> Result<String> {
    let store = state(app)
        .and_then(|s| ops::project_store(&s, project_id).map_err(|e| anyhow!(e)))
        .ok();
    preview_from_store(store.as_ref(), def, args, |format| {
        let app_state = state(app)?;
        ops::export::planned_path(&app_state, project_id, format).map_err(|e: String| anyhow!(e))
    })
}

fn preview_from_store(
    store: Option<&Store>,
    def: &ToolDef,
    args: &Value,
    export_path: impl FnOnce(OutputFormat) -> Result<String>,
) -> Result<String> {
    match def.name {
        "export_book" => {
            let parsed: ExportBookArgs = parse(args)?;
            let format =
                OutputFormat::from_ext(&parsed.format).ok_or_else(|| anyhow!("bad_format"))?;
            let path = export_path(format)?;
            Ok(format!("export {ext} → {path}", ext = format.ext(),))
        }
        "replace_in_book" => {
            let a: ReplaceInBookArgs = parse(args)?;
            let Some(store) = store else {
                return Ok(serde_json::to_string_pretty(args).unwrap_or_else(|_| args.to_string()));
            };
            Ok(preview_replace(store, &a)?)
        }
        "update_chapter_translation" => {
            let a: UpdateChapterTranslationArgs = parse(args)?;
            let Some(store) = store else {
                return Ok(serde_json::to_string_pretty(args).unwrap_or_else(|_| args.to_string()));
            };
            Ok(preview_update_chapter(store, &a)?)
        }
        _ => Ok(serde_json::to_string_pretty(args).unwrap_or_else(|_| args.to_string())),
    }
}

fn preview_replace(store: &Store, a: &ReplaceInBookArgs) -> Result<String> {
    let re = ops::text::build_regex(&a.find, a.match_case, a.whole_word, a.regex)
        .map_err(|e| anyhow!(e))?;
    let (chapters, matches, samples) = ops::text::replace_impact(store, &re)?;
    let mut flags = Vec::new();
    if a.match_case {
        flags.push("match_case");
    }
    if a.whole_word {
        flags.push("whole_word");
    }
    if a.regex {
        flags.push("regex");
    }
    let flags = if flags.is_empty() {
        String::new()
    } else {
        format!(" ({})", flags.join(", "))
    };
    let mut lines = vec![
        format!("replace «{}» → «{}»{flags}", a.find, a.replace),
        format!("{matches} match(es) in {chapters} chapter(s)"),
    ];
    for sample in samples {
        lines.push(format!("  {sample}"));
    }
    Ok(lines.join("\n"))
}

fn preview_update_chapter(store: &Store, a: &UpdateChapterTranslationArgs) -> Result<String> {
    let Some(ch) = store.chapter_full(a.index)? else {
        return Ok(format!("update chapter idx={} (not found)", a.index));
    };
    let current = ch.translated.as_deref().unwrap_or("");
    Ok(format!(
        "update chapter idx={} number={:?} status={}\n\
         title: {} → {}\n\
         current: {}\n\
         new: {}",
        a.index,
        ch.number,
        ch.status,
        ch.translated_title.as_deref().unwrap_or(&ch.source_title),
        if a.translated_title.is_empty() {
            "(unchanged)"
        } else {
            a.translated_title.as_str()
        },
        clip_text(current, 280),
        clip_text(&a.translated, 280),
    ))
}

pub(crate) fn read(
    store: &Store,
    job_running: bool,
    def: &ToolDef,
    args: &Value,
) -> Result<String> {
    match def.name {
        "get_progress" => {
            let st = store.stats()?;
            let next = store.next_pending()?;
            Ok(json!({
                "done": st.done,
                "pending": st.pending,
                "failed": st.failed,
                "total": st.total,
                "running": job_running,
                "next_pending": next,
            })
            .to_string())
        }
        "list_chapters" => {
            let a: ListChaptersArgs = parse(args)?;
            let rows =
                store.list_chapters_page(a.status.as_deref(), a.only_issues, a.offset, a.limit)?;
            let chapters: Vec<_> = rows
                .into_iter()
                .map(|r| {
                    json!({
                        "idx": r.idx,
                        "number": r.number,
                        "title": r.title,
                        "translated_title": r.translated_title,
                        "status": r.status,
                        "origin": r.origin,
                        "lang_issues": r.lang_issues,
                    })
                })
                .collect();
            Ok(json!({ "chapters": chapters, "returned": chapters.len() }).to_string())
        }
        "get_chapter" => {
            let a: GetChapterArgs = parse(args)?;
            let row = store
                .chapter_full(a.index)?
                .ok_or_else(|| anyhow!("chapter not found"))?;
            Ok(json!({
                "idx": a.index,
                "number": row.number,
                "status": row.status,
                "origin": row.origin,
                "source_title": row.source_title,
                "translated_title": row.translated_title,
                "lang_issues": row.lang_issues,
                "source": clip_text(&row.source, 4000),
                "translated": row.translated.as_deref().map(|t| clip_text(t, 4000)),
                "user_prompt": row.user_prompt,
            })
            .to_string())
        }
        "search_book" => {
            let a: SearchBookArgs = parse(args)?;
            let re = ops::text::build_regex(&a.query, a.match_case, a.whole_word, a.regex)
                .map_err(|e: String| anyhow!(e))?;
            let hits =
                ops::text::search(store, &re, a.in_source, ops::text::SearchLimits::ASSISTANT)?;
            Ok(json!({ "chapters": hits }).to_string())
        }
        "get_glossary_page" => {
            let a: GlossaryPageArgs = parse(args)?;
            let limit = a.limit.clamp(1, 100);
            let (total, terms) =
                store.glossary_page(&a.query, a.kind.as_deref(), a.offset, limit)?;
            let terms: Vec<_> = terms
                .into_iter()
                .map(|t| {
                    json!({
                        "source": t.source,
                        "target": t.target,
                        "kind": t.kind.label(),
                        "frequency": t.frequency,
                        "pinned": t.pinned,
                    })
                })
                .collect();
            Ok(json!({ "total": total, "terms": terms }).to_string())
        }
        "chapter_terms" => {
            let a: ChapterTermsArgs = parse(args)?;
            let Some((_, source)) = store.chapter(a.index)? else {
                return Ok(json!({ "terms": [] }).to_string());
            };
            let glossary = store.load_glossary()?;
            let terms: Vec<_> = crate::glossary::relevant_terms(&glossary, &source)
                .into_iter()
                .map(|t| {
                    json!({
                        "source": t.source,
                        "target": t.target,
                        "kind": t.kind.label(),
                        "pinned": t.pinned,
                    })
                })
                .collect();
            Ok(json!({ "terms": terms }).to_string())
        }
        "get_book_details" => {
            let m = store.project_metadata()?;
            Ok(json!({
                "title": m.title,
                "author": m.author,
                "title_translated": m.title_translated,
                "author_translated": m.author_translated,
                "summary": m.summary.as_ref().map(|s| clip_text(s, 800)),
            })
            .to_string())
        }
        "get_reference_info" => {
            let (count, max) = store.reference_stats()?;
            Ok(json!({ "imported": count, "max_covered": max }).to_string())
        }
        other => Err(anyhow!("not a read tool: {other}")),
    }
}

pub(crate) async fn mutate(
    app: &AppHandle,
    project_id: &str,
    def: &ToolDef,
    args: &Value,
) -> Result<String> {
    let state = state(app)?;
    match def.name {
        "start_translation" => {
            let a: StartTranslationArgs = parse(args)?;
            ops::translation::start(app, &state, project_id, a.limit)
                .map_err(|e: String| anyhow!(e))?;
            Ok(json!({ "started": true, "limit": a.limit }).to_string())
        }
        "pause_translation" => {
            ops::translation::pause(&state, project_id);
            Ok(json!({ "pause_requested": true }).to_string())
        }
        "translate_chapter" => {
            let a: TranslateChapterArgs = parse(args)?;
            ops::translation::translate_chapter(app, &state, project_id, a.index)
                .map_err(|e: String| anyhow!(e))?;
            Ok(json!({ "started": true, "index": a.index }).to_string())
        }
        "reset_translation" => {
            let a: ResetTranslationArgs = parse(args)?;
            let n = ops::translation::reset(app, &state, project_id, a.from_number)
                .map_err(|e: String| anyhow!(e))?;
            Ok(json!({ "reset": n, "from_number": a.from_number }).to_string())
        }
        "update_term" => {
            let a: UpdateTermArgs = parse(args)?;
            let source = a.source.clone();
            ops::glossary::upsert_term(
                &state,
                project_id,
                TermDto {
                    source: a.source,
                    target: a.target,
                    kind: a.kind,
                    frequency: a.frequency,
                    pinned: true,
                },
            )
            .map_err(|e: String| anyhow!(e))?;
            Ok(json!({ "updated": source }).to_string())
        }
        "delete_term" => {
            let a: DeleteTermArgs = parse(args)?;
            ops::glossary::delete_term(&state, project_id, &a.source)
                .map_err(|e: String| anyhow!(e))?;
            Ok(json!({ "deleted": a.source }).to_string())
        }
        "retarget_terms" => {
            let a: RetargetTermsArgs = parse(args)?;
            let changes: Vec<RenameChange> = a
                .changes
                .into_iter()
                .map(|c| RenameChange {
                    old_target: c.old_target,
                    new_target: c.new_target,
                    kind: c.kind.unwrap_or_else(|| "term".into()),
                })
                .collect();
            ops::glossary::retarget(app, &state, project_id, changes)
                .map_err(|e: String| anyhow!(e))?;
            Ok(json!({ "started": true }).to_string())
        }
        "harvest_glossary" => {
            let a: HarvestGlossaryArgs = parse(args)?;
            let n = ops::glossary::harvest(app, &state, project_id, a.sample, a.from_end)
                .await
                .map_err(|e: String| anyhow!(e))?;
            Ok(json!({ "glossary_size": n }).to_string())
        }
        "bootstrap_glossary" => {
            let a: BootstrapGlossaryArgs = parse(args)?;
            let n = ops::glossary::bootstrap(app, &state, project_id, a.sample)
                .await
                .map_err(|e: String| anyhow!(e))?;
            Ok(json!({ "glossary_size": n }).to_string())
        }
        "update_chapter_translation" => {
            let a: UpdateChapterTranslationArgs = parse(args)?;
            let issues = ops::translation::save_manual(
                &state,
                project_id,
                a.index,
                &a.translated_title,
                &a.translated,
            )
            .map_err(|e: String| anyhow!(e))?;
            Ok(json!({ "saved": true, "index": a.index, "lang_issues": issues }).to_string())
        }
        "set_chapter_prompt" => {
            let a: SetChapterPromptArgs = parse(args)?;
            ops::translation::set_prompt(&state, project_id, a.index, &a.prompt)
                .map_err(|e: String| anyhow!(e))?;
            Ok(json!({ "saved": true, "index": a.index }).to_string())
        }
        "set_chapter_context" => {
            let a: SetChapterContextArgs = parse(args)?;
            ops::translation::set_context(&state, project_id, a.index, &a.summary, &a.prev_tail)
                .map_err(|e: String| anyhow!(e))?;
            Ok(json!({ "saved": true, "index": a.index }).to_string())
        }
        "replace_in_book" => {
            let a: ReplaceInBookArgs = parse(args)?;
            let n = ops::text::replace(
                app,
                &state,
                project_id,
                &a.find,
                &a.replace,
                a.match_case,
                a.whole_word,
                a.regex,
            )
            .map_err(|e: String| anyhow!(e))?;
            Ok(json!({ "changed": n }).to_string())
        }
        "use_reference_as_base" => {
            let n = ops::reference::use_as_base(app, &state, project_id)
                .map_err(|e: String| anyhow!(e))?;
            Ok(json!({ "restored": n }).to_string())
        }
        "export_book" => {
            let a: ExportBookArgs = parse(args)?;
            let format = OutputFormat::from_ext(&a.format).ok_or_else(|| anyhow!("bad_format"))?;
            let path = ops::export::planned_path(&state, project_id, format)
                .map_err(|e: String| anyhow!(e))?;
            if let Some(dir) = std::path::Path::new(&path).parent() {
                std::fs::create_dir_all(dir)?;
            }
            let exported =
                ops::export::export(&state, project_id, &path).map_err(|e: String| anyhow!(e))?;
            Ok(json!({ "exported": exported }).to_string())
        }
        other => Err(anyhow!("not a mutating tool: {other}")),
    }
}

pub(crate) async fn execute(
    app: &AppHandle,
    project_id: &str,
    db: &str,
    def: &ToolDef,
    args: &Value,
) -> Result<String> {
    if def.policy == ToolPolicy::Auto {
        let store = Store::open(db)?;
        let running = app
            .try_state::<AppState>()
            .map(|s| s.with(project_id, |sess| sess.running))
            .unwrap_or(false);
        read(&store, running, def, args)
    } else {
        mutate(app, project_id, def, args).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assistant::tools::find;
    use crate::book::Chapter;
    use crate::state::Store;

    fn seed() -> Store {
        let store = Store::open(":memory:").unwrap();
        store
            .init_chapters(&[
                Chapter {
                    index: 1,
                    number: Some(1),
                    title: "第1章".into(),
                    body: "source one 王林".into(),
                },
                Chapter {
                    index: 2,
                    number: Some(2),
                    title: "第2章".into(),
                    body: "source two".into(),
                },
                Chapter {
                    index: 3,
                    number: Some(3),
                    title: "第3章".into(),
                    body: "source three".into(),
                },
            ])
            .unwrap();
        store
            .save_translation(1, "Глава 1", "перевод один")
            .unwrap();
        store
    }

    #[test]
    fn read_progress_and_list() {
        let store = seed();
        let def = find("get_progress").unwrap();
        let out = read(&store, false, def, &json!({})).unwrap();
        assert!(out.contains("\"done\":1"));
        let list = find("list_chapters").unwrap();
        let page = read(
            &store,
            false,
            list,
            &json!({ "status": "done", "limit": 10 }),
        )
        .unwrap();
        assert!(page.contains("\"returned\":1"));
    }

    #[test]
    fn get_chapter_truncates_and_keeps_lang_issues() {
        let store = seed();
        let def = find("get_chapter").unwrap();
        let out = read(&store, false, def, &json!({ "index": 1 })).unwrap();
        assert!(out.contains("перевод один"));
        assert!(out.contains("\"idx\":1"));
    }

    #[test]
    fn clip_is_char_based() {
        assert_eq!(clip_text("привет", 3), "при…");
        assert_eq!(clip_text("光阴之外", 2), "光阴…");
    }

    #[test]
    fn replace_preview_counts_matches() {
        let store = seed();
        let def = find("replace_in_book").unwrap();
        let out = preview_from_store(
            Some(&store),
            def,
            &json!({ "find": "один", "replace": "раз" }),
            |_| Ok("unused".into()),
        )
        .unwrap();
        assert!(out.contains("replace «один» → «раз»"));
        assert!(out.contains("1 match(es) in 1 chapter(s)"));
        assert!(out.contains("#1"));
    }

    #[test]
    fn update_chapter_preview_shows_current_and_new() {
        let store = seed();
        let def = find("update_chapter_translation").unwrap();
        let out = preview_from_store(
            Some(&store),
            def,
            &json!({ "index": 1, "translated_title": "Глава 1", "translated": "новый текст" }),
            |_| Ok("unused".into()),
        )
        .unwrap();
        assert!(out.contains("idx=1"));
        assert!(out.contains("перевод один"));
        assert!(out.contains("новый текст"));
    }
}
