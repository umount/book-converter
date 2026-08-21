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
    /// `data:` URL for showing the cover in the UI.
    pub fn data_url(&self) -> String {
        format!("data:{};base64,{}", self.content_type, self.base64)
    }

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

/// Metadata found in a source FB2 that we can carry over / show: the annotation
/// (summary) and the cover image.
#[derive(Debug, Clone, Default)]
pub struct Fb2Head {
    /// `<book-title>` as written in this file, in its own language.
    pub title: Option<String>,
    pub annotation: Option<String>,
    pub cover: Option<Cover>,
}

/// Extract the title, annotation (as plain text) and cover image from FB2 XML.
pub fn extract_head(xml: &str) -> Fb2Head {
    Fb2Head {
        title: slice_first(xml, "book-title")
            .map(|t| tags_to_text(&t))
            .map(|t| t.trim().to_string())
            .filter(|t| !t.is_empty()),
        annotation: slice_first(xml, "annotation")
            .map(|a| tags_to_text(&a))
            .filter(|s| !s.trim().is_empty()),
        cover: extract_cover(xml),
    }
}

/// Assemble the book into an .fb2 file.
pub fn export(chapters: &[TranslatedChapter], meta: &OutputMeta, out_path: &Path) -> Result<()> {
    std::fs::write(out_path, render(chapters, meta))
        .with_context(|| format!("writing {}", out_path.display()))?;
    Ok(())
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
    let _ = writeln!(out, "<author><nickname>{}</nickname></author>", esc(&meta.author));
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
    out.push_str("<body>\n");
    for ch in chapters {
        out.push_str("<section>\n");
        let title = ch.title.trim();
        if !title.is_empty() {
            let _ = writeln!(out, "<title><p>{}</p></title>", esc(title));
        }
        for para in ch.body.lines().map(str::trim).filter(|l| !l.is_empty()) {
            let _ = writeln!(out, "<p>{}</p>", esc(para));
        }
        out.push_str("</section>\n");
    }
    out.push_str("</body>\n");

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

/// Find the first image `<binary>` block and return it as a `Cover`.
fn extract_cover(xml: &str) -> Option<Cover> {
    for block in slice_all(xml, "binary") {
        let ct = attr(&block, "content-type")?;
        if !ct.starts_with("image/") {
            continue;
        }
        // inner text = base64 between the opening tag's '>' and '</binary>'
        let start = block.find('>')? + 1;
        let end = block.rfind("</binary>")?;
        let base64: String = block[start..end].split_whitespace().collect();
        if base64.is_empty() {
            continue;
        }
        return Some(Cover {
            content_type: ct.to_string(),
            base64,
        });
    }
    None
}

/// Read an attribute value from a tag string.
fn attr<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let key = format!("{name}=\"");
    let start = tag.find(&key)? + key.len();
    let end = tag[start..].find('"')? + start;
    Some(&tag[start..end])
}

/// Convert a fragment of FB2 markup to plain text (paragraph breaks preserved).
fn tags_to_text(xml: &str) -> String {
    let with_breaks = xml.replace("</p>", "\n").replace("<empty-line/>", "\n");
    let re = regex::Regex::new(r"<[^>]+>").expect("valid regex");
    let text = re.replace_all(&with_breaks, "");
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
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
            cover: Some(Cover { content_type: "image/jpeg".into(), base64: "QUJD".into() }),
        };
        let xml = render(&sample(), &meta);
        assert!(xml.contains("<book-title>Книга</book-title>"));
        assert!(xml.contains("<annotation>"));
        assert!(xml.contains("Краткое описание."));
        assert!(xml.contains("<coverpage><image l:href=\"#cover.jpg\"/></coverpage>"));
        assert!(xml.contains("<binary id=\"cover.jpg\" content-type=\"image/jpeg\">QUJD</binary>"));
        assert!(xml.contains("&lt; &amp; &gt;"));
    }

    #[test]
    fn extracts_annotation_and_cover() {
        let src = r##"<FictionBook><description><title-info>
<annotation><p>Первый абзац.</p><p>Второй.</p></annotation></title-info></description>
<body/><binary id="c.jpg" content-type="image/jpeg">QUJDRA==</binary></FictionBook>"##;
        let head = extract_head(src);
        assert_eq!(head.annotation.as_deref(), Some("Первый абзац.\nВторой."));
        let cover = head.cover.unwrap();
        assert_eq!(cover.content_type, "image/jpeg");
        assert_eq!(cover.base64, "QUJDRA==");
        assert!(cover.data_url().starts_with("data:image/jpeg;base64,"));
    }
}
