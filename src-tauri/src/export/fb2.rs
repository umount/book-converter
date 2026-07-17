//! Export to FB2 (FictionBook 2.0): one `<section>` per chapter + book metadata.

use std::fmt::Write as _;
use std::path::Path;

use anyhow::{Context, Result};

use super::{OutputMeta, TranslatedChapter};

/// Assemble the book into an .fb2 file.
pub fn export(chapters: &[TranslatedChapter], meta: &OutputMeta, out_path: &Path) -> Result<()> {
    let xml = render(chapters, meta);
    std::fs::write(out_path, xml).with_context(|| format!("writing {}", out_path.display()))?;
    Ok(())
}

/// Render the whole FB2 document as a string.
pub fn render(chapters: &[TranslatedChapter], meta: &OutputMeta) -> String {
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
        for para in paragraphs(&ch.body) {
            let _ = writeln!(out, "<p>{}</p>", esc(para));
        }
        out.push_str("</section>\n");
    }
    out.push_str("</body>\n</FictionBook>\n");
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
        let xml = render(&sample(), &meta);
        assert!(xml.contains("<book-title>Книга</book-title>"));
        assert!(xml.contains("<nickname>Автор</nickname>"));
        // XML special chars are escaped in body text
        assert!(xml.contains("&lt; &amp; &gt;"));
        assert!(!xml.contains("< & >"));
        assert_eq!(xml.matches("<section>").count(), 2);
    }
}
