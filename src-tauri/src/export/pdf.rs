//! Export to PDF via `genpdf`, with an embedded Unicode font (Cyrillic-capable).

use std::path::Path;

use anyhow::{anyhow, Result};
use genpdf::{elements, fonts, style, Document, Element as _, SimplePageDecorator};

use super::{OutputMeta, TranslatedChapter};

/// Assemble the book into a `.pdf`.
pub fn export(chapters: &[TranslatedChapter], meta: &OutputMeta, out_path: &Path) -> Result<()> {
    let regular = fonts::FontData::new(
        include_bytes!("../../assets/DejaVuSans.ttf").to_vec(),
        None,
    )
    .map_err(|e| anyhow!("pdf font: {e}"))?;
    let bold = fonts::FontData::new(
        include_bytes!("../../assets/DejaVuSans-Bold.ttf").to_vec(),
        None,
    )
    .map_err(|e| anyhow!("pdf font: {e}"))?;

    let family = fonts::FontFamily {
        regular: regular.clone(),
        bold: bold.clone(),
        italic: regular,
        bold_italic: bold,
    };

    let mut doc = Document::new(family);
    doc.set_title(&meta.title);
    doc.set_minimal_conformance();
    let mut decorator = SimplePageDecorator::new();
    decorator.set_margins(15);
    doc.set_page_decorator(decorator);

    // Title page
    doc.push(
        elements::Paragraph::new(meta.title.trim())
            .styled(style::Style::new().bold().with_font_size(22)),
    );
    if !meta.author.trim().is_empty() {
        doc.push(elements::Paragraph::new(meta.author.trim()));
    }

    for ch in chapters {
        doc.push(elements::Break::new(1.0));
        let title = ch.title.trim();
        if !title.is_empty() {
            doc.push(
                elements::Paragraph::new(title)
                    .styled(style::Style::new().bold().with_font_size(15)),
            );
        }
        for para in ch.body.lines().map(str::trim).filter(|l| !l.is_empty()) {
            doc.push(elements::Paragraph::new(para));
        }
    }

    doc.render_to_file(out_path)
        .map_err(|e| anyhow!("rendering PDF: {e}"))?;
    Ok(())
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
