//! Structural book and durable job IPC. Domain work lives in application services.
use crate::app::{
    contracts::{AppError, JobRef, ProjectId},
    requests::*,
    services::AppContext,
};
use crate::storage::{repository::storage_error, runs};
use tauri::{Emitter, State};

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
        let mut query=db.prepare("SELECT id,position,source_title,revision FROM book_chapters WHERE position>?1 ORDER BY position LIMIT ?2").map_err(storage_error)?;
        let rows=query.query_map(rusqlite::params![after,args.limit+1],|r|Ok(ChapterSummary{id:crate::app::contracts::ChapterId(r.get(0)?),position:r.get(1)?,title:r.get(2)?,revision:crate::app::contracts::Revision(r.get::<_,i64>(3)?.to_string())})).map_err(storage_error)?;
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
    Ok(JobView {
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
    args: ProjectArgs,
) -> Result<JobRef, AppError> {
    let manager = context.manager.clone();
    let job = tauri::async_runtime::spawn_blocking(move || {
        crate::application::runtime::prepare_metadata_run(&manager, &args.project_id)
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
            crate::storage::repository::ProjectRepository::new(
                db,
                crate::app::contracts::ProjectKind::Book,
            )?
            .update_chapter_instructions(
                &args.chapter_id.0,
                &args.expected_revision,
                &args.instructions,
            )
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
