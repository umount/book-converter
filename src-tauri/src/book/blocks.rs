//! Typed import blocks. Image placeholders occur only in the diagnostic text
//! projection; translation, editing and export operate on explicit blocks.

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
}

/// The in-text placeholder for an image block.
pub fn marker_for(asset_id: &str) -> String {
    format!("[[img:{asset_id}]]")
}

/// Text projection used during import; storage uses the original typed blocks.
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
    }
}
