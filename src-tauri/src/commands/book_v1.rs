//! Structural book and durable job IPC. Domain work lives in application services.
use crate::app::{
    contracts::{AppError, JobRef, ProjectId},
    requests::*,
    services::AppContext,
};
use crate::storage::{repository::storage_error, runs};
use tauri::{Emitter, State};

#[tauri::command]
pub async fn book_delete_chapter(context: State<'_, AppContext>, args: DeleteChapterArgs) -> Result<(), AppError> {
    let manager = context.manager.clone();
    tauri::async_runtime::spawn_blocking(move || manager.lease(&args.project_id)?.with_connection(|db, _| {
        crate::application::book_delete::delete(db, &args)
    })).await.map_err(|_| AppError::invalid("task"))?
}

#[tauri::command]
pub async fn book_get_chapter(
    context: State<'_, AppContext>,
    args: GetChapterArgs,
) -> Result<BookChapterView, AppError> {
    let manager = context.manager.clone();
    tauri::async_runtime::spawn_blocking(move || {
        manager.lease(&args.project_id)?.with_connection(|db, _| {
            crate::storage::repository::ProjectRepository::new(
                db,
                crate::app::contracts::ProjectKind::Book,
            )?
            .chapter(&args.chapter_id.0)
        })
    })
    .await
    .map_err(|_| AppError::invalid("task"))?
}
#[tauri::command]
pub async fn book_list_chapters(
    context: State<'_, AppContext>,
    args: ListChaptersArgs,
) -> Result<ChapterPage, AppError> {
    if args.limit == 0 || args.limit > 500 {
        return Err(AppError::invalid("limit"));
    }
    let manager = context.manager.clone();
    tauri::async_runtime::spawn_blocking(move||manager.lease(&args.project_id)?.with_connection(|db,_|{
        crate::storage::repository::ProjectRepository::new(db,crate::app::contracts::ProjectKind::Book)?;
        let after=if let Some(cursor)=args.cursor{db.query_row("SELECT position FROM book_chapters WHERE id=?1",[cursor],|r|r.get::<_,i64>(0)).map_err(storage_error)?}else{-1};
        let mut query=db.prepare("SELECT id,position,source_title,revision,status,origin,needs_review,(SELECT NULLIF(trim(translated_title),'') FROM book_translations WHERE chapter_id=book_chapter_states.id AND target_language=(SELECT target_language FROM project_settings WHERE singleton=1) ORDER BY revision DESC LIMIT 1) FROM book_chapter_states WHERE position>?1 ORDER BY position LIMIT ?2").map_err(storage_error)?;
        let rows=query.query_map(rusqlite::params![after,args.limit+1],|r|Ok(ChapterSummary{translated_title:r.get(7)?,status:r.get(4)?,origin:r.get(5)?,needs_review:r.get(6)?,id:crate::app::contracts::ChapterId(r.get(0)?),position:r.get(1)?,title:r.get(2)?,revision:crate::app::contracts::Revision(r.get::<_,i64>(3)?.to_string())})).map_err(storage_error)?;
        let mut items=rows.collect::<Result<Vec<_>,_>>().map_err(storage_error)?;
        let next_cursor=if items.len()>args.limit as usize{items.pop();items.last().map(|c|c.id.0.clone())}else{None};Ok(ChapterPage{items,next_cursor})
    })).await.map_err(|_|AppError::invalid("task"))?
}

fn dispatch(
    context: &AppContext,
    app: tauri::AppHandle,
    project: ProjectId,
    job: String,
) -> Result<(), AppError> {
    let kind = context.manager.lease(&project)?.with_connection(|db, _| Ok(runs::get_run(db, &job)?.kind))?;
    if matches!(kind.as_str(), "manga_recognition" | "manga_translation" | "manga_masks" | "manga_inpainting" | "manga_lettering" | "manga_automatic" | "manga_rebuild") { return super::manga_v1::dispatch(context, app, project, job); }
    let pipeline = crate::application::runtime::resume_provider(&context.manager, &project, &job)?;
    let cancel = context.book_jobs.reserve(&project, &job)?;
    let manager = context.manager.clone();
    let runtime = context.book_jobs.clone();
    tauri::async_runtime::spawn(async move {
        let _reservation = crate::application::runtime::Reservation {
            runtime,
            project: project.clone(),
            job: job.clone(),
        };
        let result =
            crate::jobs::durable::execute(&manager, &project, &job, &pipeline, cancel, |event| {
                let _ = app.emit("project-event", event);
            })
            .await;
        if let Err(error) = result {
            tracing::warn!(project = project.as_str(), job, code = ?error.code, "Book job stopped");
        }
    });
    Ok(())
}
#[tauri::command]
pub async fn book_start_translation(
    context: State<'_, AppContext>,
    app: tauri::AppHandle,
    args: StartBookTranslationArgs,
) -> Result<JobRef, AppError> {
    let manager = context.manager.clone();
    let project = args.project_id.clone();
    let job = tauri::async_runtime::spawn_blocking(move || {
        crate::application::runtime::prepare_book_run(
            &manager,
            &project,
            &args.selection,
            &args.options,
        )
    })
    .await
    .map_err(|_| AppError::invalid("task"))??;
    dispatch_created(&context, app, job)
}
pub(super) fn dispatch_created(
    context: &AppContext,
    app: tauri::AppHandle,
    job: JobRef,
) -> Result<JobRef, AppError> {
    if let Err(error) = dispatch(context, app, job.project_id.clone(), job.job_id.clone()) {
        context
            .manager
            .lease(&job.project_id)?
            .with_connection(|db, _| {
                let run = runs::get_run(db, &job.job_id)?;
                runs::transition(
                    db,
                    &run.id,
                    &run.revision,
                    crate::app::contracts::JobState::Failed,
                    Some(&error),
                    "dispatch-failed",
                )?;
                Ok(())
            })?;
        return Err(error);
    }
    Ok(job)
}
#[tauri::command]
pub async fn job_resume(
    context: State<'_, AppContext>,
    app: tauri::AppHandle,
    args: JobArgs,
) -> Result<JobRef, AppError> {
    let kind=context.manager.lease(&args.project_id)?.with_connection(|db,_|Ok(runs::get_run(db,&args.job_id.0)?.kind))?;
    if matches!(kind.as_str(),"manga_masks"|"manga_inpainting"|"manga_lettering" | "manga_automatic" | "manga_rebuild") {
        context.models.list().await.map_err(|_|AppError::invalid("mangaModelMissing"))?;
    }
    dispatch(
        &context,
        app,
        args.project_id.clone(),
        args.job_id.0.clone(),
    )?;
    Ok(JobRef {
        project_id: args.project_id,
        job_id: args.job_id.0,
    })
}
#[tauri::command]
pub async fn job_cancel(context: State<'_, AppContext>, args: JobArgs) -> Result<(), AppError> {
    if context.book_jobs.cancel(&args.project_id, &args.job_id.0) {
        return Ok(());
    }
    context
        .manager
        .lease(&args.project_id)?
        .with_connection(|db, _| {
            let run = runs::get_run(db, &args.job_id.0)?;
            if run.state == crate::app::contracts::JobState::Queued {
                runs::transition(
                    db,
                    &run.id,
                    &run.revision,
                    crate::app::contracts::JobState::Cancelled,
                    None,
                    "cancelled",
                )?;
            }
            Ok(())
        })
}

#[tauri::command]
pub async fn book_update_translation_block(
    context: State<'_, AppContext>,
    args: UpdateTranslationBlockArgs,
) -> Result<crate::app::contracts::Revision, AppError> {
    let manager = context.manager.clone();
    tauri::async_runtime::spawn_blocking(move || {
        manager.lease(&args.project_id)?.with_connection(|db, _| {
            crate::storage::results::edit_translation_block(
                db,
                &args.translation_id,
                &args.block_id.0,
                &args.expected_revision,
                &args.text,
            )
        })
    })
    .await
    .map_err(|_| AppError::invalid("task"))?
}
fn job_view(db: &rusqlite::Connection, project: &ProjectId, id: &str) -> Result<JobView, AppError> {
    let run = runs::get_run(db, id)?;
    let completed=db.query_row("SELECT COUNT(*) FROM job_steps AS step WHERE run_id=?1 AND state='succeeded' AND NOT EXISTS(SELECT 1 FROM job_steps AS newer WHERE newer.run_id=step.run_id AND newer.entity_kind=step.entity_kind AND newer.entity_id=step.entity_id AND newer.stage=step.stage AND newer.attempt>step.attempt)",[id],|r|r.get::<_,u32>(0)).map_err(storage_error)?;
    let remaining_seconds = runs::remaining_seconds(db, &run)?;
    let is_book = run.kind.starts_with("book_");
    let (completed_chapters, current) = if is_book {
        use rusqlite::OptionalExtension;
        let completed_chapters = db.query_row("SELECT COUNT(*) FROM (SELECT entity_id FROM job_steps s WHERE run_id=?1 AND entity_kind='chapter' AND state='succeeded' AND NOT EXISTS(SELECT 1 FROM job_steps n WHERE n.run_id=s.run_id AND n.entity_kind=s.entity_kind AND n.entity_id=s.entity_id AND n.stage=s.stage AND n.attempt>s.attempt) GROUP BY entity_id HAVING COUNT(*)=?2)", rusqlite::params![id, run.snapshot.stages.len()], |r| r.get::<_,u32>(0)).map_err(storage_error)?;
        let current = db.query_row("SELECT COALESCE(c.display_number,c.position+1),c.source_title,s.stage FROM job_steps s JOIN book_chapters c ON c.id=s.entity_id WHERE s.run_id=?1 AND s.entity_kind='chapter' ORDER BY s.rowid DESC LIMIT 1", [id], |r| Ok((r.get::<_,u32>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?))).optional().map_err(storage_error)?;
        (Some(completed_chapters), current)
    } else { (None, None) };

    let is_manga=run.kind.starts_with("manga_");
    let (completed_pages,page)=if is_manga {
        use rusqlite::OptionalExtension;
        let count=db.query_row("SELECT COUNT(*) FROM (SELECT entity_id FROM job_steps s WHERE run_id=?1 AND entity_kind='page' AND state='succeeded' AND NOT EXISTS(SELECT 1 FROM job_steps n WHERE n.run_id=s.run_id AND n.entity_kind=s.entity_kind AND n.entity_id=s.entity_id AND n.stage=s.stage AND n.attempt>s.attempt) GROUP BY entity_id HAVING COUNT(*)=?2)",rusqlite::params![id,run.snapshot.stages.len()],|r|r.get::<_,u32>(0)).map_err(storage_error)?;
        let page=db.query_row("SELECT p.position+1,v.title,s.stage,p.id FROM job_steps s JOIN manga_pages p ON p.id=s.entity_id JOIN manga_volumes v ON v.id=p.volume_id WHERE s.run_id=?1 AND s.entity_kind='page' ORDER BY s.rowid DESC LIMIT 1",[id],|r|Ok((r.get::<_,u32>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?,r.get::<_,String>(3)?))).optional().map_err(storage_error)?;
        (Some(count),page)
    }else{(None,None)};
    Ok(JobView {
        editing_locked_chapters: if is_book { crate::storage::results::editing_locked_chapters(db)? } else { vec![] },
        total_pages:is_manga.then_some(run.snapshot.selected_ids.len() as u32),
        completed_pages,
        current_page_number:page.as_ref().map(|p|p.0),
        current_page_id:if is_manga {page.as_ref().map(|p|p.3.clone()).or_else(||run.snapshot.selected_ids.first().cloned())} else {None},
        current_volume_title:page.as_ref().map(|p|p.1.clone()),
        job: JobRef {
            project_id: project.clone(),
            job_id: run.id,
        },
        kind: run.kind,
        state: run.state,
        revision: run.revision,
        total_steps: u32::try_from(run.snapshot.selected_ids.len() * run.snapshot.stages.len())
            .map_err(|_| AppError::invalid("jobSize"))?,
        completed_steps: completed,
        total_chapters: is_book.then_some(run.snapshot.selected_ids.len() as u32),
        completed_chapters,
        current_chapter_number: current.as_ref().map(|c| c.0),
        current_chapter_title: current.as_ref().map(|c| c.1.clone()),
        current_stage: current.map(|c| c.2).or_else(||page.map(|p|p.2)),
        remaining_seconds,
        error: run.terminal_error,
    })
}
#[tauri::command]
pub async fn job_get(context: State<'_, AppContext>, args: JobArgs) -> Result<JobView, AppError> {
    let manager = context.manager.clone();
    tauri::async_runtime::spawn_blocking(move || {
        manager
            .lease(&args.project_id)?
            .with_connection(|db, _| job_view(db, &args.project_id, &args.job_id.0))
    })
    .await
    .map_err(|_| AppError::invalid("task"))?
}
#[tauri::command]
pub async fn job_list(
    context: State<'_, AppContext>,
    args: ListJobsArgs,
) -> Result<Vec<JobView>, AppError> {
    if args.limit == 0 || args.limit > 100 {
        return Err(AppError::invalid("limit"));
    }
    let manager = context.manager.clone();
    tauri::async_runtime::spawn_blocking(move || {
        manager.lease(&args.project_id)?.with_connection(|db, _| {
            let after = if let Some(id) = &args.cursor {
                db.query_row("SELECT rowid FROM job_runs WHERE id=?1", [id], |r| {
                    r.get::<_, i64>(0)
                })
                .map_err(storage_error)?
            } else {
                i64::MAX
            };
            let mut query = db
                .prepare("SELECT id FROM job_runs WHERE rowid<?1 ORDER BY rowid DESC LIMIT ?2")
                .map_err(storage_error)?;
            let ids = query
                .query_map(rusqlite::params![after, args.limit], |r| {
                    r.get::<_, String>(0)
                })
                .map_err(storage_error)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(storage_error)?;
            ids.into_iter()
                .map(|id| job_view(db, &args.project_id, &id))
                .collect()
        })
    })
    .await
    .map_err(|_| AppError::invalid("task"))?
}

#[tauri::command]
pub async fn book_replace_preview(
    context: State<'_, AppContext>,
    args: BookReplacePreviewArgs,
) -> Result<BookReplacePreview, AppError> {
    let manager = context.manager.clone();
    let edits = context.book_edits.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let preview = manager
            .lease(&args.project_id)?
            .with_connection(|db, _| crate::application::book_edit::preview(db, &args))?;
        edits.insert(preview)
    })
    .await
    .map_err(|_| AppError::invalid("task"))?
}
#[tauri::command]
pub async fn book_replace_apply(
    context: State<'_, AppContext>,
    args: BookReplaceApplyArgs,
) -> Result<u32, AppError> {
    let manager = context.manager.clone();
    let edits = context.book_edits.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let lease = manager.lease(&args.project_id)?;
        let preview = edits.take(&args.project_id, &args.preview_id)?;
        lease.with_connection(|db, _| crate::application::book_edit::apply(db, preview))
    })
    .await
    .map_err(|_| AppError::invalid("task"))?
}

#[tauri::command]
pub async fn book_update_block(
    context: State<'_, AppContext>,
    args: UpdateBookBlockArgs,
) -> Result<crate::app::contracts::Revision, AppError> {
    let manager = context.manager.clone();
    tauri::async_runtime::spawn_blocking(move || {
        manager.lease(&args.project_id)?.with_connection(|db, _| {
            crate::storage::repository::ProjectRepository::new(
                db,
                crate::app::contracts::ProjectKind::Book,
            )?
            .update_book_text(&args.block_id.0, &args.expected_revision, &args.text)
        })
    })
    .await
    .map_err(|_| AppError::invalid("task"))?
}

#[tauri::command]
pub async fn book_export(
    context: State<'_, AppContext>,
    args: BookExportArgs,
) -> Result<(), AppError> {
    let manager = context.manager.clone();
    tauri::async_runtime::spawn_blocking(move || {
        crate::application::book_export::export_book(&manager, &args)
    })
    .await
    .map_err(|_| AppError::invalid("task"))?
}

#[tauri::command]
pub async fn book_reference_import(
    context: State<'_, AppContext>,
    args: BookReferenceImportArgs,
) -> Result<BookReferenceView, AppError> {
    let manager = context.manager.clone();
    tauri::async_runtime::spawn_blocking(move || {
        crate::application::book_reference::import(&manager, &args)
    })
    .await
    .map_err(|_| AppError::invalid("task"))?
}
#[tauri::command]
pub async fn book_reference_get(
    context: State<'_, AppContext>,
    args: ProjectArgs,
) -> Result<BookReferenceView, AppError> {
    let manager = context.manager.clone();
    tauri::async_runtime::spawn_blocking(move || {
        manager.lease(&args.project_id)?.with_connection(|db, _| {
            crate::storage::repository::ProjectRepository::new(
                db,
                crate::app::contracts::ProjectKind::Book,
            )?;
            let tx = db.transaction().map_err(storage_error)?;
            crate::application::book_reference::read(&tx)
        })
    })
    .await
    .map_err(|_| AppError::invalid("task"))?
}
#[tauri::command]
pub async fn book_reference_map(
    context: State<'_, AppContext>,
    args: BookReferenceMapArgs,
) -> Result<BookReferenceView, AppError> {
    let manager = context.manager.clone();
    tauri::async_runtime::spawn_blocking(move || {
        manager
            .lease(&args.project_id)?
            .with_connection(|db, _| crate::application::book_reference::map(db, &args))
    })
    .await
    .map_err(|_| AppError::invalid("task"))?
}

#[tauri::command]
pub async fn book_start_metadata(
    context: State<'_, AppContext>,
    app: tauri::AppHandle,
    args: StartBookMetadataArgs,
) -> Result<JobRef, AppError> {
    let manager = context.manager.clone();
    let job = tauri::async_runtime::spawn_blocking(move || {
        crate::application::runtime::prepare_metadata_run(&manager, &args.project_id, args.summary_only)
    })
    .await
    .map_err(|_| AppError::invalid("task"))??;
    dispatch_created(&context, app, job)
}
#[tauri::command]
pub async fn book_metadata_get(
    context: State<'_, AppContext>,
    args: ProjectArgs,
) -> Result<Option<BookMetadataView>, AppError> {
    let manager = context.manager.clone();
    tauri::async_runtime::spawn_blocking(move || {
        manager.lease(&args.project_id)?.with_connection(|db, _| {
            crate::storage::repository::ProjectRepository::new(
                db,
                crate::app::contracts::ProjectKind::Book,
            )?;
            let tx = db.transaction().map_err(storage_error)?;
            crate::application::book_metadata::read(&tx)
        })
    })
    .await
    .map_err(|_| AppError::invalid("task"))?
}

#[tauri::command]
pub async fn book_update_instructions(
    context: State<'_, AppContext>,
    args: UpdateChapterInstructionsArgs,
) -> Result<crate::app::contracts::Revision, AppError> {
    let manager = context.manager.clone();
    tauri::async_runtime::spawn_blocking(move || {
        manager.lease(&args.project_id)?.with_connection(|db, _| {
            crate::application::book_edit::update_instructions(db, &args)
        })
    })
    .await
    .map_err(|_| AppError::invalid("task"))?
}

#[tauri::command]
pub async fn book_start_glossary(
    context: State<'_, AppContext>,
    app: tauri::AppHandle,
    args: StartBookGlossaryArgs,
) -> Result<JobRef, AppError> {
    let manager = context.manager.clone();
    let job = tauri::async_runtime::spawn_blocking(move || {
        crate::application::runtime::prepare_glossary_run(
            &manager,
            &args.project_id,
            &args.selection,
            args.max_chapters,
            args.force,
        )
    })
    .await
    .map_err(|_| AppError::invalid("task"))??;
    dispatch_created(&context, app, job)
}

#[tauri::command]
pub async fn book_presentation_get(context: State<'_, AppContext>, args: ProjectArgs) -> Result<BookPresentation, AppError> {
    let manager=context.manager.clone();
    tauri::async_runtime::spawn_blocking(move || manager.lease(&args.project_id)?.with_connection(|db,_| crate::application::book_presentation::read(db))).await.map_err(|_|AppError::invalid("task"))?
}
#[tauri::command]
pub async fn book_presentation_update(context: State<'_, AppContext>, args: UpdateBookPresentationArgs) -> Result<BookPresentation, AppError> {
    let manager=context.manager.clone();
    tauri::async_runtime::spawn_blocking(move || manager.lease(&args.project_id)?.with_connection(|db,_| crate::application::book_presentation::update(db,&args))).await.map_err(|_|AppError::invalid("task"))?
}
#[tauri::command]
pub async fn book_cover_set(context: State<'_, AppContext>, args: SetBookCoverArgs) -> Result<BookPresentation, AppError> {
    let manager=context.manager.clone();
    tauri::async_runtime::spawn_blocking(move || manager.lease(&args.project_id)?.with_connection(|db,directory| crate::application::book_presentation::cover(db,directory,args.path.as_deref(),&args.expected_revision))).await.map_err(|_|AppError::invalid("task"))?
}

#[tauri::command]
pub async fn book_search(context:State<'_,AppContext>,args:BookSearchArgs)->Result<BookSearchPage,AppError>{
    let manager=context.manager.clone();
    tauri::async_runtime::spawn_blocking(move||manager.lease(&args.project_id)?.with_connection(|db,_|crate::application::book_search::search(db,&args))).await.map_err(|_|AppError::invalid("task"))?
}

#[tauri::command]
pub async fn book_update_translation_title(context: State<'_,AppContext>, args: UpdateTranslationTitleArgs) -> Result<crate::app::contracts::Revision,AppError> {
    let manager=context.manager.clone();
    tauri::async_runtime::spawn_blocking(move || manager.lease(&args.project_id)?.with_connection(|db,_| {
        crate::storage::repository::ProjectRepository::new(db,crate::app::contracts::ProjectKind::Book)?;
        crate::storage::results::edit_translation_title(db,&args.translation_id,&args.expected_revision,&args.title)
    })).await.map_err(|_|AppError::invalid("task"))?
}
#[tauri::command]
pub async fn book_start_title(context: State<'_,AppContext>, app: tauri::AppHandle, args: StartBookTitleArgs) -> Result<JobRef,AppError> {
    let manager=context.manager.clone();
    let job=tauri::async_runtime::spawn_blocking(move || crate::application::runtime::prepare_title_run(&manager,&args)).await.map_err(|_|AppError::invalid("task"))??;
    dispatch_created(&context,app,job)
}

#[tauri::command]
pub async fn book_start_retarget(context: State<'_, AppContext>, app: tauri::AppHandle, args: StartBookRetargetArgs) -> Result<JobRef, AppError> {
    let manager=context.manager.clone();
    let job=tauri::async_runtime::spawn_blocking(move || crate::application::book_retarget::prepare_run(&manager,&args)).await.map_err(|_|AppError::invalid("task"))??;
    dispatch_created(&context,app,job)
}

#[tauri::command]
pub async fn book_retarget_preview(context: State<'_, AppContext>,args: StartBookRetargetArgs) -> Result<BookRetargetPreview,AppError> {
    let manager=context.manager.clone();
    tauri::async_runtime::spawn_blocking(move || manager.lease(&args.project_id)?.with_connection(|db,_|crate::application::book_retarget::preview(db,&args))).await.map_err(|_|AppError::invalid("task"))?
}

#[cfg(test)]
mod progress_tests {
    use super::*;
    #[test]
    fn chapter_progress_counts_whole_chapters_and_latest_attempts() {
        let db = rusqlite::Connection::open_in_memory().unwrap();
        db.execute_batch("CREATE TABLE job_runs(id TEXT,kind TEXT,state TEXT,settings_snapshot TEXT,revision INTEGER,created_at TEXT,updated_at TEXT,terminal_error TEXT);
            CREATE TABLE job_steps(run_id TEXT,entity_kind TEXT,entity_id TEXT,stage TEXT,attempt INTEGER,state TEXT,duration_ms INTEGER);
            CREATE TABLE book_chapters(id TEXT,position INTEGER,display_number INTEGER,source_title TEXT);
            CREATE TABLE book_source_blocks(chapter_id TEXT,kind TEXT,text TEXT);
            INSERT INTO book_source_blocks VALUES('a','text','First'),('b','text','Second');
            INSERT INTO book_chapters VALUES('a',0,100,'First'),('b',1,101,'Second');").unwrap();
        let snapshot = serde_json::json!({"settings":{"target_language":"ru"},"settings_revision":"0","glossary_revision":"0","selected_ids":["a","b"],"prompt_version":"v1","stages":["glossary","translation","context"],"provider":null,"instructions":null});
        db.execute("INSERT INTO job_runs VALUES('run','book_translation','running',?1,0,'now','now',NULL)",[snapshot.to_string()]).unwrap();
        db.execute_batch("INSERT INTO job_steps VALUES('run','chapter','a','glossary',1,'succeeded',100),('run','chapter','a','translation',1,'succeeded',100),('run','chapter','a','context',1,'succeeded',100),('run','chapter','b','glossary',1,'running',NULL);").unwrap();
        let view = job_view(&db, &ProjectId::new(), "run").unwrap();
        assert!(view.editing_locked_chapters.contains(&"a".into()));
        assert!(view.editing_locked_chapters.contains(&"b".into()));
        assert_eq!(view.completed_steps, 3);
        assert_eq!(view.total_chapters, Some(2));
        assert_eq!(view.completed_chapters, Some(1));
        assert_eq!(view.current_chapter_number, Some(101));
        assert_eq!(view.current_chapter_title.as_deref(), Some("Second"));
        assert_eq!(view.current_stage.as_deref(), Some("glossary"));
        db.execute_batch("INSERT INTO job_steps VALUES('run','chapter','a','translation',2,'running',NULL);").unwrap();
        let view = job_view(&db, &ProjectId::new(), "run").unwrap();
        assert_eq!(view.completed_chapters, Some(0));
        assert_eq!(view.current_chapter_number, Some(100));
    }
}
