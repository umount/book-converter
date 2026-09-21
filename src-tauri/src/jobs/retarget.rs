//! Background glossary-retarget job.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};

use tauri::{AppHandle, Emitter};

use crate::config::Config;
use crate::dto::RenameChange;
use crate::state::Store;
use crate::translator::DeepSeekClient;

pub(crate) async fn run(
    project_id: &str,
    db: &str,
    changes: &[RenameChange],
    cancel: &AtomicBool,
    app: &AppHandle,
) -> anyhow::Result<usize> {
    use crate::retarget::paragraph_mentions;

    let config = Config::load();
    let client = DeepSeekClient::new(config.clone())?;
    let store = Store::open(db)?;
    let lang = &config.target_lang;

    let mentions_any = |text: &str| {
        changes
            .iter()
            .any(|change| paragraph_mentions(text, &change.old_target))
    };

    let chapters = store.translated_chapters()?;
    let jobs: Vec<(usize, String, String)> = chapters
        .into_iter()
        .filter(|(_, title, body)| mentions_any(title) || body.lines().any(mentions_any))
        .collect();
    let contexts = store.chapter_contexts()?;
    let context_jobs: Vec<(usize, String, String)> = contexts
        .into_iter()
        .filter(|(_, summary, prev_tail)| mentions_any(summary) || mentions_any(prev_tail))
        .collect();

    let meta_summary = store
        .get_meta("running_summary")?
        .filter(|summary| mentions_any(summary));
    let meta_boot_tail = store
        .get_meta("boot_prev_tail")?
        .filter(|tail| mentions_any(tail));

    let summary_by_idx: HashMap<usize, String> = store
        .chapter_contexts()?
        .into_iter()
        .map(|(idx, summary, _)| (idx, summary))
        .collect();

    let total = jobs.len()
        + context_jobs.len()
        + usize::from(meta_summary.is_some())
        + usize::from(meta_boot_tail.is_some());
    let mut done_units = 0usize;
    let mut changed = 0usize;

    for (idx, title, body) in jobs {
        if cancel.load(Ordering::Relaxed) {
            break;
        }

        let new_title = apply_changes(project_id, &client, lang, changes, &title, app).await;
        let mut out_lines = Vec::with_capacity(body.lines().count());
        for line in body.lines() {
            out_lines.push(apply_changes(project_id, &client, lang, changes, line, app).await);
        }
        let new_body = out_lines.join("\n");

        let did_change = new_title != title || new_body != body;
        if did_change {
            store.save_translation(idx, &new_title, &new_body)?;
            if let Some(summary) = summary_by_idx.get(&idx) {
                store.save_chapter_context(
                    idx,
                    summary,
                    &crate::textutil::closing_excerpt(&new_body, 400),
                )?;
            }
            changed += 1;
        }
        done_units += 1;
        emit_progress(app, project_id, done_units, total, &new_title, did_change);
    }

    // Re-read after body rewrites: their stored tails may already be refreshed.
    for (idx, summary, prev_tail) in store.chapter_contexts()? {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        if !mentions_any(&summary) && !mentions_any(&prev_tail) {
            continue;
        }

        let new_summary = if mentions_any(&summary) {
            apply_changes(project_id, &client, lang, changes, &summary, app).await
        } else {
            summary.clone()
        };
        let new_tail = if mentions_any(&prev_tail) {
            apply_changes(project_id, &client, lang, changes, &prev_tail, app).await
        } else {
            prev_tail.clone()
        };

        let did_change = new_summary != summary || new_tail != prev_tail;
        if did_change {
            store.save_chapter_context(idx, &new_summary, &new_tail)?;
            changed += 1;
        }
        done_units += 1;
        emit_progress(
            app,
            project_id,
            done_units,
            total,
            &format!("context #{idx}"),
            did_change,
        );
    }

    if let Some(summary) = meta_summary {
        if !cancel.load(Ordering::Relaxed) {
            let new_summary =
                apply_changes(project_id, &client, lang, changes, &summary, app).await;
            let did_change = new_summary != summary;
            if did_change {
                store.set_meta("running_summary", &new_summary)?;
                changed += 1;
            }
            done_units += 1;
            emit_progress(
                app,
                project_id,
                done_units,
                total,
                "running_summary",
                did_change,
            );
        }
    }

    if let Some(tail) = meta_boot_tail {
        if !cancel.load(Ordering::Relaxed) {
            let new_tail = apply_changes(project_id, &client, lang, changes, &tail, app).await;
            let did_change = new_tail != tail;
            if did_change {
                store.set_meta("boot_prev_tail", &new_tail)?;
                changed += 1;
            }
            done_units += 1;
            emit_progress(
                app,
                project_id,
                done_units,
                total,
                "boot_prev_tail",
                did_change,
            );
        }
    }

    Ok(changed)
}

fn emit_progress(
    app: &AppHandle,
    project_id: &str,
    done: usize,
    total: usize,
    title: &str,
    changed: bool,
) {
    let _ = app.emit(
        "retarget_progress",
        serde_json::json!({
            "project": project_id,
            "done": done,
            "total": total,
            "title": title,
            "changed": changed,
        }),
    );
}

async fn apply_changes(
    project_id: &str,
    client: &DeepSeekClient,
    lang: &str,
    changes: &[RenameChange],
    text: &str,
    app: &AppHandle,
) -> String {
    use crate::retarget::paragraph_mentions;

    let mut current = text.to_string();
    for change in changes {
        if paragraph_mentions(&current, &change.old_target) {
            match rewrite_paragraph(
                client,
                lang,
                &change.kind,
                &change.old_target,
                &change.new_target,
                &current,
            )
            .await
            {
                Ok(Some(rewritten)) => current = rewritten,
                Ok(None) => {}
                Err(error) => {
                    tracing::warn!("retarget rewrite failed: {error:#}");
                    let _ = app.emit(
                        "retarget_warn",
                        serde_json::json!({
                            "project": project_id,
                            "message": format!(
                                "{} -> {}: {error}",
                                change.old_target, change.new_target
                            ),
                        }),
                    );
                }
            }
        }
    }
    current
}

async fn rewrite_paragraph(
    client: &DeepSeekClient,
    lang: &str,
    kind: &str,
    old_target: &str,
    new_target: &str,
    text: &str,
) -> anyhow::Result<Option<String>> {
    let (system, user) = crate::retarget::rewrite_prompt(lang, kind, old_target, new_target, text);
    let output = client.translate(&system, &user).await?;
    let output = output.trim().trim_matches('"').trim().to_string();
    Ok((!output.is_empty()).then_some(output))
}
