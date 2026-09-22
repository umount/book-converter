//! Typed chapter content: a chapter is an ordered list of blocks, not a string.
//!
//! An EPUB page can be a whole image (manga, illustrated editions, comic-style
//! web novels) or prose with illustrations in between. The rest of the pipeline
//! — chunker, prompts, glossary, search, find/replace, export — works on the
//! chapter's text, so blocks are kept as the **truth** while `Chapter::body`
//! stays a derived projection of them: text runs verbatim, images as a
//! `[[img:<asset_id>]]` line holding their position.
//!
//! The marker is a literal, which is what makes it survive translation, the
//! reader's autosave, book-wide replace and glossary retargeting untouched.

/// What one block of a chapter holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockKind {
    /// A run of prose (paragraphs separated by newlines).
    Text,
    /// A single image, referenced by asset id.
    Image,
    /// A caption belonging to the image above it (`<figcaption>`).
    Caption,
}

impl BlockKind {
    pub fn as_str(self) -> &'static str {
        match self {
            BlockKind::Text => "text",
            BlockKind::Image => "image",
            BlockKind::Caption => "caption",
        }
    }

}

/// One block of a chapter's original content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {
    pub kind: BlockKind,
    /// Source text for `Text` / `Caption`; empty for `Image`.
    pub text: String,
    /// Asset id for `Image`; `None` otherwise.
    pub asset_id: Option<String>,
}

impl Block {
    pub fn text(text: impl Into<String>) -> Block {
        Block {
            kind: BlockKind::Text,
            text: text.into(),
            asset_id: None,
        }
    }

    pub fn caption(text: impl Into<String>) -> Block {
        Block {
            kind: BlockKind::Caption,
            text: text.into(),
            asset_id: None,
        }
    }

    pub fn image(asset_id: impl Into<String>) -> Block {
        Block {
            kind: BlockKind::Image,
            text: String::new(),
            asset_id: Some(asset_id.into()),
        }
    }
}

/// Blocks belonging to one chapter, keyed by its reading-order index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChapterBlocks {
    pub chapter_index: usize,
    pub kind: ChapterKind,
    pub blocks: Vec<Block>,
}

/// An image the source file carries, before it is copied into the project.
///
/// `id` is the content hash, so two spine pages pointing at the same file (or two
/// identical files under different names) collapse into one asset.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssetRef {
    pub id: String,
    /// Where the bytes live inside the source container (a zip entry name).
    pub href: String,
    pub content_type: String,
    pub bytes: u64,
}

/// What a chapter is made of, as one value the UI and the translation queue can
/// branch on without joining the blocks table.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ChapterKind {
    /// Prose only — everything the text pipeline was built for.
    #[default]
    Text,
    /// Images only: nothing for a text translator to do (a manga page).
    Image,
    /// Prose with images in between.
    Mixed,
    /// Neither: a page that parsed to nothing.
    Empty,
}

impl ChapterKind {
    /// Classify a chapter by what its blocks actually hold.
    pub fn classify(blocks: &[Block]) -> ChapterKind {
        let has_image = blocks.iter().any(|b| b.kind == BlockKind::Image);
        let has_text = blocks
            .iter()
            .any(|b| b.kind != BlockKind::Image && !b.text.trim().is_empty());
        match (has_text, has_image) {
            (true, true) => ChapterKind::Mixed,
            (true, false) => ChapterKind::Text,
            (false, true) => ChapterKind::Image,
            (false, false) => ChapterKind::Empty,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            ChapterKind::Text => "text",
            ChapterKind::Image => "image",
            ChapterKind::Mixed => "mixed",
            ChapterKind::Empty => "empty",
        }
    }

    pub fn from_str(s: &str) -> ChapterKind {
        match s {
            "image" => ChapterKind::Image,
            "mixed" => ChapterKind::Mixed,
            "empty" => ChapterKind::Empty,
            _ => ChapterKind::Text,
        }
    }

    /// Whether the text translation pipeline has anything to work with.
    pub fn is_translatable(self) -> bool {
        matches!(self, ChapterKind::Text | ChapterKind::Mixed)
    }
}

/// The in-text placeholder for an image block.
pub fn marker_for(asset_id: &str) -> String {
    format!("[[img:{asset_id}]]")
}

/// Asset ids of the image markers in `text`, in order of appearance.
pub fn markers_in(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(marker_id)
        .map(str::to_string)
        .collect()
}

/// The asset id when a line is nothing but an image marker.
pub fn marker_id(line: &str) -> Option<&str> {
    let inner = line.trim().strip_prefix("[[img:")?.strip_suffix("]]")?;
    let id = inner.trim();
    (!id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric())).then_some(id)
}

/// Drop image markers from text, for consumers that only want prose (export,
/// the running summary, the source sample used for language detection).
pub fn strip_markers(text: &str) -> String {
    let kept: Vec<&str> = text
        .lines()
        .filter(|line| marker_id(line).is_none())
        .collect();
    collapse_blank_lines(&kept.join("\n"))
}

/// Put back the image markers a translation lost.
///
/// The model is told to copy the marker lines through, and usually does — but a
/// dropped one would lose a picture from the chapter for good, so the result is
/// checked rather than trusted. A marker the translation never returned is
/// re-inserted at the same relative position among the paragraphs it had in the
/// source; one the model invented or repeated is dropped.
///
/// Text with no markers in its source is returned untouched.
pub fn restore_markers(source: &str, translated: &str) -> String {
    let expected = markers_in(source);
    if expected.is_empty() {
        return translated.to_string();
    }

    // Where each marker sits among the source's paragraphs.
    let mut want: Vec<(String, usize)> = Vec::new();
    let mut source_paragraphs = 0usize;
    for line in source.lines() {
        if line.trim().is_empty() {
            continue;
        }
        match marker_id(line) {
            Some(id) => want.push((id.to_string(), source_paragraphs)),
            None => source_paragraphs += 1,
        }
    }

    // Keep each expected marker once; drop anything the model made up.
    let mut kept: Vec<String> = Vec::new();
    let mut present: Vec<String> = Vec::new();
    for line in translated.lines() {
        match marker_id(line).map(str::to_string) {
            Some(id) if expected.contains(&id) && !present.contains(&id) => {
                kept.push(marker_for(&id));
                present.push(id);
            }
            Some(_) => {}
            None => kept.push(line.to_string()),
        }
    }
    let missing: Vec<(String, usize)> = want
        .into_iter()
        .filter(|(id, _)| !present.contains(id))
        .collect();
    if missing.is_empty() && kept.len() == translated.lines().count() {
        return translated.to_string();
    }

    let is_prose = |line: &str| !line.trim().is_empty() && marker_id(line).is_none();
    let target_paragraphs = kept.iter().filter(|l| is_prose(l)).count();
    let scale = |at: usize| {
        if source_paragraphs == 0 {
            return 0;
        }
        (at * target_paragraphs + source_paragraphs / 2) / source_paragraphs
    };

    // A marker stands on its own line, with a blank line on either side.
    let insert = |out: &mut Vec<String>, id: &str| {
        if out.last().is_some_and(|line| !line.trim().is_empty()) {
            out.push(String::new());
        }
        out.push(marker_for(id));
        out.push(String::new());
    };

    let mut out: Vec<String> = Vec::new();
    let mut seen = 0usize;
    let mut pending = missing.into_iter().peekable();
    for line in kept {
        while pending.peek().is_some_and(|(_, at)| seen >= scale(*at)) {
            let (id, _) = pending.next().expect("peeked");
            insert(&mut out, &id);
        }
        if is_prose(&line) {
            seen += 1;
        }
        out.push(line);
    }
    for (id, _) in pending {
        insert(&mut out, &id);
    }
    collapse_blank_lines(&out.join("\n"))
}

/// Chapter text built from blocks: prose verbatim, images as marker lines.
pub fn derive_text(blocks: &[Block]) -> String {
    let mut parts: Vec<String> = Vec::new();
    for block in blocks {
        match block.kind {
            BlockKind::Image => {
                if let Some(id) = &block.asset_id {
                    parts.push(marker_for(id));
                }
            }
            _ => {
                let text = block.text.trim();
                if !text.is_empty() {
                    parts.push(text.to_string());
                }
            }
        }
    }
    parts.join("\n\n")
}

/// Squeeze runs of blank lines down to one, and trim the ends.
fn collapse_blank_lines(s: &str) -> String {
    let mut out = String::new();
    let mut blank = false;
    for line in s.lines() {
        if line.trim().is_empty() {
            blank = true;
            continue;
        }
        if !out.is_empty() {
            out.push('\n');
            if blank {
                out.push('\n');
            }
        }
        blank = false;
        out.push_str(line);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_covers_every_shape() {
        assert_eq!(
            ChapterKind::classify(&[Block::text("prose")]),
            ChapterKind::Text
        );
        assert_eq!(
            ChapterKind::classify(&[Block::image("a1")]),
            ChapterKind::Image
        );
        assert_eq!(
            ChapterKind::classify(&[Block::text("prose"), Block::image("a1")]),
            ChapterKind::Mixed
        );
        assert_eq!(ChapterKind::classify(&[]), ChapterKind::Empty);
        // A caption alone is still text the translator can work on.
        assert_eq!(
            ChapterKind::classify(&[Block::caption("Fig. 1")]),
            ChapterKind::Text
        );
        // Blank prose does not make a page of images translatable.
        assert_eq!(
            ChapterKind::classify(&[Block::text("   "), Block::image("a1")]),
            ChapterKind::Image
        );
    }

    /// A text-only chapter must derive to exactly its prose: no markers, no
    /// reflowing, so nothing changes for the books that work today.
    #[test]
    fn derive_text_of_prose_is_the_prose() {
        let blocks = vec![Block::text("Первый абзац.\n\nВторой абзац.")];
        assert_eq!(derive_text(&blocks), "Первый абзац.\n\nВторой абзац.");
    }

    #[test]
    fn derive_text_places_markers_where_the_images_are() {
        let blocks = vec![
            Block::text("До картинки."),
            Block::image("ab12"),
            Block::caption("Подпись."),
            Block::text("После."),
        ];
        assert_eq!(
            derive_text(&blocks),
            "До картинки.\n\n[[img:ab12]]\n\nПодпись.\n\nПосле."
        );
        assert_eq!(markers_in(&derive_text(&blocks)), vec!["ab12"]);
    }

    #[test]
    fn strip_markers_leaves_prose_readable() {
        let text = "До картинки.\n\n[[img:ab12]]\n\nПосле.";
        assert_eq!(strip_markers(text), "До картинки.\n\nПосле.");
        // Nothing to strip is a no-op.
        assert_eq!(strip_markers("Просто текст."), "Просто текст.");
    }

    /// The usual case: the model copied the markers through, so nothing is
    /// touched — not even whitespace.
    #[test]
    fn a_translation_that_kept_its_markers_is_left_alone() {
        let source = "До картинки.\n\n[[img:ab12]]\n\nПосле.";
        let translated = "Before.\n\n[[img:ab12]]\n\nAfter.";
        assert_eq!(restore_markers(source, translated), translated);
        // A chapter that never had a picture is never rewritten.
        assert_eq!(restore_markers("plain", "обычный текст"), "обычный текст");
    }

    #[test]
    fn a_dropped_marker_comes_back_where_it_belongs() {
        let source = "Первый.\n\n[[img:ab12]]\n\nВторой.\n\nТретий.";
        let out = restore_markers(source, "One.\n\nTwo.\n\nThree.");
        assert_eq!(out, "One.\n\n[[img:ab12]]\n\nTwo.\n\nThree.");
        assert_eq!(markers_in(&out), vec!["ab12"]);
    }

    /// A page whose picture opens or closes the chapter keeps it there.
    #[test]
    fn markers_at_the_edges_stay_at_the_edges() {
        assert_eq!(
            restore_markers("[[img:aa]]\n\nПролог.", "Prologue."),
            "[[img:aa]]\n\nPrologue."
        );
        assert_eq!(
            restore_markers("Конец.\n\n[[img:bb]]", "The end."),
            "The end.\n\n[[img:bb]]"
        );
    }

    /// Whatever the model does with them, every picture ends up in the text
    /// exactly once and in source order.
    #[test]
    fn invented_and_repeated_markers_are_dropped() {
        let source = "A.\n\n[[img:aa]]\n\nB.\n\n[[img:bb]]\n\nC.";
        let out = restore_markers(source, "A.\n\n[[img:aa]]\n\n[[img:aa]]\n\nB.\n\n[[img:zz]]\n\nC.");
        assert_eq!(markers_in(&out), vec!["aa", "bb"]);
        assert!(!out.contains("zz"));
    }

    /// A marker is only a marker on its own line: prose that merely mentions the
    /// syntax must not be swallowed.
    #[test]
    fn marker_must_be_the_whole_line() {
        assert!(markers_in("он сказал [[img:ab12]] вслух").is_empty());
        assert_eq!(strip_markers("он сказал [[img:ab12]] вслух").lines().count(), 1);
        assert!(markers_in("[[img:not an id]]").is_empty());
    }
}
