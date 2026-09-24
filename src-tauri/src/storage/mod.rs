//! Explicit versioned connections. Old progress databases are never migrated here.
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
        tx.execute(
            "INSERT INTO project_settings(singleton,kind,target_language) VALUES(1,?1,?2)",
            (
                match kind {
                    ProjectKind::Book => "book",
                    ProjectKind::Manga => "manga",
                },
                target_language,
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
    Ok(connection)
}

/// The revision predicate and mutation execute in one SQLite statement.
pub fn update_book_text(
    connection: &mut Connection,
    id: &str,
    expected: i64,
    text: &str,
) -> anyhow::Result<i64> {
    let tx = connection.transaction()?;
    let chapter: String = tx.query_row(
        "UPDATE book_source_blocks SET text=?1,revision=revision+1 WHERE id=?2 AND revision=?3 AND kind IN ('text','caption') RETURNING chapter_id",
        (text,id,expected), |row| row.get(0))?;
    tx.execute(
        "UPDATE book_chapters SET revision=revision+1 WHERE id=?1",
        [&chapter],
    )?;
    tx.execute(
        "UPDATE book_translations SET status='stale' WHERE chapter_id=?1",
        [&chapter],
    )?;
    tx.commit()?;
    Ok(expected + 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn database(kind: ProjectKind) -> Connection {
        let connection = Connection::open_in_memory().unwrap();
        configure(&connection).unwrap();
        connection
            .execute_batch(include_str!("schema.sql"))
            .unwrap();
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
        assert_eq!(update_book_text(&mut db, "b", 0, "After").unwrap(), 1);
        assert!(update_book_text(&mut db, "b", 0, "Late result").is_err());
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
    }
}
