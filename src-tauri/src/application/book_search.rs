//! Bounded literal search over a consistent structural-book snapshot.
use crate::{
    app::{contracts::*, requests::*},
    storage::repository::{storage_error, ProjectRepository},
};
use rusqlite::Connection;
pub fn search(db: &mut Connection, args: &BookSearchArgs) -> Result<BookSearchPage, AppError> {
    if args.query.trim().is_empty()
        || args.query.len() > 1024
        || args.limit == 0
        || args.limit > 100
    {
        return Err(AppError::invalid("search"));
    }
    ProjectRepository::new(db, ProjectKind::Book)?;
    let pattern = regex::RegexBuilder::new(&regex::escape(&args.query))
        .case_insensitive(!args.case_sensitive)
        .build()
        .map_err(|_| AppError::invalid("search"))?;
    let tx = db.transaction().map_err(storage_error)?;
    let after = if let Some(id) = &args.cursor {
        tx.query_row("SELECT c.position,b.position FROM book_source_blocks b JOIN book_chapters c ON c.id=b.chapter_id WHERE b.id=?1",[id],|r|Ok((r.get::<_,i64>(0)?,r.get::<_,i64>(1)?))).map_err(storage_error)?
    } else {
        (-1, -1)
    };
    let expression=match args.side {BookSearchSide::Source=>"b.text",BookSearchSide::Translation=>"(SELECT tb.translated_text FROM book_translation_blocks tb JOIN book_translations t ON t.id=tb.translation_id WHERE tb.source_block_id=b.id AND t.target_language=(SELECT target_language FROM project_settings) ORDER BY t.revision DESC LIMIT 1)"};
    let sql=format!("SELECT b.id,c.id,c.source_title,{expression} FROM book_source_blocks b JOIN book_chapters c ON c.id=b.chapter_id WHERE b.kind IN ('text','caption') AND (c.position,b.position)>(?1,?2) ORDER BY c.position,b.position");
    let mut q = tx.prepare(&sql).map_err(storage_error)?;
    let mut rows = q
        .query(rusqlite::params![after.0, after.1])
        .map_err(storage_error)?;
    let mut matches = Vec::new();
    while let Some(row) = rows.next().map_err(storage_error)? {
        let Some(text) = row.get::<_, Option<String>>(3).map_err(storage_error)? else {
            continue;
        };
        let Some(found) = pattern.find(&text) else {
            continue;
        };
        let before = text[..found.start()]
            .chars()
            .rev()
            .take(80)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect::<String>();
        let after = text[found.end()..].chars().take(80).collect::<String>();
        let snippet = format!(
            "{}{}{}{}{}",
            if found.start() > before.len() {
                "…"
            } else {
                ""
            },
            before,
            found.as_str(),
            after,
            if found.end() + after.len() < text.len() {
                "…"
            } else {
                ""
            }
        );
        matches.push(BookSearchMatch {
            chapter_id: ChapterId(row.get(1).map_err(storage_error)?),
            block_id: BlockId(row.get(0).map_err(storage_error)?),
            title: row.get(2).map_err(storage_error)?,
            snippet,
        });
        if matches.len() > args.limit as usize {
            break;
        }
    }
    let next_cursor = if matches.len() > args.limit as usize {
        matches.pop();
        matches.last().map(|m| m.block_id.0.clone())
    } else {
        None
    };
    Ok(BookSearchPage {
        matches,
        next_cursor,
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unicode_literal_search_paginates_without_interpreting_regex() {
        let mut db = crate::storage::tests::database(ProjectKind::Book);
        for i in 0..3 {
            let id = format!("c{i}");
            db.execute(
                "INSERT INTO book_chapters(id,position,source_title) VALUES(?1,?2,'Chapter')",
                rusqlite::params![id, i],
            )
            .unwrap();
            db.execute("INSERT INTO book_source_blocks(id,chapter_id,position,kind,text) VALUES(?1,?1,0,'text','中文 [Name] 示例')",[&id]).unwrap();
        }
        let mut args = BookSearchArgs {
            project_id: ProjectId::new(),
            query: "[name]".into(),
            side: BookSearchSide::Source,
            case_sensitive: false,
            cursor: None,
            limit: 2,
        };
        let first = search(&mut db, &args).unwrap();
        assert_eq!(first.matches.len(), 2);
        assert_eq!(first.matches[0].snippet, "中文 [Name] 示例");
        args.cursor = first.next_cursor;
        let last = search(&mut db, &args).unwrap();
        assert_eq!(last.matches[0].chapter_id.0, "c2");
        assert!(last.next_cursor.is_none());
        args.cursor = None;
        args.case_sensitive = true;
        assert!(search(&mut db, &args).unwrap().matches.is_empty());
        args.case_sensitive = false;
        args.side = BookSearchSide::Translation;
        assert!(search(&mut db, &args).unwrap().matches.is_empty());
        db.execute("INSERT INTO book_translations(id,chapter_id,source_revision,status,provenance,target_language,translated_title,context_fingerprint,glossary_revision,revision) VALUES('t','c0',0,'ready','manual','ru','','',0,0)",[]).unwrap();
        db.execute("INSERT INTO book_translation_blocks(translation_id,chapter_id,source_block_id,translated_text) VALUES('t','c0','c0','Перевод [Name]')",[]).unwrap();
        assert_eq!(
            search(&mut db, &args).unwrap().matches[0].snippet,
            "Перевод [Name]"
        );
    }
}
