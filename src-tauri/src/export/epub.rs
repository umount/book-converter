//! Export to EPUB: one XHTML document per chapter + a table of contents.

use std::path::Path;

use anyhow::{Context, Result};
use epub_builder::{EpubBuilder, EpubContent, ReferenceType, ZipLibrary};

use super::{OutputMeta, TranslatedChapter};

/// Assemble the book into an `.epub` with a per-chapter TOC.
pub fn export(chapters: &[TranslatedChapter], meta: &OutputMeta, out_path: &Path) -> Result<()> {
    let zip = ZipLibrary::new().map_err(|e| anyhow::anyhow!("epub zip backend: {e}"))?;
    let mut builder = EpubBuilder::new(zip).map_err(|e| anyhow::anyhow!("epub builder: {e}"))?;

    builder
        .metadata("title", &meta.title)
        .and_then(|b| b.metadata("author", &meta.author))
        .and_then(|b| b.metadata("lang", &meta.lang))
        .map_err(|e| anyhow::anyhow!("epub metadata: {e}"))?;
    builder.inline_toc();

    for ch in chapters {
        let xhtml = chapter_xhtml(ch);
        let title = ch.title.trim();
        builder
            .add_content(
                EpubContent::new(format!("chapter_{}.xhtml", ch.index), xhtml.as_bytes())
                    .title(title)
                    .reftype(ReferenceType::Text),
            )
            .map_err(|e| anyhow::anyhow!("epub add chapter {}: {e}", ch.index))?;
    }

    let mut file = std::fs::File::create(out_path)
        .with_context(|| format!("creating {}", out_path.display()))?;
    builder
        .generate(&mut file)
        .map_err(|e| anyhow::anyhow!("epub generate: {e}"))?;
    Ok(())
}

/// Render one chapter as a standalone XHTML document.
fn chapter_xhtml(ch: &TranslatedChapter) -> String {
    let mut body = String::new();
    let title = ch.title.trim();
    if !title.is_empty() {
        body.push_str(&format!("<h1>{}</h1>\n", esc(title)));
    }
    for para in ch.body.lines().map(str::trim).filter(|l| !l.is_empty()) {
        body.push_str(&format!("<p>{}</p>\n", esc(para)));
    }
    format!(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n\
         <!DOCTYPE html>\n\
         <html xmlns=\"http://www.w3.org/1999/xhtml\">\n\
         <head><title>{}</title></head>\n<body>\n{}</body>\n</html>\n",
        esc(title),
        body,
    )
}

/// Escape XML/XHTML text content.
fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}
