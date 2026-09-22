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

    pub fn from_str(s: &str) -> BlockKind {
        match s {
            "image" => BlockKind::Image,
            "caption" => BlockKind::Caption,
            _ => BlockKind::Text,
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
    text.lines().filter_map(marker_id).collect()
}

/// The asset id when a line is nothing but an image marker.
fn marker_id(line: &str) -> Option<String> {
    let t = line.trim();
    let inner = t.strip_prefix("[[img:")?.strip_suffix("]]")?;
    let id = inner.trim();
    (!id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric())).then(|| id.to_string())
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

    /// A marker is only a marker on its own line: prose that merely mentions the
    /// syntax must not be swallowed.
    #[test]
    fn marker_must_be_the_whole_line() {
        assert!(markers_in("он сказал [[img:ab12]] вслух").is_empty());
        assert_eq!(strip_markers("он сказал [[img:ab12]] вслух").lines().count(), 1);
        assert!(markers_in("[[img:not an id]]").is_empty());
    }
}
