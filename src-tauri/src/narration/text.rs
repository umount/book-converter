use super::{contracts::*, failure, ChapterInput, Input};
use crate::{
    app::contracts::ProjectKind,
    project::lifecycle::ProjectLease,
    storage::{
        repository::{storage_error, ProjectRepository},
        shared,
    },
};
use rusqlite::OptionalExtension;

pub(super) const VOICES: &[&str] = &[
    "Vivian", "Serena", "Uncle_Fu", "Dylan", "Eric", "Ryan", "Aiden", "Ono_Anna", "Sohee",
];
pub(super) fn language(code: &str) -> Option<&'static str> {
    match code.to_lowercase().split(['-', '_']).next()? {
        "ru" => Some("Russian"),
        "en" => Some("English"),
        "zh" => Some("Chinese"),
        "ja" => Some("Japanese"),
        "ko" => Some("Korean"),
        "de" => Some("German"),
        "fr" => Some("French"),
        "es" => Some("Spanish"),
        "it" => Some("Italian"),
        "pt" => Some("Portuguese"),
        _ => None,
    }
}

/// Bounded Unicode chunks, preferring sentence boundaries and then whitespace.
/// No text is dropped (including punctuation and long words).
pub(super) fn chunks(text: &str, limit: usize) -> Vec<String> {
    assert!(limit > 0);
    let mut rest = text.trim();
    let mut result = Vec::new();
    while !rest.is_empty() {
        let end = rest
            .char_indices()
            .nth(limit)
            .map(|(i, _)| i)
            .unwrap_or(rest.len());
        let prefix = &rest[..end];
        let split = if end == rest.len() {
            end
        } else {
            let sentence = prefix
                .char_indices()
                .filter(|(_, c)| ".!?。！？\n".contains(*c))
                .map(|(i, c)| i + c.len_utf8())
                .next_back();
            sentence
                .filter(|i| *i >= end / 3)
                .or_else(|| {
                    prefix
                        .char_indices()
                        .filter(|(_, c)| c.is_whitespace())
                        .map(|(i, c)| i + c.len_utf8())
                        .next_back()
                })
                .filter(|i| *i > 0)
                .unwrap_or(end)
        };
        let part = rest[..split].trim();
        if !part.is_empty() {
            result.push(part.to_owned());
        }
        rest = rest[split..].trim_start();
    }
    result
}

pub(super) fn snapshot(
    lease: &ProjectLease,
    args: &AudioStartArgs,
) -> Result<Input, crate::app::contracts::AppError> {
    if !VOICES.contains(&args.voice.as_str()) {
        return Err(failure("audioVoice"));
    }
    lease.with_connection(|db, _| {
        ProjectRepository::new(db, ProjectKind::Book)?;
        let tx = db.transaction().map_err(storage_error)?;
        let settings = shared::settings(&tx)?;
        let code = match args.text { AudioText::Original => settings.choices.source_language.as_deref(), AudioText::Translation => Some(settings.choices.target_language.as_str()) };
        let language = code.and_then(language).ok_or_else(|| failure("audioLanguage"))?.to_owned();
        let ids = {
            let mut query = tx.prepare("SELECT id FROM book_chapters ORDER BY position").map_err(storage_error)?;
            let rows = query.query_map([], |r| r.get::<_, String>(0)).map_err(storage_error)?;
            rows.collect::<Result<Vec<_>, _>>().map_err(storage_error)?
        };
        let mut chapters = Vec::new();
        for id in args.selection.resolve(&ids)? {
            let source_title: String = tx.query_row("SELECT source_title FROM book_chapters WHERE id=?1", [&id], |r| r.get(0)).map_err(storage_error)?;
            let translation: Option<(String, String)> = if args.text == AudioText::Translation {
                tx.query_row("SELECT id,translated_title FROM book_translations WHERE chapter_id=?1 AND target_language=?2 ORDER BY revision DESC LIMIT 1", rusqlite::params![id, settings.choices.target_language], |r| Ok((r.get(0)?,r.get(1)?))).optional().map_err(storage_error)?
            } else { None };
            let mut query = tx.prepare("SELECT b.text, t.translated_text FROM book_source_blocks b LEFT JOIN book_translation_blocks t ON t.source_block_id=b.id AND t.translation_id=?2 WHERE b.chapter_id=?1 AND b.kind IN ('text','caption') ORDER BY b.position").map_err(storage_error)?;
            let rows = query.query_map(rusqlite::params![id, translation.as_ref().map(|t| &t.0)], |r| Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?))).map_err(storage_error)?;
            let mut body = Vec::new();
            for row in rows {
                let (source, translated) = row.map_err(storage_error)?;
                if source.trim().is_empty() { continue; }
                let text = if args.text == AudioText::Original { source } else {
                    translated.filter(|s| !s.trim().is_empty()).ok_or_else(|| failure("audioIncomplete"))?
                };
                body.push(text);
            }
            if body.is_empty() { continue; }
            let title = if args.text == AudioText::Original { source_title } else {
                translation.map(|t| t.1).filter(|t| !t.trim().is_empty()).ok_or_else(|| failure("audioIncomplete"))?
            };
            let mut parts = chunks(&title, 400);
            for paragraph in body { parts.extend(chunks(&paragraph, 400)); }
            chapters.push(ChapterInput { title, chunks: parts });
        }
        if chapters.is_empty() { return Err(failure("audioEmpty")); }
        Ok(Input { version: 1, language, voice: args.voice.clone(), device: args.device.clone(), chapters })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn splits_unicode_without_losing_text() {
        for text in [
            "Привет! Это длинная глава. 你好，世界。 И конец!",
            "词".repeat(901).as_str(),
            "abcdefghijk",
        ] {
            let parts = chunks(text, 8);
            assert!(parts
                .iter()
                .all(|p| !p.is_empty() && p.chars().count() <= 8));
            assert_eq!(
                parts
                    .concat()
                    .chars()
                    .filter(|c| !c.is_whitespace())
                    .collect::<String>(),
                text.chars()
                    .filter(|c| !c.is_whitespace())
                    .collect::<String>()
            );
        }
        assert!(chunks(" \n ", 400).is_empty());
    }
    #[test]
    fn validates_languages() {
        assert_eq!(language("ru-RU"), Some("Russian"));
        assert_eq!(language("xx"), None);
    }
}
