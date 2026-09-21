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
        "Format your reply exactly like this:\n\
         {TITLE_MARK}\n\
         the translated chapter title, on one line\n\
         {BODY_MARK}\n\
         the translated chapter text\n\
         Each marker appears exactly once, alone on its line. Put the title \
         between them and nothing else, and do not repeat the title at the start \
         of the text. Write nothing before the first marker or after the text."
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
/// Written to survive the ways a model actually gets the format wrong, because
/// it does so often enough to matter: about one chapter in six came back with a
/// repeated `<<<TITLE>>>` and the real title pushed into the body. So:
///
/// - marker text is stripped from both halves, never stored;
/// - a repeated title marker resolves to the content after the **last** one;
/// - a title that ends up as the body's first line is lifted back out, when it
///   can be recognised as this chapter's heading (`number` is the strong signal);
/// - a title duplicated in both places is removed from the body;
/// - anything still unusable falls back to [`split_title_body`], the
///   pre-envelope heuristic, and finally to the source title.
///
/// `number` is the book chapter number, when known.
pub fn parse_chapter_reply(
    raw: &str,
    source_title: &str,
    number: Option<usize>,
) -> (String, String) {
    let text = strip_code_fence(raw);
    let Some(body_at) = text.find(BODY_MARK) else {
        return split_title_body(&strip_markers(text), source_title);
    };

    let head = &text[..body_at];
    // A model that emits the title marker twice leaves the title, if it wrote
    // one at all, after the last of them.
    let title_part = match head.rfind(TITLE_MARK) {
        Some(at) => &head[at + TITLE_MARK.len()..],
        // No title marker but a body marker: anything ahead of the body is the
        // title the model wrote without labelling it.
        None => head,
    };
    let mut title = clean_title(&strip_markers(title_part));
    let mut body = strip_markers(&text[body_at + BODY_MARK.len()..])
        .trim()
        .to_string();

    // An empty body means the markers were decoration on an otherwise normal
    // reply; the heuristic will do better than handing back nothing.
    if body.is_empty() {
        return split_title_body(&strip_markers(text), source_title);
    }

    match body.split_once('\n') {
        // The title slot was empty and the heading went into the text instead.
        Some((first, rest)) if title.is_empty() && !rest.trim().is_empty() => {
            if is_heading(first, number) {
                title = clean_title(first);
                body = rest.trim().to_string();
            }
        }
        // The model wrote the title in both places; the body keeps the text only.
        Some((first, rest)) if !rest.trim().is_empty() && clean_title(first) == title => {
            body = rest.trim().to_string();
        }
        _ => {}
    }

    if title.is_empty() {
        title = source_title.trim().to_string();
    }
    (title, body)
}

/// Parse a continuation-chunk reply, which carries a body and no title.
pub fn parse_body_reply(raw: &str) -> String {
    let text = strip_code_fence(raw);
    let body = match text.find(BODY_MARK) {
        Some(at) => &text[at + BODY_MARK.len()..],
        None => text,
    };
    strip_markers(body).trim().to_string()
}

/// Remove every `<<<…>>>` marker token from a fragment.
///
/// Marker text must never reach stored output: as well as looking like
/// corruption, `TITLE` is a run of Latin letters, so the target-language check
/// reports it as a foreign word and spends a repair pass trying to translate it.
fn strip_markers(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(start) = rest.find("<<<") {
        match rest[start..].find(">>>") {
            Some(rel_end) => {
                out.push_str(&rest[..start]);
                rest = &rest[start + rel_end + 3..];
            }
            // An unterminated "<<<" is ordinary text, not a marker.
            None => break,
        }
    }
    out.push_str(rest);
    out
}

/// Whether a line is this chapter's heading rather than its first paragraph.
///
/// With a chapter number known, requiring it to appear is precise and works in
/// any language. Without one, this falls back to the shape of a heading: a
/// short single line. Only ever consulted when the title slot came back empty,
/// so the alternative is having no title at all.
fn is_heading(line: &str, number: Option<usize>) -> bool {
    let line = line.trim();
    if line.is_empty() || line.chars().count() > MAX_TITLE_CHARS {
        return false;
    }
    match number {
        Some(n) => line.contains(&n.to_string()),
        None => line.chars().count() <= 60,
    }
}

/// Longest line still plausible as a chapter title.
const MAX_TITLE_CHARS: usize = 120;

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
        let raw =
            "<<<TITLE>>>\nГлава 523. Меч\n<<<BODY>>>\nЛинь Хань шагнул вперёд.\n\nВетер стих.";
        let (title, body) = parse_chapter_reply(raw, "第523章", Some(523));
        assert_eq!(title, "Глава 523. Меч");
        assert_eq!(body, "Линь Хань шагнул вперёд.\n\nВетер стих.");
    }

    /// The whole point: a short opening paragraph is no longer eaten as a title.
    #[test]
    fn keeps_a_short_first_paragraph_in_the_body() {
        let raw = "<<<TITLE>>>\nГлава 7\n<<<BODY>>>\nОн замер.\n\nПотом побежал.";
        let (title, body) = parse_chapter_reply(raw, "第7章", Some(7));
        assert_eq!(title, "Глава 7");
        assert!(body.starts_with("Он замер."));
    }

    #[test]
    fn empty_title_line_falls_back_to_the_source_title() {
        let raw = "<<<TITLE>>>\n\n<<<BODY>>>\nТекст главы.";
        let (title, body) = parse_chapter_reply(raw, "第9章", Some(9));
        assert_eq!(title, "第9章");
        assert_eq!(body, "Текст главы.");
    }

    #[test]
    fn unmarked_reply_falls_back_to_the_heuristic() {
        let (title, body) = parse_chapter_reply("Глава 1\n\nТекст главы.", "第1章", Some(1));
        assert_eq!(title, "Глава 1");
        assert_eq!(body, "Текст главы.");
    }

    /// A reply that is nothing but a marker yields no text, and keeps the source
    /// title. It must never store the marker itself as the chapter's content.
    #[test]
    fn a_marker_only_reply_yields_nothing_usable() {
        let (title, body) = parse_chapter_reply("<<<BODY>>>\n", "第3章", Some(3));
        assert_eq!(title, "第3章");
        assert_eq!(body, "");
    }

    #[test]
    fn strips_a_wrapping_code_fence() {
        let raw = "```\n<<<TITLE>>>\nГлава 4\n<<<BODY>>>\nТекст.\n```";
        let (title, body) = parse_chapter_reply(raw, "第4章", Some(4));
        assert_eq!(title, "Глава 4");
        assert_eq!(body, "Текст.");
    }

    #[test]
    fn strips_markdown_marks_from_the_title() {
        let raw = "<<<TITLE>>>\n### Глава 5\n<<<BODY>>>\nТекст.";
        let (title, _) = parse_chapter_reply(raw, "第5章", Some(5));
        assert_eq!(title, "Глава 5");
    }

    /// The exact failure seen in production: the model emitted the title marker
    /// twice, left the slot empty, and put the heading at the top of the text.
    /// It stored a chapter titled literally "<<<TITLE>>>".
    #[test]
    fn recovers_a_title_the_model_pushed_into_the_body() {
        let raw = "<<<TITLE>>>\n<<<TITLE>>>\n<<<BODY>>>\n\
                   Глава 1301. Десятый Предел!\n\n\
                   Путь постижения Десятого Предела был труден.";
        let (title, body) = parse_chapter_reply(raw, "第1301章 第十极！", Some(1301));
        assert_eq!(title, "Глава 1301. Десятый Предел!");
        assert_eq!(body, "Путь постижения Десятого Предела был труден.");
    }

    /// Marker text must never survive into what gets stored: `TITLE` is Latin,
    /// so the target-language check would report it as a foreign word and burn
    /// a repair pass on it (which is what happened).
    #[test]
    fn marker_text_never_reaches_the_output() {
        let raw = "<<<TITLE>>>\n<<<TITLE>>>\n<<<BODY>>>\nГлава 9. Меч\n\nТекст.";
        let (title, body) = parse_chapter_reply(raw, "第9章", Some(9));
        assert!(
            !title.contains("<<<") && !title.contains("TITLE"),
            "{title:?}"
        );
        assert!(!body.contains("<<<") && !body.contains("TITLE"), "{body:?}");
    }

    /// A repeated marker with the title after the last one still parses.
    #[test]
    fn repeated_title_marker_takes_the_last() {
        let raw = "<<<TITLE>>>\n<<<TITLE>>>\nГлава 42. Путь\n<<<BODY>>>\nТекст главы.";
        let (title, body) = parse_chapter_reply(raw, "第42章", Some(42));
        assert_eq!(title, "Глава 42. Путь");
        assert_eq!(body, "Текст главы.");
    }

    /// The title written in both places is kept once, not duplicated.
    #[test]
    fn a_title_repeated_in_the_body_is_dropped_there() {
        let raw = "<<<TITLE>>>\nГлава 8. Ветер\n<<<BODY>>>\nГлава 8. Ветер\n\nТекст главы.";
        let (title, body) = parse_chapter_reply(raw, "第8章", Some(8));
        assert_eq!(title, "Глава 8. Ветер");
        assert_eq!(body, "Текст главы.");
    }

    /// Without the title slot filled, an ordinary opening paragraph must NOT be
    /// promoted: that is the mistake the envelope exists to prevent.
    #[test]
    fn an_opening_paragraph_is_not_mistaken_for_a_heading() {
        let raw = "<<<TITLE>>>\n<<<BODY>>>\nОн замер.\n\nПотом побежал.";
        let (title, body) = parse_chapter_reply(raw, "第7章", Some(7));
        assert_eq!(title, "第7章", "the source title is the safe fallback");
        assert_eq!(body, "Он замер.\n\nПотом побежал.");
    }

    /// A heading is recognised by carrying the chapter number, in any language.
    #[test]
    fn heading_recognition_needs_the_number() {
        assert!(is_heading("Глава 1301. Десятый Предел!", Some(1301)));
        assert!(is_heading("第1301章 第十极！", Some(1301)));
        assert!(!is_heading("Глава 1301. Десятый Предел!", Some(77)));
        assert!(!is_heading("Он замер.", Some(1301)));
        // Too long to be a title even with the number in it.
        assert!(!is_heading(
            &format!("1301 {}", "я".repeat(200)),
            Some(1301)
        ));
    }

    #[test]
    fn strip_markers_leaves_ordinary_text_alone() {
        assert_eq!(strip_markers("Текст <<<BODY>>> ещё"), "Текст  ещё");
        assert_eq!(
            strip_markers("Он сказал: <<< это не маркер"),
            "Он сказал: <<< это не маркер"
        );
        assert_eq!(strip_markers("чистый текст"), "чистый текст");
    }

    #[test]
    fn body_reply_drops_its_marker() {
        assert_eq!(parse_body_reply("<<<BODY>>>\nПродолжение."), "Продолжение.");
        assert_eq!(parse_body_reply("  Продолжение.  "), "Продолжение.");
        // A continuation chunk that leaks the title marker too.
        assert_eq!(
            parse_body_reply("<<<TITLE>>>\n<<<BODY>>>\nПродолжение."),
            "Продолжение."
        );
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
