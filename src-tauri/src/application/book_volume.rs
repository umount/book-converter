//! Standalone volume titles do not require a translated chapter body.
use crate::{app::{contracts::{AppError, Revision}, requests::BookVolumeTitle}, storage::{repository::{storage_error, conflict}, shared}};
use rusqlite::{Connection, OptionalExtension};

pub fn read(db: &Connection, source: &str) -> Result<BookVolumeTitle, AppError> {
    let language = shared::settings(db)?.choices.target_language;
    let saved: Option<(String,i64)> = db.query_row("SELECT title,revision FROM book_volume_titles WHERE source=?1 AND target_language=?2", rusqlite::params![source,language], |r| Ok((r.get(0)?,r.get(1)?))).optional().map_err(storage_error)?;
    let (title, revision) = saved.unwrap_or((String::new(),0));
    Ok(BookVolumeTitle {source: source.into(), title, revision: Revision(revision.to_string())})
}
pub fn save(db: &mut Connection, source: &str, title: &str, expected: &Revision) -> Result<(), AppError> {
    if source.trim().is_empty() || source.len()>1024 || title.len()>4096 { return Err(AppError::invalid("volumeTitle")); }
    let tx = db.transaction().map_err(storage_error)?;
    if read(&tx,source)?.revision != *expected { return Err(conflict()); }
    let language = shared::settings(&tx)?.choices.target_language;
    tx.execute("INSERT INTO book_volume_titles(source,target_language,title,revision) VALUES(?1,?2,?3,?4) ON CONFLICT(source,target_language) DO UPDATE SET title=excluded.title,revision=excluded.revision", rusqlite::params![source,language,title.trim(),expected.value()?.checked_add(1).ok_or_else(||AppError::invalid("revision"))?]).map_err(storage_error)?;
    tx.commit().map_err(storage_error)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn saves_without_translation_and_rejects_stale_edits() {
        let mut db=crate::storage::tests::database(crate::app::contracts::ProjectKind::Book);
        let source="第2部 · 第7集";
        assert_eq!(read(&db,source).unwrap().title, "");
        save(&mut db,source,"Часть 2 · Том 7",&Revision("0".into())).unwrap();
        assert_eq!(read(&db,source).unwrap().title,"Часть 2 · Том 7");
        assert!(save(&mut db,source,"Late",&Revision("0".into())).is_err());
        db.execute("INSERT INTO book_volume_titles VALUES(?1,'en','Part 2 · Volume 7',1)",[source]).unwrap();
        assert_eq!(read(&db,source).unwrap().title,"Часть 2 · Том 7");
    }
}
