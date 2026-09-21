//! Export to PDF: genpdf handles text layout/pagination (embedded Cyrillic font,
//! readable spacing, cover, a contents page with page numbers), and lopdf adds
//! real **bookmarks** (a clickable outline) plus a correct Unicode document title.
//!
//! Chapter start pages are needed both for the contents' page numbers and for the
//! bookmarks. genpdf paginates internally, so the front matter and each chapter
//! are also rendered on their own to count pages — a chapter starts on a fresh
//! page, so its standalone page count equals the count inside the full document.
//! The contents lists one row per chapter, so its own length does not depend on
//! the page-number text (no circular dependency).

use std::io::Cursor;
use std::path::Path;

use anyhow::{anyhow, Result};
use genpdf::{elements, fonts, style, Alignment, Document, Element as _, Margins};

use super::{OutputMeta, TranslatedChapter};

/// Assemble the book into a `.pdf`.
pub fn export(chapters: &[TranslatedChapter], meta: &OutputMeta, out_path: &Path) -> Result<()> {
    let family = font_family()?;

    // --- pass 1: chapter start pages ---
    let placeholder = vec![0usize; chapters.len()];
    let front_pages = page_count(&render(family.clone(), |d| {
        push_front_matter(d, meta, chapters, &placeholder)
    })?)?;
    let mut starts: Vec<usize> = Vec::with_capacity(chapters.len());
    let mut cur = front_pages;
    for ch in chapters {
        starts.push(cur + 1); // 1-based page number of this chapter's first page
        cur += page_count(&render(family.clone(), |d| push_chapter(d, ch))?)?;
    }

    // --- pass 2: final document with page numbers in the contents ---
    let bytes = render(family.clone(), |d| {
        push_front_matter(d, meta, chapters, &starts);
        for ch in chapters {
            d.push(elements::PageBreak::new());
            push_chapter(d, ch);
        }
    })?;
    std::fs::write(out_path, &bytes).map_err(|e| anyhow!("writing {}: {e}", out_path.display()))?;

    // --- pass 3: bookmarks (outline) + Unicode title ---
    if let Err(e) = add_outline(out_path, meta, chapters, &starts) {
        tracing::warn!("PDF outline/metadata step failed: {e:#}");
    }
    Ok(())
}

fn font_family() -> Result<fonts::FontFamily<fonts::FontData>> {
    let regular =
        fonts::FontData::new(include_bytes!("../../assets/DejaVuSans.ttf").to_vec(), None)
            .map_err(|e| anyhow!("pdf font: {e}"))?;
    let bold = fonts::FontData::new(
        include_bytes!("../../assets/DejaVuSans-Bold.ttf").to_vec(),
        None,
    )
    .map_err(|e| anyhow!("pdf font: {e}"))?;
    Ok(fonts::FontFamily {
        regular: regular.clone(),
        bold: bold.clone(),
        italic: regular,
        bold_italic: bold,
    })
}

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
    doc.render(&mut buf)
        .map_err(|e| anyhow!("rendering PDF: {e}"))?;
    Ok(buf)
}

fn push_front_matter(
    doc: &mut Document,
    meta: &OutputMeta,
    chapters: &[TranslatedChapter],
    pages: &[usize],
) {
    // --- page 1: cover only, centered (genpdf embeds JPEG, so normalize to it) ---
    if let Some(cover) = &meta.cover {
        if let Some((jpeg, _w, h)) = cover_as_jpeg(&cover.base64) {
            const DPI: f64 = 150.0;
            // vertical center: top space = (usable height − cover height) / 2.
            let usable_mm = 297.0 - 2.0 * 18.0; // A4 minus margins
            let cover_mm = h as f64 / DPI * 25.4;
            let line_mm = 5.7; // ≈ default line height
            let top_lines = (((usable_mm - cover_mm) / 2.0).max(0.0) / line_mm).round();
            doc.push(elements::Break::new(top_lines));
            if let Ok(image) = elements::Image::from_reader(Cursor::new(jpeg)) {
                doc.push(image.with_alignment(Alignment::Center).with_dpi(DPI));
            }
            doc.push(elements::PageBreak::new());
        }
    }

    // --- page 2 (or 1 if no cover): title + author ---
    doc.push(elements::Break::new(12.0));
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

    // --- contents: "Title .......... page" ---
    doc.push(elements::PageBreak::new());
    doc.push(
        elements::Paragraph::new(crate::i18n::label(&meta.lang, "contents"))
            .styled(style::Style::new().bold().with_font_size(18)),
    );
    doc.push(elements::Break::new(0.6));

    let mut table = elements::TableLayout::new(vec![10, 1]);
    table.set_cell_decorator(elements::FrameCellDecorator::new(false, false, false));
    for (ch, page) in chapters.iter().zip(pages.iter()) {
        let title = ch.title.trim();
        if title.is_empty() {
            continue;
        }
        let _ = table
            .row()
            .element(elements::Paragraph::new(title).padded(Margins::trbl(0.5, 2.0, 0.5, 0.0)))
            .element(
                elements::Paragraph::new(page.to_string())
                    .aligned(Alignment::Right)
                    .padded(Margins::trbl(0.5, 0.0, 0.5, 0.0)),
            )
            .push();
    }
    doc.push(table);
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

/// Decode a base64 cover (any common format), re-encode it as JPEG bytes (genpdf
/// can only embed JPEG), and report its pixel dimensions.
fn cover_as_jpeg(base64: &str) -> Option<(Vec<u8>, u32, u32)> {
    use base64::Engine as _;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(base64.trim())
        .ok()?;
    let img = image::load_from_memory(&bytes).ok()?;
    let (w, h) = (img.width(), img.height());
    let rgb = image::DynamicImage::ImageRgb8(img.to_rgb8());
    let mut out = Vec::new();
    rgb.write_to(&mut Cursor::new(&mut out), image::ImageFormat::Jpeg)
        .ok()?;
    Some((out, w, h))
}

fn page_count(bytes: &[u8]) -> Result<usize> {
    Ok(lopdf::Document::load_mem(bytes)?.get_pages().len())
}

/// Add a clickable outline (bookmark per chapter, at its start page) + a Unicode
/// title to the saved file.
fn add_outline(
    path: &Path,
    meta: &OutputMeta,
    chapters: &[TranslatedChapter],
    starts: &[usize],
) -> Result<()> {
    use lopdf::{Bookmark, Document as LDoc, Object, StringFormat};

    let mut doc = LDoc::load(path)?;
    let pages: Vec<lopdf::ObjectId> = doc.get_pages().into_values().collect();

    for (ch, &start) in chapters.iter().zip(starts.iter()) {
        if let Some(&page_id) = pages.get(start.saturating_sub(1)) {
            let title = ch.title.trim();
            let name = if title.is_empty() {
                "—".to_string()
            } else {
                title.to_string()
            };
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

    let mut title_bytes = vec![0xFE, 0xFF];
    for u in meta.title.trim().encode_utf16() {
        title_bytes.extend_from_slice(&u.to_be_bytes());
    }
    let info_id = match doc
        .trailer
        .get(b"Info")
        .ok()
        .and_then(|o| o.as_reference().ok())
    {
        Some(id) => id,
        None => {
            let id = doc.add_object(Object::Dictionary(lopdf::Dictionary::new()));
            doc.trailer.set("Info", Object::Reference(id));
            id
        }
    };
    if let Ok(Object::Dictionary(info)) = doc.get_object_mut(info_id) {
        info.set(
            "Title",
            Object::String(title_bytes, StringFormat::Hexadecimal),
        );
    }

    doc.save(path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_pdf_with_bookmarks() {
        let chapters = vec![
            TranslatedChapter {
                index: 1,
                number: Some(1),
                title: "Глава 1. Начало".into(),
                body: "Первый абзац.\n\nВторой абзац.".into(),
            },
            TranslatedChapter {
                index: 2,
                number: Some(2),
                title: "Глава 2. Продолжение".into(),
                body: "Текст второй главы.".into(),
            },
        ];
        let meta = OutputMeta {
            title: "Книга".into(),
            author: "Автор".into(),
            ..Default::default()
        };
        let path = std::env::temp_dir().join(format!("bc_pdf_{}.pdf", std::process::id()));
        export(&chapters, &meta, &path).unwrap();
        let doc = lopdf::Document::load(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        let root = doc.trailer.get(b"Root").unwrap().as_reference().unwrap();
        let cat = doc.get_object(root).unwrap().as_dict().unwrap();
        assert!(cat.has(b"Outlines"), "expected an outline (bookmarks)");
    }
}
