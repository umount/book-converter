//! Export to FB2 (FictionBook 2.0): one `<section>` per chapter + book metadata.
//!
//! When continuing from a reference FB2, the original `<description>` (title,
//! author, cover reference, annotation) and `<binary>` blocks (the cover image)
//! are carried over verbatim via [`Fb2Head`], so metadata and cover are preserved.

use std::fmt::Write as _;
use std::path::Path;

use anyhow::{Context, Result};

use super::{OutputMeta, TranslatedChapter};

/// Original FB2 head to preserve on export: the raw `<description>` block and the
/// raw `<binary>` blocks (e.g. the cover image).
#[derive(Debug, Clone, Default)]
pub struct Fb2Head {
    pub description: Option<String>,
    pub binaries: Vec<String>,
}

/// Extract the `<description>` and all `<binary>` blocks from FB2 XML.
pub fn extract_head(xml: &str) -> Fb2Head {
    Fb2Head {
        description: slice_first(xml, "description"),
        binaries: slice_all(xml, "binary"),
    }
}

/// Assemble the book into an .fb2 file (optionally preserving a source head).
pub fn export(
    chapters: &[TranslatedChapter],
    meta: &OutputMeta,
    head: Option<&Fb2Head>,
    out_path: &Path,
) -> Result<()> {
    let xml = render(chapters, meta, head);
    std::fs::write(out_path, xml).with_context(|| format!("writing {}", out_path.display()))?;
    Ok(())
}

/// Render the whole FB2 document as a string.
pub fn render(chapters: &[TranslatedChapter], meta: &OutputMeta, head: Option<&Fb2Head>) -> String {
    let mut out = String::new();
    out.push_str("<?xml version=\"1.0\" encoding=\"utf-8\"?>\n");
    out.push_str(
        "<FictionBook xmlns=\"http://www.gribuser.ru/xml/fictionbook/2.0\" \
         xmlns:l=\"http://www.w3.org/1999/xlink\">\n",
    );

    // --- description: reuse the source one (keeps author + cover) if available ---
    match head.and_then(|h| h.description.as_deref()) {
        Some(desc) => {
            out.push_str(desc);
            out.push('\n');
        }
        None => {
            out.push_str("<description>\n<title-info>\n");
            let _ = writeln!(out, "<genre>literature</genre>");
            let _ = writeln!(out, "<author><nickname>{}</nickname></author>", esc(&meta.author));
            let _ = writeln!(out, "<book-title>{}</book-title>", esc(&meta.title));
            let _ = writeln!(out, "<lang>{}</lang>", esc(&meta.lang));
            out.push_str("</title-info>\n<document-info>\n");
            let _ = writeln!(out, "<id>book-converter-{}</id>", slug(&meta.title));
            out.push_str("<program-used>book-converter</program-used>\n");
            out.push_str("</document-info>\n</description>\n");
        }
    }

    // --- body ---
    out.push_str("<body>\n");
    for ch in chapters {
        out.push_str("<section>\n");
        let title = ch.title.trim();
        if !title.is_empty() {
            let _ = writeln!(out, "<title><p>{}</p></title>", esc(title));
        }
        for para in paragraphs(&ch.body) {
            let _ = writeln!(out, "<p>{}</p>", esc(para));
        }
        out.push_str("</section>\n");
    }
    out.push_str("</body>\n");

    // --- binaries (cover image, …) carried over verbatim ---
    if let Some(h) = head {
        for bin in &h.binaries {
            out.push_str(bin);
            out.push('\n');
        }
    }

    out.push_str("</FictionBook>\n");
    out
}

/// Split a chapter body into paragraphs on line breaks (blank lines collapse).
fn paragraphs(body: &str) -> impl Iterator<Item = &str> {
    body.lines().map(str::trim).filter(|l| !l.is_empty())
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

/// First `<tag …>…</tag>` block (raw), if present.
fn slice_first(xml: &str, tag: &str) -> Option<String> {
    let open = format!("<{tag}");
    let close = format!("</{tag}>");
    let start = xml.find(&open)?;
    let end = xml[start..].find(&close)? + start + close.len();
    Some(xml[start..end].to_string())
}

/// All `<tag …>…</tag>` blocks (raw), in order.
fn slice_all(xml: &str, tag: &str) -> Vec<String> {
    let open = format!("<{tag}");
    let close = format!("</{tag}>");
    let mut out = Vec::new();
    let mut pos = 0;
    while let Some(rel) = xml[pos..].find(&open) {
        let start = pos + rel;
        match xml[start..].find(&close) {
            Some(rel_end) => {
                let end = start + rel_end + close.len();
                out.push(xml[start..end].to_string());
                pos = end;
            }
            None => break,
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Vec<TranslatedChapter> {
        vec![
            TranslatedChapter {
                index: 1,
                number: Some(1),
                title: "Глава 1".into(),
                body: "Первый абзац.\n\nВторой абзац с < & >.".into(),
            },
            TranslatedChapter { index: 2, number: Some(2), title: "Глава 2".into(), body: "Текст.".into() },
        ]
    }

    #[test]
    fn escapes_and_structures() {
        let meta = OutputMeta { title: "Книга".into(), author: "Автор".into(), lang: "ru".into() };
        let xml = render(&sample(), &meta, None);
        assert!(xml.contains("<book-title>Книга</book-title>"));
        assert!(xml.contains("<nickname>Автор</nickname>"));
        assert!(xml.contains("&lt; &amp; &gt;"));
        assert_eq!(xml.matches("<section>").count(), 2);
    }

    #[test]
    fn preserves_head_description_and_cover() {
        let source = r##"<FictionBook><description><title-info>
<author><first-name>Er Gen</first-name></author><book-title>Src</book-title>
<coverpage><image l:href="#cover.jpg"/></coverpage></title-info></description>
<body><section><p>x</p></section></body>
<binary id="cover.jpg" content-type="image/jpeg">QUJD</binary></FictionBook>"##;
        let head = extract_head(source);
        assert!(head.description.as_ref().unwrap().contains("Er Gen"));
        assert_eq!(head.binaries.len(), 1);

        let meta = OutputMeta::default();
        let xml = render(&sample(), &meta, Some(&head));
        // original author + cover reference carried over
        assert!(xml.contains("Er Gen"));
        assert!(xml.contains("coverpage"));
        // cover binary carried over, placed after the body
        assert!(xml.contains(r#"<binary id="cover.jpg""#));
        assert!(xml.find("</body>").unwrap() < xml.find("<binary").unwrap());
    }
}
