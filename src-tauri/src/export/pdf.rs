//! Export to PDF via `genpdf`, with an embedded Unicode font (Cyrillic-capable),
//! readable spacing, a contents page, and a cover.
//!
//! Note: `genpdf 0.2` pins `printpdf 0.3`, which does not write the PDF Info
//! `Title` in Unicode — a Cyrillic title there shows as mojibake — so the document
//! title metadata is left unset and the title is rendered on the title page only.

use std::io::Cursor;
use std::path::Path;

use anyhow::{anyhow, Result};
use genpdf::{elements, fonts, style, Alignment, Document, Element as _, Margins, Scale, SimplePageDecorator};

use super::{OutputMeta, TranslatedChapter};

/// Assemble the book into a `.pdf`.
pub fn export(chapters: &[TranslatedChapter], meta: &OutputMeta, out_path: &Path) -> Result<()> {
    let regular = fonts::FontData::new(include_bytes!("../../assets/DejaVuSans.ttf").to_vec(), None)
        .map_err(|e| anyhow!("pdf font: {e}"))?;
    let bold = fonts::FontData::new(include_bytes!("../../assets/DejaVuSans-Bold.ttf").to_vec(), None)
        .map_err(|e| anyhow!("pdf font: {e}"))?;
    let family = fonts::FontFamily {
        regular: regular.clone(),
        bold: bold.clone(),
        italic: regular,
        bold_italic: bold,
    };

    let mut doc = Document::new(family);
    doc.set_line_spacing(1.35);
    doc.set_minimal_conformance();
    let mut decorator = SimplePageDecorator::new();
    decorator.set_margins(18);
    doc.set_page_decorator(decorator);

    // --- title page (cover + title + author) ---
    if let Some(cover) = &meta.cover {
        use base64::Engine as _;
        if let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(cover.base64.trim()) {
            if let Ok(image) = elements::Image::from_reader(Cursor::new(bytes)) {
                doc.push(
                    image
                        .with_alignment(Alignment::Center)
                        .with_scale(Scale::new(0.32, 0.32)),
                );
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

    // --- contents ---
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

    // --- body ---
    for ch in chapters {
        doc.push(elements::PageBreak::new());
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

    doc.render_to_file(out_path)
        .map_err(|e| anyhow!("rendering PDF: {e}"))?;

    // genpdf/printpdf 0.3 write the Info Title as non-Unicode; patch it to a
    // proper UTF-16BE string so the viewer's window title is correct.
    if let Err(e) = set_pdf_title(out_path, meta.title.trim()) {
        tracing::warn!("could not set PDF title metadata: {e:#}");
    }
    Ok(())
}

/// Set the PDF Info `/Title` to a Unicode (UTF-16BE) string.
fn set_pdf_title(path: &Path, title: &str) -> Result<()> {
    use lopdf::{Dictionary, Document, Object, StringFormat};

    let mut doc = Document::load(path)?;

    let mut bytes = vec![0xFE, 0xFF]; // UTF-16BE BOM
    for unit in title.encode_utf16() {
        bytes.extend_from_slice(&unit.to_be_bytes());
    }
    let title_obj = Object::String(bytes, StringFormat::Hexadecimal);

    let info_id = match doc.trailer.get(b"Info").ok().and_then(|o| o.as_reference().ok()) {
        Some(id) => id,
        None => {
            let id = doc.add_object(Object::Dictionary(Dictionary::new()));
            doc.trailer.set("Info", Object::Reference(id));
            id
        }
    };
    if let Ok(Object::Dictionary(dict)) = doc.get_object_mut(info_id) {
        dict.set("Title", title_obj);
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
    fn renders_a_valid_pdf() {
        let chapters = vec![TranslatedChapter {
            index: 1,
            number: Some(1),
            title: "Глава 1".into(),
            body: "Первый абзац на русском.\n\nВторой абзац.".into(),
        }];
        let meta = OutputMeta { title: "Книга".into(), author: "Автор".into(), ..Default::default() };
        let path = std::env::temp_dir().join(format!("bc_pdf_{}.pdf", std::process::id()));
        export(&chapters, &meta, &path).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        assert!(bytes.starts_with(b"%PDF"));
        assert!(bytes.len() > 1000);
    }
}
