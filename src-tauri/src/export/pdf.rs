//! Export to PDF: genpdf handles text layout/pagination (embedded Cyrillic font,
//! readable spacing, cover, contents), and lopdf adds real **bookmarks** (a
//! clickable outline) plus a correct Unicode document title.
//!
//! Bookmarks need each chapter's start page. genpdf paginates internally, so we
//! render the front matter and each chapter both into the final document and
//! (once more) on their own just to count pages — chapters start on a fresh page,
//! so the standalone page count equals the count inside the full document.

use std::io::Cursor;
use std::path::Path;

use anyhow::{anyhow, Result};
use genpdf::{elements, fonts, style, Alignment, Document, Element as _, Margins, Scale};

use super::{OutputMeta, TranslatedChapter};

/// Assemble the book into a `.pdf`.
pub fn export(chapters: &[TranslatedChapter], meta: &OutputMeta, out_path: &Path) -> Result<()> {
    let family = font_family()?;

    // Final document: front matter + every chapter.
    let bytes = render(family.clone(), |doc| {
        push_front_matter(doc, meta, chapters);
        for ch in chapters {
            doc.push(elements::PageBreak::new());
            push_chapter(doc, ch);
        }
    })?;
    std::fs::write(out_path, &bytes).map_err(|e| anyhow!("writing {}: {e}", out_path.display()))?;

    // Compute each chapter's start page (page counts of the same content rendered
    // alone), then add bookmarks + a proper Unicode title.
    if let Err(e) = add_outline(out_path, meta, chapters, &family) {
        tracing::warn!("PDF outline/metadata step failed: {e:#}");
    }
    Ok(())
}

fn font_family() -> Result<fonts::FontFamily<fonts::FontData>> {
    let regular = fonts::FontData::new(include_bytes!("../../assets/DejaVuSans.ttf").to_vec(), None)
        .map_err(|e| anyhow!("pdf font: {e}"))?;
    let bold = fonts::FontData::new(include_bytes!("../../assets/DejaVuSans-Bold.ttf").to_vec(), None)
        .map_err(|e| anyhow!("pdf font: {e}"))?;
    Ok(fonts::FontFamily {
        regular: regular.clone(),
        bold: bold.clone(),
        italic: regular,
        bold_italic: bold,
    })
}

/// Build a genpdf document with `fill` and render it to bytes.
fn render(
    family: fonts::FontFamily<fonts::FontData>,
    fill: impl FnOnce(&mut Document),
) -> Result<Vec<u8>> {
    let mut doc = Document::new(family);
    doc.set_line_spacing(1.35);
    doc.set_minimal_conformance();
    let mut decorator = genpdf::SimplePageDecorator::new();
    decorator.set_margins(18);
    doc.set_page_decorator(decorator);
    fill(&mut doc);
    let mut buf = Vec::new();
    doc.render(&mut buf).map_err(|e| anyhow!("rendering PDF: {e}"))?;
    Ok(buf)
}

fn push_front_matter(doc: &mut Document, meta: &OutputMeta, chapters: &[TranslatedChapter]) {
    if let Some(cover) = &meta.cover {
        use base64::Engine as _;
        if let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(cover.base64.trim()) {
            if let Ok(image) = elements::Image::from_reader(Cursor::new(bytes)) {
                doc.push(image.with_alignment(Alignment::Center).with_scale(Scale::new(1.0, 1.0)).with_dpi(150.0));
            }
        }
    }
    doc.push(elements::Break::new(1.0));
    doc.push(
        elements::Paragraph::new(meta.title.trim())
            .aligned(Alignment::Center)
            .styled(style::Style::new().bold().with_font_size(24)),
    );
    if !meta.author.trim().is_empty() {
        doc.push(
            elements::Paragraph::new(meta.author.trim())
                .aligned(Alignment::Center)
                .styled(style::Style::new().with_font_size(13)),
        );
    }

    doc.push(elements::PageBreak::new());
    doc.push(
        elements::Paragraph::new(contents_label(&meta.lang))
            .styled(style::Style::new().bold().with_font_size(18)),
    );
    doc.push(elements::Break::new(0.6));
    for ch in chapters {
        let title = ch.title.trim();
        if !title.is_empty() {
            doc.push(elements::Paragraph::new(title).padded(Margins::trbl(0.0, 0.0, 1.2, 0.0)));
        }
    }
}

fn push_chapter(doc: &mut Document, ch: &TranslatedChapter) {
    let title = ch.title.trim();
    if !title.is_empty() {
        doc.push(
            elements::Paragraph::new(title)
                .styled(style::Style::new().bold().with_font_size(16))
                .padded(Margins::trbl(0.0, 0.0, 4.0, 0.0)),
        );
    }
    for para in ch.body.lines().map(str::trim).filter(|l| !l.is_empty()) {
        doc.push(elements::Paragraph::new(para).padded(Margins::trbl(0.0, 0.0, 2.2, 0.0)));
    }
}

/// Count pages of a rendered PDF.
fn page_count(bytes: &[u8]) -> Result<usize> {
    Ok(lopdf::Document::load_mem(bytes)?.get_pages().len())
}

/// Add a clickable outline (bookmarks per chapter) + Unicode title to the file.
fn add_outline(
    path: &Path,
    meta: &OutputMeta,
    chapters: &[TranslatedChapter],
    family: &fonts::FontFamily<fonts::FontData>,
) -> Result<()> {
    use lopdf::{Bookmark, Document as LDoc, Object, StringFormat};

    // Front-matter page count.
    let front_bytes = render(family.clone(), |doc| push_front_matter(doc, meta, chapters))?;
    let mut start = page_count(&front_bytes)?; // pages before chapter 1 (0-based index of ch.1 page)

    // (chapter title, 1-based start page).
    let mut starts: Vec<(String, usize)> = Vec::new();
    for ch in chapters {
        starts.push((ch.title.trim().to_string(), start + 1));
        let ch_bytes = render(family.clone(), |doc| push_chapter(doc, ch))?;
        start += page_count(&ch_bytes)?;
    }

    let mut doc = LDoc::load(path)?;
    let pages: Vec<lopdf::ObjectId> = doc.get_pages().into_values().collect();

    for (title, page_no) in &starts {
        if let Some(&page_id) = pages.get(page_no.saturating_sub(1)) {
            let name = if title.is_empty() { "—".to_string() } else { title.clone() };
            doc.add_bookmark(Bookmark::new(name, [0.0, 0.0, 0.0], 0, page_id), None);
        }
    }
    if let Some(outline_id) = doc.build_outline() {
        if let Ok(root_id) = doc.trailer.get(b"Root").and_then(|o| o.as_reference()) {
            if let Ok(Object::Dictionary(cat)) = doc.get_object_mut(root_id) {
                cat.set("Outlines", Object::Reference(outline_id));
            }
        }
    }

    // Unicode (UTF-16BE) title metadata.
    let mut title_bytes = vec![0xFE, 0xFF];
    for u in meta.title.trim().encode_utf16() {
        title_bytes.extend_from_slice(&u.to_be_bytes());
    }
    let info_id = match doc.trailer.get(b"Info").ok().and_then(|o| o.as_reference().ok()) {
        Some(id) => id,
        None => {
            let id = doc.add_object(Object::Dictionary(lopdf::Dictionary::new()));
            doc.trailer.set("Info", Object::Reference(id));
            id
        }
    };
    if let Ok(Object::Dictionary(info)) = doc.get_object_mut(info_id) {
        info.set("Title", Object::String(title_bytes, StringFormat::Hexadecimal));
    }

    doc.save(path)?;
    Ok(())
}

/// Localized "Contents" heading for the few languages we target.
fn contents_label(lang: &str) -> &'static str {
    if lang.to_lowercase().starts_with("ru") {
        "Содержание"
    } else {
        "Contents"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_pdf_with_bookmarks() {
        let chapters = vec![
            TranslatedChapter { index: 1, number: Some(1), title: "Глава 1. Начало".into(), body: "Первый абзац.\n\nВторой абзац.".into() },
            TranslatedChapter { index: 2, number: Some(2), title: "Глава 2. Продолжение".into(), body: "Текст второй главы.".into() },
        ];
        let meta = OutputMeta { title: "Книга".into(), author: "Автор".into(), ..Default::default() };
        let path = std::env::temp_dir().join(format!("bc_pdf_{}.pdf", std::process::id()));
        export(&chapters, &meta, &path).unwrap();
        let doc = lopdf::Document::load(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        // outline present
        let root = doc.trailer.get(b"Root").unwrap().as_reference().unwrap();
        let cat = doc.get_object(root).unwrap().as_dict().unwrap();
        assert!(cat.has(b"Outlines"), "expected an outline (bookmarks)");
    }
}
