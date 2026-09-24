//! Read-only manga workspace before model processing is configured.
use crate::{
    app::{
        contracts::{AppError, AssetId, PageId, ProjectKind, Revision, VolumeId},
        requests::{
            ListMangaPagesArgs, MangaVolumeSummary, PageSummary, PageSummaryPage, ProjectArgs,
        },
        services::AppContext,
    },
    storage::repository::{storage_error, ProjectRepository},
};
use rusqlite::{Connection, OptionalExtension};

#[tauri::command]
pub async fn manga_list_volumes(
    context: tauri::State<'_, AppContext>,
    args: ProjectArgs,
) -> Result<Vec<MangaVolumeSummary>, AppError> {
    let manager = context.manager.clone();
    tauri::async_runtime::spawn_blocking(move || {
        manager
            .lease(&args.project_id)?
            .with_connection(|db, _| list_volumes(db))
    })
    .await
    .map_err(|_| AppError::invalid("task"))?
}

fn list_volumes(db: &mut Connection) -> Result<Vec<MangaVolumeSummary>, AppError> {
    ProjectRepository::new(db, ProjectKind::Manga)?;
    let mut query = db.prepare("SELECT v.id,v.title,v.reading_direction,COUNT(p.id) FROM manga_volumes v LEFT JOIN manga_pages p ON p.volume_id=v.id GROUP BY v.id ORDER BY v.position").map_err(storage_error)?;
    let rows = query
        .query_map([], |r| {
            Ok(MangaVolumeSummary {
                id: VolumeId(r.get(0)?),
                title: r.get(1)?,
                reading_direction: r.get(2)?,
                page_count: r.get(3)?,
            })
        })
        .map_err(storage_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(storage_error)?;
    Ok(rows)
}

#[tauri::command]
pub async fn manga_list_pages(
    context: tauri::State<'_, AppContext>,
    args: ListMangaPagesArgs,
) -> Result<PageSummaryPage, AppError> {
    let manager = context.manager.clone();
    tauri::async_runtime::spawn_blocking(move || {
        manager
            .lease(&args.project_id)?
            .with_connection(|db, _| list_pages(db, &args))
    })
    .await
    .map_err(|_| AppError::invalid("task"))?
}

fn list_pages(db: &mut Connection, args: &ListMangaPagesArgs) -> Result<PageSummaryPage, AppError> {
    if args.limit == 0 || args.limit > 500 {
        return Err(AppError::invalid("limit"));
    }
    ProjectRepository::new(db, ProjectKind::Manga)?;
    let tx = db.transaction().map_err(storage_error)?;
    let volume = args.volume_id.as_ref().map(|id| id.0.as_str());
    let after = if let Some(id) = &args.cursor {
        tx.query_row(
            "SELECT v.position,p.position FROM manga_pages p JOIN manga_volumes v ON v.id=p.volume_id WHERE p.id=?1 AND (?2 IS NULL OR p.volume_id=?2)",
            rusqlite::params![id, volume],
            |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)),
        ).optional().map_err(storage_error)?.ok_or_else(|| AppError::invalid("cursor"))?
    } else {
        (-1, -1)
    };
    let mut query = tx.prepare(
        "SELECT p.id,p.volume_id,p.position,p.original_asset_id,p.width,p.height,p.revision,(SELECT asset_id FROM manga_page_previews WHERE page_id=p.id) FROM manga_pages p JOIN manga_volumes v ON v.id=p.volume_id WHERE (?1 IS NULL OR p.volume_id=?1) AND (v.position,p.position)>(?2,?3) ORDER BY v.position,p.position LIMIT ?4",
    ).map_err(storage_error)?;
    let mut items = query
        .query_map(
            rusqlite::params![volume, after.0, after.1, args.limit + 1],
            |r| {
                Ok(PageSummary {
                    id: PageId(r.get(0)?),
                    volume_id: VolumeId(r.get(1)?),
                    position: r.get(2)?,
                    original_asset_id: AssetId(r.get(3)?),
                    thumbnail_asset_id: r.get::<_, Option<String>>(7)?.map(AssetId),
                    width: r.get(4)?,
                    height: r.get(5)?,
                    revision: Revision(r.get::<_, i64>(6)?.to_string()),
                })
            },
        )
        .map_err(storage_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(storage_error)?;
    let next_cursor = if items.len() > args.limit as usize {
        items.pop();
        items.last().map(|v| v.id.0.clone())
    } else {
        None
    };
    Ok(PageSummaryPage { items, next_cursor })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::contracts::ProjectId;
    #[test]
    fn pagination_crosses_volume_boundaries_without_skipping_or_mixing_domains() {
        let mut db = Connection::open_in_memory().unwrap();
        db.execute_batch(include_str!("../storage/schema.sql"))
            .unwrap();
        db.execute_batch(include_str!("../storage/schema_extensions.sql"))
            .unwrap();
        db.execute(
            "INSERT INTO project_settings(singleton,kind,target_language) VALUES(1,'manga','ru')",
            [],
        )
        .unwrap();
        db.execute("INSERT INTO assets(id,relative_path,mime,byte_length) VALUES(?1,'assets/test.png','image/png',1)", ["a".repeat(64)]).unwrap();
        for (id, position) in [("v1", 0), ("v2", 1)] {
            db.execute(
                "INSERT INTO manga_volumes VALUES(?1,?2,'Volume','rtl')",
                rusqlite::params![id, position],
            )
            .unwrap();
            for page in 0..2 {
                db.execute(
                    "INSERT INTO manga_pages VALUES(?1,?2,?3,?4,100,200,0)",
                    rusqlite::params![format!("{id}-{page}"), id, page, "a".repeat(64)],
                )
                .unwrap();
            }
        }
        let volumes = list_volumes(&mut db).unwrap();
        assert_eq!(
            volumes
                .iter()
                .map(|v| (v.id.0.as_str(), v.page_count, v.reading_direction.as_str()))
                .collect::<Vec<_>>(),
            vec![("v1", 2, "rtl"), ("v2", 2, "rtl")]
        );
        let mut args = ListMangaPagesArgs {
            project_id: ProjectId::new(),
            volume_id: None,
            cursor: None,
            limit: 3,
        };
        let first = list_pages(&mut db, &args).unwrap();
        assert_eq!(
            first
                .items
                .iter()
                .map(|v| v.id.0.as_str())
                .collect::<Vec<_>>(),
            vec!["v1-0", "v1-1", "v2-0"]
        );
        args.cursor = first.next_cursor;
        let last = list_pages(&mut db, &args).unwrap();
        assert_eq!(last.items[0].id.0, "v2-1");
        assert!(last.next_cursor.is_none());
        args.volume_id = Some(VolumeId("v1".into()));
        assert!(list_pages(&mut db, &args).is_err());
        args.cursor = None;
        assert_eq!(list_pages(&mut db, &args).unwrap().items.len(), 2);
        args.limit = 0;
        assert!(list_pages(&mut db, &args).is_err());
        args.limit = 1;
        let mut book_db = Connection::open_in_memory().unwrap();
        book_db
            .execute_batch(include_str!("../storage/schema.sql"))
            .unwrap();
        book_db.execute("INSERT INTO project_settings(singleton,kind,target_language) VALUES(1,'book','ru')", []).unwrap();
        assert!(list_pages(&mut book_db, &args).is_err());
        assert!(list_volumes(&mut book_db).is_err());
    }
}

#[tauri::command]
pub async fn manga_start_stage(
    context: tauri::State<'_, AppContext>,
    app: tauri::AppHandle,
    args: crate::app::requests::StartMangaStageArgs,
) -> Result<crate::app::contracts::JobRef, AppError> {
    let manager = context.manager.clone();
    let job = tauri::async_runtime::spawn_blocking(move || {
        crate::application::manga::runtime::prepare(&manager, &args)
    })
    .await
    .map_err(|_| AppError::invalid("task"))??;
    super::book_v1::dispatch_created(&context, app, job)
}

pub(super) fn dispatch(
    context: &AppContext,
    app: tauri::AppHandle,
    project: crate::app::contracts::ProjectId,
    job: String,
) -> Result<(), AppError> {
    use tauri::Emitter;
    let pipeline =
        crate::application::manga::runtime::resume_provider(&context.manager, &project, &job)?;
    let cancel = context.book_jobs.reserve(&project, &job)?;
    let manager = context.manager.clone();
    let runtime = context.book_jobs.clone();
    tauri::async_runtime::spawn(async move {
        let _reservation = crate::application::runtime::Reservation {
            runtime,
            project: project.clone(),
            job: job.clone(),
        };
        if let Err(error) =
            crate::jobs::durable::execute(&manager, &project, &job, &pipeline, cancel, |event| {
                let _ = app.emit("project-event", event);
            })
            .await
        {
            tracing::warn!(project = project.as_str(), job, code = ?error.code, "Manga job stopped");
        }
    });
    Ok(())
}

#[tauri::command]
pub async fn manga_get_page(
    context: tauri::State<'_, AppContext>,
    args: crate::app::requests::GetMangaPageArgs,
) -> Result<crate::app::requests::MangaPageView, AppError> {
    let manager = context.manager.clone();
    tauri::async_runtime::spawn_blocking(move || {
        manager
            .lease(&args.project_id)?
            .with_connection(|db, _| crate::application::manga::view::page(db, &args.page_id.0))
    })
    .await
    .map_err(|_| AppError::invalid("task"))?
}

/// Inspect local setup without uploading pages, downloading weights or creating jobs.
#[tauri::command]
pub async fn manga_preflight(
    context: tauri::State<'_, AppContext>,
    args: ProjectArgs,
) -> Result<crate::app::contracts::MangaPreflight, AppError> {
    let manager = context.manager.clone();
    tauri::async_runtime::spawn_blocking(move || {
        manager.lease(&args.project_id)?.with_connection(|db, _| {
            crate::application::manga::preflight::inspect(db)
        })
    }).await.map_err(|_| AppError::invalid("task"))?
}
