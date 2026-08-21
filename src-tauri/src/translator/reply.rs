//! Parsing what the model sends back for one chapter.
//!
//! A chapter reply has to carry two things, the translated title and the body,
//! and the split between them used to be guessed: take the first line, strip
//! `#` and `*`, and hope the model put a title there. That guess is wrong in
//! both directions. A chapter whose first paragraph is short loses it to the
//! title, and a model that skips the title silently promotes real prose.
//!
//! So the reply is framed instead:
//!
//! ```text
//! <<<TITLE>>>
//! Глава 523. За гранью меча
//! <<<BODY>>>
//! Линь Хань шагнул вперёд.
//! ```
//!
//! **Why markers and not JSON.** [`crate::translator::DeepSeekClient::translate`]
//! survives the output token limit by continuing a reply that came back with
//! `finish_reason = length`. Truncated JSON cannot be parsed and continued JSON
//! cannot be rejoined, so JSON would turn a rare title-parsing slip into a hard
//! failure on exactly the long chapters that continuation exists to rescue. It
//! would also push the whole chapter through string escaping. Markers cost
//! nothing to escape and a continuation simply appends to the body.
//!
//! JSON is used where it is safe and worth it: the short, bounded replies of
//! term extraction and language repair, which cannot outgrow one response.

/// Marks the translated chapter title in a model reply.
pub const TITLE_MARK: &str = "<<<TITLE>>>";
/// Marks the start of the translated chapter body in a model reply.
pub const BODY_MARK: &str = "<<<BODY>>>";

/// The reply format instruction for a chunk that carries the chapter title.
pub fn envelope_instruction() -> String {
    format!(
        "Format your reply exactly like this, with both markers on their own lines:\n\
         {TITLE_MARK}\n\
         the translated chapter title on one line\n\
         {BODY_MARK}\n\
         the translated chapter text\n\
         Write nothing before, between or after the markers except that content. \
         If the input carries no chapter title, still emit {TITLE_MARK} and leave \
         its line empty."
    )
}

/// The reply format instruction for a continuation chunk, which has no title.
pub fn body_only_instruction() -> String {
    format!(
        "This is a continuation of a chapter already in progress, so it has no \
         title. Begin your reply with {BODY_MARK} on its own line, followed by \
         the translated text and nothing else."
    )
}

/// Parse a first-chunk reply into `(title, body)`.
///
/// Falls back to [`split_title_body`], the pre-envelope heuristic, when the
/// model ignored the format, so a non-conforming reply degrades to the old
/// behaviour instead of failing.
pub fn parse_chapter_reply(raw: &str, source_title: &str) -> (String, String) {
    let text = strip_code_fence(raw);
    let Some(body_at) = text.find(BODY_MARK) else {
        return split_title_body(text, source_title);
    };

    let head = &text[..body_at];
    let body = text[body_at + BODY_MARK.len()..].trim().to_string();

    let title = match head.find(TITLE_MARK) {
        Some(at) => head[at + TITLE_MARK.len()..].trim(),
        // No title marker but a body marker: anything ahead of the body is the
        // title the model wrote without labelling it.
        None => head.trim(),
    };
    let title = clean_title(title);

    // An empty body means the markers were decoration on an otherwise normal
    // reply; the heuristic will do better than handing back nothing.
    if body.is_empty() {
        return split_title_body(text, source_title);
    }

    let title = if title.is_empty() {
        source_title.trim().to_string()
    } else {
        title
    };
    (title, body)
}

/// Parse a continuation-chunk reply, which carries a body and no title.
pub fn parse_body_reply(raw: &str) -> String {
    let text = strip_code_fence(raw);
    match text.find(BODY_MARK) {
        Some(at) => text[at + BODY_MARK.len()..].trim().to_string(),
        None => text.trim().to_string(),
    }
}

/// Drop a wrapping ```code fence``` some models add around a whole reply.
fn strip_code_fence(raw: &str) -> &str {
    let t = raw.trim();
    let Some(rest) = t.strip_prefix("```") else {
        return t;
    };
    // Skip the language tag on the opening fence, then the closing fence.
    let after_tag = rest.find('\n').map(|i| &rest[i + 1..]).unwrap_or("");
    match after_tag.rfind("```") {
        Some(end) => after_tag[..end].trim(),
        None => after_tag.trim(),
    }
}

/// Pre-envelope heuristic: treat the first line as the title when a second
/// non-empty line follows, otherwise keep the source title and take everything
/// as body. Retained as the fallback for replies that ignore the markers.
pub fn split_title_body(full: &str, source_title: &str) -> (String, String) {
    if source_title.trim().is_empty() {
        return (String::new(), full.trim().to_string());
    }
    let trimmed = full.trim_start();
    match trimmed.split_once('\n') {
        Some((first, rest)) if !rest.trim().is_empty() => {
            (clean_title(first), rest.trim().to_string())
        }
        _ => (source_title.trim().to_string(), trimmed.trim().to_string()),
    }
}

/// Strip markdown heading marks and stray whitespace from a title line.
pub fn clean_title(line: &str) -> String {
    line.trim()
        .trim_start_matches(|c: char| c == '#' || c == '*' || c.is_whitespace())
        .trim()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_envelope() {
        let raw = "<<<TITLE>>>\nГлава 523. Меч\n<<<BODY>>>\nЛинь Хань шагнул вперёд.\n\nВетер стих.";
        let (title, body) = parse_chapter_reply(raw, "第523章");
        assert_eq!(title, "Глава 523. Меч");
        assert_eq!(body, "Линь Хань шагнул вперёд.\n\nВетер стих.");
    }

    /// The whole point: a short opening paragraph is no longer eaten as a title.
    #[test]
    fn keeps_a_short_first_paragraph_in_the_body() {
        let raw = "<<<TITLE>>>\nГлава 7\n<<<BODY>>>\nОн замер.\n\nПотом побежал.";
        let (title, body) = parse_chapter_reply(raw, "第7章");
        assert_eq!(title, "Глава 7");
        assert!(body.starts_with("Он замер."));
    }

    #[test]
    fn empty_title_line_falls_back_to_the_source_title() {
        let raw = "<<<TITLE>>>\n\n<<<BODY>>>\nТекст главы.";
        let (title, body) = parse_chapter_reply(raw, "第9章");
        assert_eq!(title, "第9章");
        assert_eq!(body, "Текст главы.");
    }

    #[test]
    fn unmarked_reply_falls_back_to_the_heuristic() {
        let (title, body) = parse_chapter_reply("Глава 1\n\nТекст главы.", "第1章");
        assert_eq!(title, "Глава 1");
        assert_eq!(body, "Текст главы.");
    }

    #[test]
    fn markers_without_a_body_fall_back_too() {
        let (title, body) = parse_chapter_reply("<<<BODY>>>\n", "第3章");
        assert_eq!(title, "第3章");
        assert_eq!(body, "<<<BODY>>>");
    }

    #[test]
    fn strips_a_wrapping_code_fence() {
        let raw = "```\n<<<TITLE>>>\nГлава 4\n<<<BODY>>>\nТекст.\n```";
        let (title, body) = parse_chapter_reply(raw, "第4章");
        assert_eq!(title, "Глава 4");
        assert_eq!(body, "Текст.");
    }

    #[test]
    fn strips_markdown_marks_from_the_title() {
        let raw = "<<<TITLE>>>\n### Глава 5\n<<<BODY>>>\nТекст.";
        let (title, _) = parse_chapter_reply(raw, "第5章");
        assert_eq!(title, "Глава 5");
    }

    #[test]
    fn body_reply_drops_its_marker() {
        assert_eq!(parse_body_reply("<<<BODY>>>\nПродолжение."), "Продолжение.");
        assert_eq!(parse_body_reply("  Продолжение.  "), "Продолжение.");
    }

    #[test]
    fn split_title_body_variants() {
        let (t, b) = split_title_body("Глава 1\n\nТекст главы.", "第1章");
        assert_eq!(t, "Глава 1");
        assert_eq!(b, "Текст главы.");
        let (t, _) = split_title_body("### Глава 3\n\nтекст", "第3章");
        assert_eq!(t, "Глава 3");
        let (t, b) = split_title_body("Просто текст.", "");
        assert_eq!(t, "");
        assert_eq!(b, "Просто текст.");
        let (t, b) = split_title_body("Одна строка", "第2章");
        assert_eq!(t, "第2章");
        assert_eq!(b, "Одна строка");
    }
}
