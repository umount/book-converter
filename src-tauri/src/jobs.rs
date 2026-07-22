//! Background translation / retarget jobs (run on a dedicated OS thread).

use std::sync::atomic::{AtomicBool, Ordering};

use tauri::{AppHandle, Emitter};

use crate::config::Config;
use crate::dto::{Progress, RenameChange};
use crate::orchestrator::Orchestrator;
use crate::state::Store;
use crate::translator::DeepSeekClient;

pub(crate) async fn run_job(
    project_id: &str,
    db: &str,
    style: Option<String>,
    limit: Option<usize>,
    only_index: Option<usize>,
    cancel: &AtomicBool,
    app: &AppHandle,
) -> anyhow::Result<()> {
    let config = Config::load();
    let cl = DeepSeekClient::new(config.clone())?;
    let store = Store::open(db)?;
    let mut orch = Orchestrator::new(&cl, &store, &config, style)?;
    let emit = |ev: crate::orchestrator::ProgressEvent| {
        let _ = app.emit(
            "progress",
            Progress {
                project: project_id.to_string(),
                done: ev.stats.done,
                total: ev.stats.total,
                failed: ev.stats.failed,
                pending: ev.stats.pending,
                running: true,
                job_done: ev.job_done,
                job_total: ev.job_total,
                current_idx: ev.current_idx,
                current_title: ev.current_title,
                phase: ev.phase.to_string(),
                last_ms: ev.last_ms,
                eta_secs: ev.eta_secs,
            },
        );
    };
    if let Some(idx) = only_index {
        orch.run_one(idx, cancel, emit).await
    } else {
        orch.run(limit, cancel, emit).await
    }
}

pub(crate) async fn run_retarget(
    project_id: &str,
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

        let new_title = apply_changes(project_id, &cl, lang, changes, &title, app).await;
        let mut out_lines: Vec<String> = Vec::with_capacity(body.lines().count());
        for line in body.lines() {
            out_lines.push(apply_changes(project_id, &cl, lang, changes, line, app).await);
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
                "project": project_id,
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
    project_id: &str,
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
                        serde_json::json!({
                            "project": project_id,
                            "message": format!("{} -> {}: {e}", c.old_target, c.new_target),
                        }),
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
