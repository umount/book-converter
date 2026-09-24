//! Explicit versioned connections. Old progress databases are never migrated here.
pub mod edits;
pub mod repository;
pub mod results;
pub mod runs;
pub mod shared;

use crate::app::contracts::ProjectKind;
use rusqlite::{Connection, OpenFlags};
use std::{path::Path, time::Duration};

fn configure(connection: &Connection) -> anyhow::Result<()> {
    connection.pragma_update(None, "foreign_keys", true)?;
    connection.pragma_update(None, "journal_mode", "WAL")?;
    connection.busy_timeout(Duration::from_secs(5))?;
    Ok(())
}

pub fn create(path: &Path, kind: ProjectKind, target_language: &str) -> anyhow::Result<Connection> {
    create_with_lock(path,kind,target_language,true)
}
pub(crate) fn create_staged(path:&Path,kind:ProjectKind)->anyhow::Result<Connection>{
    create_with_lock(path,kind,"und",false)
}
fn create_with_lock(path:&Path,kind:ProjectKind,target_language:&str,locked:bool)->anyhow::Result<Connection>{
    // Reserve the name exclusively; never open and mutate a concurrent creator's file.
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    let result = (|| -> anyhow::Result<Connection> {
        let mut connection = Connection::open(path)?;
        configure(&connection)?;
        let tx = connection.transaction()?;
        tx.execute_batch(include_str!("schema.sql"))?;
        tx.execute_batch(include_str!("schema_extensions.sql"))?;
        tx.execute(
            "INSERT INTO project_settings(singleton,kind,target_language,languages_locked) VALUES(1,?1,?2,?3)",
            (
                match kind {
                    ProjectKind::Book => "book",
                    ProjectKind::Manga => "manga",
                },
                target_language, locked,
            ),
        )?;
        tx.commit()?;
        Ok(connection)
    })();
    if result.is_err() {
        // The failed connection has dropped before cleanup (required on Windows).
        let _ = std::fs::remove_file(path);
        for suffix in ["-wal", "-shm"] {
            let mut sidecar = path.as_os_str().to_os_string();
            sidecar.push(suffix);
            let _ = std::fs::remove_file(std::path::PathBuf::from(sidecar));
        }
    }
    result
}

pub fn open(path: &Path) -> anyhow::Result<Connection> {
    let connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_WRITE)?;
    let version: u32 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
    anyhow::ensure!(version == 1, "Unsupported project database version");
    configure(&connection)?;
    connection.execute_batch(include_str!("schema_extensions.sql"))?;
    Ok(connection)
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    pub(crate) fn database(kind: ProjectKind) -> Connection {
        let connection = Connection::open_in_memory().unwrap();
        configure(&connection).unwrap();
        connection
            .execute_batch(include_str!("schema.sql"))
            .unwrap();
        connection.execute_batch(include_str!("schema_extensions.sql")).unwrap();
        connection
            .execute(
                "INSERT INTO project_settings(singleton,kind,target_language) VALUES(1,?1,'ru')",
                [if kind == ProjectKind::Book {
                    "book"
                } else {
                    "manga"
                }],
            )
            .unwrap();
        connection
    }

    #[test]
    fn failed_creation_cleans_up_and_existing_database_is_preserved() {
        let root = std::env::temp_dir().join(format!("schema-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("project.db");
        assert!(create(&path, ProjectKind::Book, "").is_err());
        assert!(!path.exists());
        let connection = create(&path, ProjectKind::Book, "ru").unwrap();
        assert!(create(&path, ProjectKind::Manga, "en").is_err());
        assert_eq!(
            connection
                .query_row("SELECT kind FROM project_settings", [], |r| r
                    .get::<_, String>(0))
                .unwrap(),
            "book"
        );
        drop(connection);
        assert!(open(&path).is_ok());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn domains_cannot_be_mixed() {
        let db = database(ProjectKind::Manga);
        assert!(db
            .execute(
                "INSERT INTO book_chapters(id,position,source_title) VALUES('c',0,'Chapter')",
                []
            )
            .is_err());
        db.execute("INSERT INTO manga_volumes VALUES('v',0,'Volume','rtl')", [])
            .unwrap();
    }

    #[test]
    fn text_updates_compare_revisions_atomically() {
        let mut db = database(ProjectKind::Book);
        db.execute(
            "INSERT INTO book_chapters(id,position,source_title) VALUES('c',0,'Chapter')",
            [],
        )
        .unwrap();
        db.execute("INSERT INTO book_source_blocks(id,chapter_id,position,kind,text) VALUES('b','c',0,'text','Before')", []).unwrap();
        let mut repository =
            repository::ProjectRepository::new(&mut db, ProjectKind::Book).unwrap();
        let revision = crate::app::contracts::Revision("0".into());
        assert_eq!(
            repository
                .update_book_text("b", &revision, "After")
                .unwrap()
                .0,
            "1"
        );
        assert_eq!(
            repository
                .update_book_text("b", &revision, "Late result")
                .unwrap_err()
                .code,
            crate::app::contracts::ErrorCode::RevisionConflict
        );
        assert_eq!(
            repository
                .update_book_text("missing", &revision, "After")
                .unwrap_err()
                .code,
            crate::app::contracts::ErrorCode::NotFound
        );
        assert_eq!(
            db.query_row(
                "SELECT text FROM book_source_blocks WHERE id='b'",
                [],
                |r| r.get::<_, String>(0)
            )
            .unwrap(),
            "After"
        );
    }

    #[test]
    fn image_blocks_require_real_assets_and_consistent_payloads() {
        let db = database(ProjectKind::Book);
        db.execute(
            "INSERT INTO book_chapters(id,position,source_title) VALUES('c',0,'Chapter')",
            [],
        )
        .unwrap();
        assert!(db.execute("INSERT INTO book_source_blocks(id,chapter_id,position,kind,asset_id) VALUES('b','c',0,'image','missing')", []).is_err());
        assert!(db.execute("INSERT INTO book_source_blocks(id,chapter_id,position,kind,text) VALUES('b','c',0,'image','fake')", []).is_err());
    }

    #[test]
    fn repeated_images_keep_separate_page_identity() {
        let db = database(ProjectKind::Manga);
        db.execute("INSERT INTO assets(id,relative_path,mime,byte_length) VALUES(?1,'assets/test.png','image/png',100)",["a".repeat(64)]).unwrap();
        db.execute("INSERT INTO manga_volumes VALUES('v',0,'Volume','rtl')", [])
            .unwrap();
        for position in 0..2 {
            db.execute("INSERT INTO manga_pages(id,volume_id,position,original_asset_id,width,height) VALUES(?1,'v',?2,?3,32,32)",
                (format!("p{position}"),position,"a".repeat(64))).unwrap();
        }
        assert_eq!(
            db.query_row("SELECT COUNT(*) FROM manga_pages", [], |r| r
                .get::<_, u32>(0))
                .unwrap(),
            2
        );
        assert!(db.execute("DELETE FROM assets", []).is_err());
        assert!(db
            .execute("UPDATE assets SET relative_path='assets/replaced.png'", [])
            .is_err());
        assert!(db.execute("UPDATE manga_pages SET width=64", []).is_err());
        assert!(db
            .execute("UPDATE project_settings SET kind='book'", [])
            .is_err());
    }
}

#[cfg(test)]
mod persistence_tests;
