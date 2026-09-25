use super::lifecycle::ProjectManager;
use crate::app::{
    contracts::{BookBlockContent, ProjectKind},
    requests::{DomainProgress, LanguagePair, ProjectChoices},
};
use std::{path::PathBuf, sync::Arc};
struct Fixture {
    root: PathBuf,
    manager: Arc<ProjectManager>,
}
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("lifecycle-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        Self {
            manager: Arc::new(ProjectManager::new(root.join("app"))),
            root,
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}
fn source(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../tests/fixtures")
        .join(name)
}
fn choices() -> ProjectChoices {
    ProjectChoices {
        name: "Example".into(),
        languages: LanguagePair {
            source: Some("en".into()),
            target: "ru".into(),
        },
        processing_profile_id: None,
    }
}

#[test]
fn staged_book_import_is_atomic_idempotent_and_independent_of_source() {
    let f = Fixture::new();
    let input = f.root.join("book.epub");
    std::fs::copy(source("structural.epub"), &input).unwrap();
    let preview = f.manager.inspect_source(ProjectKind::Book, &input).unwrap();
    assert!(f.manager.catalog().unwrap().is_empty());
    std::fs::remove_file(input).unwrap();
    let created = f.manager.create(&preview.import_id.0, &choices()).unwrap();
    assert_eq!(
        f.manager.create(&preview.import_id.0, &choices()).unwrap(),
        created
    );
    assert_eq!(f.manager.open(&created.id).unwrap(), created);
    assert_eq!(f.manager.catalog().unwrap().len(), 1);
    f.manager
        .lease(&created.id)
        .unwrap()
        .with_connection(|db, _| {
            assert_eq!(
                db.query_row("SELECT COUNT(*) FROM book_chapters", [], |r| r
                    .get::<_, i64>(0))
                    .unwrap(),
                3
            );
            assert_eq!(
                db.query_row("SELECT COUNT(*) FROM assets", [], |r| r.get::<_, i64>(0))
                    .unwrap(),
                1
            );
            assert_eq!(
                db.query_row(
                    "SELECT COUNT(*) FROM book_source_blocks WHERE kind='image'",
                    [],
                    |r| r.get::<_, i64>(0)
                )
                .unwrap(),
                3
            );
            let ids = {
                let mut q = db
                    .prepare("SELECT id FROM book_chapters ORDER BY position")
                    .unwrap();
                let rows = q.query_map([], |r| r.get::<_, String>(0)).unwrap();
                rows.collect::<Result<Vec<_>, _>>().unwrap()
            };
            let mut repo =
                crate::storage::repository::ProjectRepository::new(db, ProjectKind::Book)?;
            for id in ids {
                for block in repo.chapter(&id)?.blocks {
                    if let BookBlockContent::Image { asset_id, .. } = block.content {
                        assert_eq!(asset_id.len(), 64);
                    }
                }
            }
            Ok(())
        })
        .unwrap();
    let archive = f.root.join("copy.bcproj");
    f.manager.export_archive(&created.id, &archive).unwrap();
    let imported = f.manager.import_archive(&archive).unwrap();
    assert_ne!(imported.id, created.id);
    assert_eq!(f.manager.catalog().unwrap().len(), 2);
    let zip = zip::ZipArchive::new(std::fs::File::open(&archive).unwrap()).unwrap();
    assert!(zip
        .file_names()
        .all(|name| name == "project.json" || name == "project.db" || name.starts_with("assets/")));
}

#[test]
fn comic_catalog_has_pages_without_fake_chapters_and_cancel_leaves_no_project() {
    let f = Fixture::new();
    let preview = f
        .manager
        .inspect_source(ProjectKind::Manga, &source("two-volumes.cbz"))
        .unwrap();
    f.manager.cancel_import(&preview.import_id.0).unwrap();
    f.manager.cancel_import(&preview.import_id.0).unwrap();
    assert!(f.manager.catalog().unwrap().is_empty());
    let preview = f
        .manager
        .inspect_source(ProjectKind::Manga, &source("two-volumes.cbz"))
        .unwrap();
    let project = f.manager.create(&preview.import_id.0, &choices()).unwrap();
    assert_eq!(
        f.manager.catalog().unwrap()[0].progress,
        DomainProgress::Manga {
            pages: 6,
            lettered: 0,
            approved: 0
        }
    );
    f.manager
        .lease(&project.id)
        .unwrap()
        .with_connection(|db, _| {
            assert_eq!(
                db.query_row("SELECT COUNT(*) FROM manga_volumes", [], |r| r
                    .get::<_, i64>(0))
                    .unwrap(),
                2
            );
            assert_eq!(
                db.query_row("SELECT COUNT(*) FROM book_chapters", [], |r| r
                    .get::<_, i64>(0))
                    .unwrap(),
                0
            );
            Ok(())
        })
        .unwrap();
    let mut names = vec!["v10/p1", "v2/p10", "v2/p2", "v2/p1"];
    names.sort_by(|a, b| super::import::natural_cmp(a, b));
    assert_eq!(names, vec!["v2/p1", "v2/p2", "v2/p10", "v10/p1"]);
}

#[test]
fn deletion_cancels_and_waits_for_leases_before_removing_bytes() {
    let f = Fixture::new();
    let preview = f
        .manager
        .inspect_source(ProjectKind::Book, &source("structural.epub"))
        .unwrap();
    let project = f.manager.create(&preview.import_id.0, &choices()).unwrap();
    let lease = f.manager.lease(&project.id).unwrap();
    let manager = f.manager.clone();
    let id = project.id.clone();
    let (send, receive) = std::sync::mpsc::channel();
    let thread = std::thread::spawn(move || {
        manager.delete(&id).unwrap();
        send.send(()).unwrap();
    });
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while !lease.cancelled() {
        assert!(std::time::Instant::now() < deadline);
        std::thread::yield_now();
    }
    assert!(receive.try_recv().is_err());
    assert!(lease.with_connection(|_, _| Ok(())).is_err());
    assert!(f
        .root
        .join("app/projects")
        .join(project.id.as_str())
        .exists());
    drop(lease);
    receive
        .recv_timeout(std::time::Duration::from_secs(5))
        .unwrap();
    thread.join().unwrap();
    assert!(!f
        .root
        .join("app/projects")
        .join(project.id.as_str())
        .exists());
    assert!(f.manager.lease(&project.id).is_err());
}

#[test]
fn failed_source_and_unsafe_archive_do_not_publish_or_leave_staging() {
    use std::io::Write;
    let f = Fixture::new();
    assert!(f
        .manager
        .inspect_source(ProjectKind::Book, &f.root.join("missing.epub"))
        .is_err());
    let path = f.root.join("bad.bcproj");
    let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
    zip.start_file("../escape", zip::write::SimpleFileOptions::default())
        .unwrap();
    zip.write_all(b"bad").unwrap();
    zip.finish().unwrap();
    assert!(f.manager.import_archive(&path).is_err());
    assert!(f.manager.catalog().unwrap().is_empty());
    assert_eq!(
        std::fs::read_dir(f.root.join("app/staging"))
            .unwrap()
            .count(),
        0
    );
    assert!(!f.root.join("escape").exists());
    assert!(f.manager.cancel_import("../projects").is_err());
}

#[test]
fn reset_requires_quiescence_preserves_settings_and_runs_once() {
    let f = Fixture::new();
    let root = f.root.join("app");
    let old = root.join("projects/legacy-test");
    std::fs::create_dir_all(&old).unwrap();
    let seed = || {
        std::fs::create_dir_all(&old).unwrap();
        std::fs::write(
            old.join("project.json"),
            br#"{"name":"Legacy","source_path":"/preserve/source.epub"}"#,
        )
        .unwrap();
        std::fs::write(old.join("progress.db"), b"legacy").unwrap();
    };
    seed();
    std::fs::write(root.join("settings.db"), b"keep").unwrap();
    std::fs::write(root.join(".env"), b"keep").unwrap();
    assert!(f
        .manager
        .execute_reset(|_| Err(crate::app::contracts::AppError::invalid("busy")))
        .is_err());
    assert!(old.exists());
    let report = f
        .manager
        .execute_reset(|ids| {
            assert_eq!(ids, &["legacy-test"]);
            Ok(())
        })
        .unwrap();
    assert_eq!(report.removed_project_ids, vec!["legacy-test"]);
    assert!(!old.exists());
    assert_eq!(std::fs::read(root.join("settings.db")).unwrap(), b"keep");
    assert!(root.join(".env").exists());
    seed();
    assert!(
        f.manager
            .execute_reset(|_| panic!("already reset"))
            .unwrap()
            .already_completed
    );
    assert!(old.exists());
}

#[test]
fn manga_import_reports_real_page_progress_and_finalization() {
    let f = Fixture::new();
    let mut events = Vec::new();
    f.manager
        .inspect_source_with_progress(
            ProjectKind::Manga,
            &source("two-volumes.cbz"),
            &mut |event| events.push(event),
        )
        .unwrap();
    assert_eq!(events.first().unwrap().stage, "scanning");
    assert_eq!(events.last().unwrap().stage, "finalizing");
    let pages: Vec<_> = events
        .iter()
        .filter(|event| event.stage == "pages")
        .collect();
    assert_eq!(pages.first().unwrap().completed, 0);
    let last = pages.last().unwrap();
    assert!(last.completed > 0);
    assert_eq!(last.total, Some(last.completed));
    assert!(pages
        .windows(2)
        .all(|pair| pair[1].completed == pair[0].completed + 1));
}
