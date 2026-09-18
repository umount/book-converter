//! Dispatch assistant tool calls onto Store / jobs.

use anyhow::{anyhow, Result};
use serde::Deserialize;
use serde_json::json;
use tauri::{AppHandle, Manager};

use crate::config::Config;
use crate::dto::RenameChange;
use crate::glossary::{Term, TermKind};
use crate::jobs::{run_retarget, run_translation, spawn_project_job};
use crate::session::AppState;
use crate::state::Store;
use crate::textutil;

pub struct ToolExecutor {
    pub app: AppHandle,
    pub project_id: String,
    pub db: String,
}

impl ToolExecutor {
    fn store(&self) -> Result<Store> {
        Store::open(&self.db)
    }

    fn state(&self) -> Result<tauri::State<'_, AppState>> {
        self.app
            .try_state::<AppState>()
            .ok_or_else(|| anyhow!("app state missing"))
    }

    pub async fn execute(&self, name: &str, args_json: &str) -> Result<String> {
        let args: serde_json::Value = if args_json.trim().is_empty() {
            json!({})
        } else {
            serde_json::from_str(args_json)
                .map_err(|e| anyhow!("invalid tool arguments JSON: {e}"))?
        };

        match name {
            "get_progress" => self.get_progress(),
            "list_chapters" => self.list_chapters(&args),
            "get_chapter" => self.get_chapter(&args),
            "search_book" => self.search_book(&args),
            "get_glossary_page" => self.get_glossary_page(&args),
            "chapter_terms" => self.chapter_terms(&args),
            "get_book_details" => self.get_book_details(),
            "get_reference_info" => self.get_reference_info(),
            "start_translation" => self.start_translation(&args),
            "pause_translation" => self.pause_translation(),
            "translate_chapter" => self.translate_chapter(&args),
            "reset_translation" => self.reset_translation(&args),
            "update_term" => self.update_term(&args),
            "delete_term" => self.delete_term(&args),
            "retarget_terms" => self.retarget_terms(&args),
            "harvest_glossary" => self.harvest_glossary(&args).await,
            "bootstrap_glossary" => self.bootstrap_glossary(&args).await,
            "update_chapter_translation" => self.update_chapter_translation(&args),
            "set_chapter_prompt" => self.set_chapter_prompt(&args),
            "set_chapter_context" => self.set_chapter_context(&args),
            "replace_in_book" => self.replace_in_book(&args),
            "use_reference_as_base" => self.use_reference_as_base(),
            "export_book" => self.export_book(&args),
            other => Err(anyhow!("unknown tool '{other}'")),
        }
    }

    fn get_progress(&self) -> Result<String> {
        let store = self.store()?;
        let st = store.stats()?;
        let running = self.state()?.with(&self.project_id, |s| s.running);
        let next = store.next_pending()?;
        Ok(json!({
            "done": st.done,
            "pending": st.pending,
            "failed": st.failed,
            "total": st.total,
            "running": running,
            "next_pending": next,
        })
        .to_string())
    }

    fn list_chapters(&self, args: &serde_json::Value) -> Result<String> {
        let status = args.get("status").and_then(|v| v.as_str());
        let only_issues = args
            .get("only_issues")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let limit = args
            .get("limit")
            .and_then(|v| v.as_u64())
            .unwrap_or(40)
            .min(200) as usize;
        let offset = args
            .get("offset")
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as usize;

        let rows: Vec<_> = self
            .store()?
            .list_chapters()?
            .into_iter()
            .filter(|r| status.map(|s| r.status == s).unwrap_or(true))
            .filter(|r| {
                !only_issues
                    || r.lang_issues
                        .as_ref()
                        .is_some_and(|s| !s.trim().is_empty())
            })
            .skip(offset)
            .take(limit)
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
        Ok(json!({ "chapters": rows, "returned": rows.len() }).to_string())
    }

    fn get_chapter(&self, args: &serde_json::Value) -> Result<String> {
        let index = req_usize(args, "index")?;
        let store = self.store()?;
        let row = store
            .chapter_full(index)?
            .ok_or_else(|| anyhow!("chapter not found"))?;
        let issues = store
            .list_chapters()?
            .into_iter()
            .find(|r| r.idx == index)
            .and_then(|r| r.lang_issues);
        Ok(json!({
            "idx": index,
            "number": row.number,
            "status": row.status,
            "origin": row.origin,
            "source_title": row.source_title,
            "translated_title": row.translated_title,
            "lang_issues": issues,
            "source": clip_text(&row.source, 4000),
            "translated": row.translated.as_deref().map(|t| clip_text(t, 4000)),
            "user_prompt": row.user_prompt,
        })
        .to_string())
    }

    fn search_book(&self, args: &serde_json::Value) -> Result<String> {
        let query = req_str(args, "query")?;
        let in_source = args
            .get("in_source")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let match_case = args
            .get("match_case")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let whole_word = args
            .get("whole_word")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let mut pattern = regex::escape(query);
        if whole_word {
            pattern = format!(r"\b{pattern}\b");
        }
        let re = regex::RegexBuilder::new(&pattern)
            .case_insensitive(!match_case)
            .build()?;

        let store = self.store()?;
        let mut hits = Vec::new();
        for chapter in store.searchable_chapters(in_source)? {
            let mut chapter_hits = Vec::new();
            let mut count = 0usize;
            for (i, line) in chapter.text.lines().enumerate() {
                if re.is_match(line) {
                    count += 1;
                    if chapter_hits.len() < 5 {
                        chapter_hits.push(json!({
                            "line": i + 1,
                            "preview": clip_text(line.trim(), 120),
                        }));
                    }
                }
            }
            if count > 0 {
                hits.push(json!({
                    "idx": chapter.idx,
                    "number": chapter.number,
                    "title": chapter.title,
                    "count": count,
                    "hits": chapter_hits,
                }));
            }
            if hits.len() >= 40 {
                break;
            }
        }
        Ok(json!({ "chapters": hits }).to_string())
    }

    fn get_glossary_page(&self, args: &serde_json::Value) -> Result<String> {
        let query = args.get("query").and_then(|v| v.as_str()).unwrap_or("");
        let kind = args.get("kind").and_then(|v| v.as_str());
        let offset = args.get("offset").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
        let limit = args
            .get("limit")
            .and_then(|v| v.as_u64())
            .unwrap_or(30)
            .clamp(1, 100) as usize;
        let (total, terms) = self.store()?.glossary_page(query, kind, offset, limit)?;
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

    fn chapter_terms(&self, args: &serde_json::Value) -> Result<String> {
        let index = req_usize(args, "index")?;
        let store = self.store()?;
        let Some((_, source)) = store.chapter(index)? else {
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

    fn get_book_details(&self) -> Result<String> {
        let m = self.store()?.project_metadata()?;
        Ok(json!({
            "title": m.title,
            "author": m.author,
            "title_translated": m.title_translated,
            "author_translated": m.author_translated,
            "summary": m.summary.as_ref().map(|s| clip_text(s, 800)),
        })
        .to_string())
    }

    fn get_reference_info(&self) -> Result<String> {
        let (count, max) = self.store()?.reference_stats()?;
        Ok(json!({ "imported": count, "max_covered": max }).to_string())
    }

    fn start_translation(&self, args: &serde_json::Value) -> Result<String> {
        let limit = args.get("limit").and_then(|v| v.as_u64()).map(|n| n as usize);
        let state = self.state()?;
        let (db, cancel) = state
            .begin_job(&self.project_id, "job_running")
            .map_err(|e| anyhow!(e))?;
        let project_id = self.project_id.clone();
        spawn_project_job(
            self.app.clone(),
            project_id.clone(),
            cancel,
            move |app, project_id, cancel| async move {
                run_translation(&project_id, &db, limit, None, &cancel, &app).await
            },
            |app, project_id, ()| {
                let _ = app.emit(
                    "done",
                    serde_json::json!({ "project": project_id }),
                );
            },
        );
        Ok(json!({ "started": true, "limit": limit }).to_string())
    }

    fn pause_translation(&self) -> Result<String> {
        self.state()?.request_cancel(&self.project_id);
        Ok(json!({ "pause_requested": true }).to_string())
    }

    fn translate_chapter(&self, args: &serde_json::Value) -> Result<String> {
        let index = req_usize(args, "index")?;
        let state = self.state()?;
        let (db, cancel) = state
            .begin_job(&self.project_id, "job_running")
            .map_err(|e| anyhow!(e))?;
        let project_id = self.project_id.clone();
        spawn_project_job(
            self.app.clone(),
            project_id.clone(),
            cancel,
            move |app, project_id, cancel| async move {
                run_translation(&project_id, &db, None, Some(index), &cancel, &app).await
            },
            |app, project_id, ()| {
                let _ = app.emit(
                    "done",
                    serde_json::json!({ "project": project_id }),
                );
            },
        );
        Ok(json!({ "started": true, "index": index }).to_string())
    }

    fn reset_translation(&self, args: &serde_json::Value) -> Result<String> {
        let state = self.state()?;
        let running = state.with(&self.project_id, |s| s.running);
        if running {
            return Err(anyhow!("job_running"));
        }
        let from_number = args
            .get("from_number")
            .and_then(|v| v.as_u64())
            .map(|n| n as usize);
        let store = self.store()?;
        let n = store.reset_from_number(from_number)?;
        if from_number.is_none() {
            let _ = store.set_meta("running_summary", "");
        }
        Ok(json!({ "reset": n, "from_number": from_number }).to_string())
    }

    fn update_term(&self, args: &serde_json::Value) -> Result<String> {
        let source = req_str(args, "source")?;
        let target = req_str(args, "target")?;
        let kind = args
            .get("kind")
            .and_then(|v| v.as_str())
            .unwrap_or("term");
        let frequency = args
            .get("frequency")
            .and_then(|v| v.as_u64())
            .unwrap_or(1) as u32;
        self.store()?.upsert_term(&Term {
            source: source.to_string(),
            target: target.to_string(),
            kind: TermKind::from_label(kind),
            frequency: frequency.max(1),
            pinned: true,
        })?;
        Ok(json!({ "updated": source }).to_string())
    }

    fn delete_term(&self, args: &serde_json::Value) -> Result<String> {
        let source = req_str(args, "source")?;
        self.store()?.delete_term(source)?;
        Ok(json!({ "deleted": source }).to_string())
    }

    fn retarget_terms(&self, args: &serde_json::Value) -> Result<String> {
        #[derive(Deserialize)]
        struct Change {
            old_target: String,
            new_target: String,
            #[serde(default)]
            kind: Option<String>,
        }
        #[derive(Deserialize)]
        struct Body {
            changes: Vec<Change>,
        }
        let body: Body = serde_json::from_value(args.clone())?;
        let changes: Vec<RenameChange> = body
            .changes
            .into_iter()
            .filter(|c| {
                !c.old_target.trim().is_empty() && c.new_target.trim() != c.old_target.trim()
            })
            .map(|c| RenameChange {
                old_target: c.old_target,
                new_target: c.new_target,
                kind: c.kind.unwrap_or_else(|| "term".into()),
            })
            .collect();
        if changes.is_empty() {
            return Err(anyhow!("nothing_to_update"));
        }
        let state = self.state()?;
        let (db, cancel) = state
            .begin_job(&self.project_id, "job_running")
            .map_err(|e| anyhow!(e))?;
        let project_id = self.project_id.clone();
        spawn_project_job(
            self.app.clone(),
            project_id.clone(),
            cancel,
            move |app, project_id, cancel| async move {
                run_retarget(&project_id, &db, &changes, &cancel, &app).await
            },
            |app, project_id, changed| {
                let _ = app.emit(
                    "retarget_done",
                    serde_json::json!({ "project": project_id, "changed": changed }),
                );
            },
        );
        Ok(json!({ "started": true }).to_string())
    }

    async fn harvest_glossary(&self, args: &serde_json::Value) -> Result<String> {
        let sample = args
            .get("sample")
            .and_then(|v| v.as_u64())
            .unwrap_or(20)
            .max(1) as usize;
        let from_end = args
            .get("from_end")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let running = self.state()?.with(&self.project_id, |s| s.running);
        if running {
            return Err(anyhow!("job_running"));
        }
        let store = self.store()?;
        let pairs = store.done_chapter_pairs()?;
        if pairs.is_empty() {
            return Err(anyhow!("nothing_translated"));
        }
        let mut chosen: Vec<_> = if from_end {
            pairs.into_iter().rev().take(sample).collect()
        } else {
            pairs.into_iter().take(sample).collect()
        };
        chosen.sort_by_key(|(idx, ..)| *idx);

        let config = Config::load();
        let client = crate::translator::DeepSeekClient::new(config.clone())?;
        let mut glossary = store.load_glossary()?;
        for (_idx, source, translated) in &chosen {
            if let Ok(terms) =
                crate::translator::extract_terms(&client, &config, source, translated, 2).await
            {
                crate::glossary::merge(&mut glossary, terms);
            }
        }
        store.save_glossary(&glossary)?;
        Ok(json!({ "glossary_size": glossary.len(), "chapters": chosen.len() }).to_string())
    }

    async fn bootstrap_glossary(&self, args: &serde_json::Value) -> Result<String> {
        let sample = args
            .get("sample")
            .and_then(|v| v.as_u64())
            .unwrap_or(20)
            .max(1) as usize;
        let store = self.store()?;
        let pairs = store.reference_pairs(sample)?;
        if pairs.is_empty() {
            return Err(anyhow!("no_reference"));
        }
        let config = Config::load();
        let client = crate::translator::DeepSeekClient::new(config.clone())?;
        let mut glossary = store.load_glossary()?;
        let extracted = crate::reference::bootstrap_glossary(&client, &config, &pairs).await?;
        crate::glossary::merge(&mut glossary, extracted);
        store.save_glossary(&glossary)?;
        Ok(json!({ "glossary_size": glossary.len() }).to_string())
    }

    fn update_chapter_translation(&self, args: &serde_json::Value) -> Result<String> {
        let index = req_usize(args, "index")?;
        let title = args
            .get("translated_title")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim();
        let body = req_str(args, "translated")?.trim();
        let store = self.store()?;
        if let Some(ch) = store.chapter_full(index)? {
            if ch.status == "in_progress" {
                return Err(anyhow!("chapter_busy"));
            }
        }
        if body.is_empty() && store.has_translation(index)? {
            return Err(anyhow!("refuse_empty_overwrite"));
        }
        let source = store
            .chapter(index)?
            .map(|(_, s)| s)
            .unwrap_or_default();
        let issues =
            textutil::leftover_foreign(&Config::load().target_lang, title, body, &source);
        store.save_manual_translation(index, title, body, &issues)?;
        Ok(json!({
            "saved": true,
            "index": index,
            "lang_issues": (!issues.is_empty()).then(|| issues.join(", ")),
        })
        .to_string())
    }

    fn set_chapter_prompt(&self, args: &serde_json::Value) -> Result<String> {
        let index = req_usize(args, "index")?;
        let prompt = req_str(args, "prompt")?;
        self.store()?.set_chapter_user_prompt(index, prompt)?;
        Ok(json!({ "saved": true, "index": index }).to_string())
    }

    fn set_chapter_context(&self, args: &serde_json::Value) -> Result<String> {
        let index = req_usize(args, "index")?;
        let summary = req_str(args, "summary")?;
        let prev_tail = req_str(args, "prev_tail")?;
        self.store()?
            .set_context_before(index, summary, prev_tail)?;
        Ok(json!({ "saved": true, "index": index }).to_string())
    }

    fn replace_in_book(&self, args: &serde_json::Value) -> Result<String> {
        let find = req_str(args, "find")?;
        let replace = args
            .get("replace")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let match_case = args
            .get("match_case")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let whole_word = args
            .get("whole_word")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let regex_mode = args.get("regex").and_then(|v| v.as_bool()).unwrap_or(false);
        if find.is_empty() {
            return Ok(json!({ "changed": 0 }).to_string());
        }
        let mut pattern = if regex_mode {
            find.to_string()
        } else {
            regex::escape(find)
        };
        if whole_word {
            pattern = format!(r"\b{pattern}\b");
        }
        let re = regex::RegexBuilder::new(&pattern)
            .case_insensitive(!match_case)
            .build()?;
        let n = self
            .store()?
            .replace_in_translations(&re, replace, regex_mode)?;
        Ok(json!({ "changed": n }).to_string())
    }

    fn use_reference_as_base(&self) -> Result<String> {
        let n = self.store()?.restore_reference_chapters()?;
        Ok(json!({ "restored": n }).to_string())
    }

    fn export_book(&self, args: &serde_json::Value) -> Result<String> {
        let out_path = req_str(args, "out_path")?;
        // Full export needs the same OutputTarget helpers as the Export menu.
        // Ask the user to confirm a concrete path; write via a subprocess-style
        // call is deferred — for now surface a clear instruction.
        Err(anyhow!(
            "export_book: open File → Export in the app UI (requested path: {out_path})"
        ))
    }
}

fn req_str<'a>(args: &'a serde_json::Value, key: &str) -> Result<&'a str> {
    args.get(key)
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("missing string argument '{key}'"))
}

fn req_usize(args: &serde_json::Value, key: &str) -> Result<usize> {
    args.get(key)
        .and_then(|v| v.as_u64())
        .map(|n| n as usize)
        .ok_or_else(|| anyhow!("missing integer argument '{key}'"))
}

fn clip_text(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let clipped: String = s.chars().take(max).collect();
    format!("{clipped}…")
}

use tauri::Emitter;
