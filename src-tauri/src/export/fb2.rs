//! Export to FB2 (FictionBook 2.0): one `<section>` per chapter + book metadata
//! (translated title, author, annotation/summary, cover image).

use std::fmt::Write as _;
use std::path::Path;

use anyhow::{Context, Result};

use super::{OutputMeta, TranslatedChapter};

/// A cover image: its MIME type and base64-encoded bytes.
#[derive(Debug, Clone, Default)]
pub struct Cover {
    pub content_type: String,
    pub base64: String,
}

impl Cover {
    /// File extension implied by the content type (for the FB2 binary id).
    fn ext(&self) -> &str {
        match self.content_type.as_str() {
            "image/png" => "png",
            "image/gif" => "gif",
            "image/webp" => "webp",
            _ => "jpg",
        }
    }
}

/// Assemble the book into an .fb2 file.
pub fn export(chapters: &[TranslatedChapter], meta: &OutputMeta, out_path: &Path) -> Result<()> {
    std::fs::write(out_path, render(chapters, meta))
        .with_context(|| format!("writing {}", out_path.display()))?;
    Ok(())
}

/// Base64 of a file, or `None` when it cannot be read (a picture missing from
/// the project costs that picture, not the export).
fn read_base64(path: &Path) -> Option<String> {
    use base64::Engine as _;
    match std::fs::read(path) {
        Ok(bytes) => Some(base64::engine::general_purpose::STANDARD.encode(bytes)),
        Err(e) => {
            tracing::warn!(path = %path.display(), "fb2 export: image unreadable: {e}");
            None
        }
    }
}

/// Render the whole FB2 document as a string.
pub fn render(chapters: &[TranslatedChapter], meta: &OutputMeta) -> String {
    let cover_id = meta.cover.as_ref().map(|c| format!("cover.{}", c.ext()));

    let mut out = String::new();
    out.push_str("<?xml version=\"1.0\" encoding=\"utf-8\"?>\n");
    out.push_str(
        "<FictionBook xmlns=\"http://www.gribuser.ru/xml/fictionbook/2.0\" \
         xmlns:l=\"http://www.w3.org/1999/xlink\">\n",
    );

    // --- description ---
    out.push_str("<description>\n<title-info>\n");
    let _ = writeln!(out, "<genre>literature</genre>");
    let _ = writeln!(
        out,
        "<author><nickname>{}</nickname></author>",
        esc(&meta.author)
    );
    let _ = writeln!(out, "<book-title>{}</book-title>", esc(&meta.title));
    if let Some(annotation) = meta.annotation.as_deref().filter(|a| !a.trim().is_empty()) {
        out.push_str("<annotation>\n");
        for para in annotation.lines().map(str::trim).filter(|l| !l.is_empty()) {
            let _ = writeln!(out, "<p>{}</p>", esc(para));
        }
        out.push_str("</annotation>\n");
    }
    if let Some(id) = &cover_id {
        let _ = writeln!(out, "<coverpage><image l:href=\"#{id}\"/></coverpage>");
    }
    let _ = writeln!(out, "<lang>{}</lang>", esc(&meta.lang));
    out.push_str("</title-info>\n<document-info>\n");
    let _ = writeln!(out, "<id>book-converter-{}</id>", slug(&meta.title));
    out.push_str("<program-used>book-converter</program-used>\n");
    out.push_str("</document-info>\n</description>\n");

    // --- body ---
    // Pictures are referenced inline and carried as binaries at the end of the
    // file, the same way the cover is.
    let mut embedded: Vec<(String, &super::ExportImage, String)> = Vec::new();
    out.push_str("<body>\n");
    for ch in chapters {
        out.push_str("<section>\n");
        let title = ch.title.trim();
        if !title.is_empty() {
            let _ = writeln!(out, "<title><p>{}</p></title>", esc(title));
        }
        for piece in ch.body.pieces() {
            match piece {
                super::Piece::Para(text) => {
                    let _ = writeln!(out, "<p>{}</p>", esc(text));
                }
                super::Piece::Image(id) => {
                    let Some(image) = meta.images.get(id) else {
                        continue;
                    };
                    if !embedded.iter().any(|(known, _, _)| known == id) {
                        let Some(base64) = read_base64(&image.path) else {
                            continue;
                        };
                        embedded.push((id.to_string(), image, base64));
                    }
                    let _ = writeln!(out, "<p><image l:href=\"#img{id}\"/></p>");
                }
            }
        }
        out.push_str("</section>\n");
    }
    out.push_str("</body>\n");

    // --- image binaries ---
    for (id, image, base64) in &embedded {
        let _ = writeln!(
            out,
            "<binary id=\"img{id}\" content-type=\"{}\">{base64}</binary>",
            esc(&image.content_type),
        );
    }

    // --- cover binary ---
    if let (Some(id), Some(cover)) = (&cover_id, meta.cover.as_ref()) {
        let _ = writeln!(
            out,
            "<binary id=\"{id}\" content-type=\"{}\">{}</binary>",
            cover.content_type, cover.base64
        );
    }

    out.push_str("</FictionBook>\n");
    out
}

/// Escape XML text content.
fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// A file-safe slug for the document id.
fn slug(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .trim_matches('-')
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn sample() -> Vec<TranslatedChapter> {
        vec![TranslatedChapter {
            index: 1,
            number: Some(1),
            title: "Глава 1".into(),
            body: "Абзац с < & >.".into(),
        }]
    }

    #[test]
    fn builds_title_annotation_cover() {
        let meta = OutputMeta {
            title: "Книга".into(),
            author: "Автор".into(),
            lang: "ru".into(),
            annotation: Some("Краткое описание.".into()),
            cover: Some(Cover {
                content_type: "image/jpeg".into(),
                base64: "QUJD".into(),
            }),
            ..OutputMeta::default()
        };
        let xml = render(&sample(), &meta);
        assert!(xml.contains("<book-title>Книга</book-title>"));
        assert!(xml.contains("<annotation>"));
        assert!(xml.contains("Краткое описание."));
        assert!(xml.contains("<coverpage><image l:href=\"#cover.jpg\"/></coverpage>"));
        assert!(xml.contains("<binary id=\"cover.jpg\" content-type=\"image/jpeg\">QUJD</binary>"));
        assert!(xml.contains("&lt; &amp; &gt;"));
    }

    /// An illustrated chapter carries its picture, not a `[[img:…]]` line.
    #[test]
    fn embeds_a_referenced_picture() {
        let dir = std::env::temp_dir().join(format!("bc_fb2_img_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("ab12.png");
        std::fs::write(&path, b"ABC").unwrap();

        let meta = OutputMeta {
            images: HashMap::from([(
                "ab12".to_string(),
                super::super::ExportImage {
                    path,
                    content_type: "image/png".into(),
                },
            )]),
            ..OutputMeta::default()
        };
        let chapters = vec![TranslatedChapter {
            index: 1,
            number: Some(1),
            title: "Глава 1".into(),
            body: crate::export::ChapterBody::Blocks(vec![crate::export::ExportBlock::Text("До.".into()),crate::export::ExportBlock::Image("ab12".into()),crate::export::ExportBlock::Text("После.".into())]),
        }];
        let xml = render(&chapters, &meta);
        assert!(xml.contains("<p><image l:href=\"#imgab12\"/></p>"));
        assert!(xml.contains("<binary id=\"imgab12\" content-type=\"image/png\">QUJD</binary>"));
        assert!(!xml.contains("[[img:"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A picture the project lost is skipped, and the chapter still exports.
    #[test]
    fn a_missing_picture_does_not_fail_the_export() {
        let chapters = vec![TranslatedChapter {
            index: 1,
            number: Some(1),
            title: "Глава 1".into(),
            body: crate::export::ChapterBody::Blocks(vec![crate::export::ExportBlock::Text("До.".into()),crate::export::ExportBlock::Image("ab12".into()),crate::export::ExportBlock::Text("После.".into())]),
        }];
        let xml = render(&chapters, &OutputMeta::default());
        assert!(xml.contains("<p>До.</p>"));
        assert!(xml.contains("<p>После.</p>"));
        assert!(!xml.contains("[[img:"));
    }


}
