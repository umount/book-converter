//! Parse FB2 (FictionBook 2.0, XML) into chapters.
//!
//! Used both for **FB2 input** (translate an FB2 book) and for reading a
//! **reference translation** (a professional FB2 whose names/lore/style seed the
//! glossary). The parser is format-generic, not tuned to any one book: it walks
//! `<section>` blocks, taking each section's `<title>` and its `<p>` paragraphs.
//!
//! Chapter numbering is read from the heading with a generic pattern
//! (`<int>` optionally `.<part>`, plus Chinese `第N章`), so "Глава 1.1", "第1章",
//! "Chapter 1", "1.1 …" all work without hardcoding a language. Structural import
//! lives in `fb2_content` and preserves unnumbered sections.
//!
//! Self-contained (only `quick-xml` + `regex` + std), so it is testable without
//! the Tauri crate.

use anyhow::{Context, Result};
use quick_xml::events::Event;
use quick_xml::reader::Reader;
use regex::Regex;

use super::parser::{parse_chapter_number, BookMeta};

/// A raw FB2 section: its heading text and concatenated paragraph text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fb2Section {
    pub title: String,
    pub body: String,
}

/// A parsed FB2 document: book metadata plus every content section in order.
#[derive(Debug, Clone, Default)]
pub struct Fb2Doc {
    pub meta: BookMeta,
    pub sections: Vec<Fb2Section>,
}

/// Parse FB2 XML into metadata + sections.
pub fn parse_fb2(xml: &str) -> Result<Fb2Doc> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);

    let mut doc = Fb2Doc::default();

    // Open section stack: (title, body). Paragraphs attach to the innermost one.
    let mut stack: Vec<Fb2Section> = Vec::new();
    let mut title_depth = 0usize; // >0 while inside a <title>
    let mut in_body = false; // inside <body> (chapter content, not description)

    // Metadata capture (from <description>).
    let mut in_book_title = false;
    let mut in_author = false;
    let mut in_first_name = false;
    let mut in_last_name = false;
    let mut author_first = String::new();
    let mut author_last = String::new();

    loop {
        match reader.read_event().context("reading FB2 XML")? {
            Event::Start(e) => match e.local_name().as_ref() {
                b"body" => in_body = true,
                b"section" if in_body => stack.push(Fb2Section {
                    title: String::new(),
                    body: String::new(),
                }),
                b"title" if in_body => title_depth += 1,
                b"book-title" => in_book_title = true,
                b"author" => in_author = true,
                b"first-name" if in_author => in_first_name = true,
                b"last-name" if in_author => in_last_name = true,
                _ => {}
            },
            Event::End(e) => match e.local_name().as_ref() {
                b"body" => in_body = false,
                b"section" if in_body => {
                    if let Some(mut sec) = stack.pop() {
                        sec.title = normalize_ws(&sec.title);
                        sec.body = sec.body.trim().to_string();
                        // Keep only sections that carry content or a heading.
                        if !sec.title.is_empty() || !sec.body.is_empty() {
                            doc.sections.push(sec);
                        }
                    }
                }
                b"title" if in_body => title_depth = title_depth.saturating_sub(1),
                b"p" if in_body && title_depth == 0 => {
                    if let Some(sec) = stack.last_mut() {
                        sec.body.push_str("\n\n");
                    }
                }
                b"book-title" => in_book_title = false,
                b"author" => {
                    if doc.meta.author.is_none() {
                        let name = format!("{} {}", author_first.trim(), author_last.trim());
                        let name = name.trim().to_string();
                        if !name.is_empty() {
                            doc.meta.author = Some(name);
                        }
                    }
                    in_author = false;
                }
                b"first-name" => in_first_name = false,
                b"last-name" => in_last_name = false,
                _ => {}
            },
            Event::Text(e) => {
                let text = e.unescape().context("unescaping FB2 text")?;
                if in_book_title && doc.meta.title.is_none() {
                    doc.meta.title = Some(text.trim().to_string());
                } else if in_first_name && author_first.is_empty() {
                    author_first = text.trim().to_string();
                } else if in_last_name && author_last.is_empty() {
                    author_last = text.trim().to_string();
                } else if in_body {
                    if let Some(sec) = stack.last_mut() {
                        if title_depth > 0 {
                            if !sec.title.is_empty() {
                                sec.title.push(' ');
                            }
                            sec.title.push_str(&text);
                        } else {
                            sec.body.push_str(&text);
                        }
                    }
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }

    Ok(doc)
}

/// Extract a chapter number (and optional part) from a heading, language-agnostically.
///
/// Handles `第N章` (Chinese numerals or Arabic) and a generic `<int>[.<part>]`
/// anywhere in the heading ("Глава 1.1", "Chapter 1", "1.1 Title").
pub fn heading_number(title: &str) -> Option<(usize, Option<usize>)> {
    // Chinese chapter marker first (covers Arabic and Chinese numerals).
    if let Some(caps) = chinese_marker().captures(title) {
        if let Some(n) = parse_chapter_number(&caps[1]) {
            return Some((n, None));
        }
    }
    // Generic "<int>[.<part>]".
    let caps = generic_number().captures(title)?;
    let number = caps.get(1)?.as_str().parse().ok()?;
    let part = caps.get(2).and_then(|m| m.as_str().parse().ok());
    Some((number, part))
}

fn chinese_marker() -> Regex {
    Regex::new(r"第([0-9]+|[一二三四五六七八九十百千零两〇]+)章").expect("valid regex")
}

fn generic_number() -> Regex {
    Regex::new(r"(\d+)(?:[.\-–](\d+))?").expect("valid regex")
}

/// Collapse runs of whitespace to single spaces and trim.
fn normalize_ws(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    const FB2: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<FictionBook xmlns="http://www.gribuser.ru/xml/fictionbook/2.0">
<description><title-info>
<author><first-name>Er Gen</first-name></author>
<book-title>За гранью времени</book-title>
</title-info></description>
<body>
<section><title><p>Глоссарий</p></title><p>Уровни культивации</p></section>
<section><title><p>Глава 1.1 Выживший</p></title><p>Март, начало весны.</p><p>Он открыл глаза.</p></section>
<section><title><p>Глава 1.2. Выживший</p></title><p>Гриф кружил вдалеке.</p></section>
<section><title><p>Глава 2. Инородная энергия</p></title><p>Прошёл день.</p></section>
</body>
</FictionBook>"#;

    #[test]
    fn parses_meta_and_sections() {
        let doc = parse_fb2(FB2).unwrap();
        assert_eq!(doc.meta.title.as_deref(), Some("За гранью времени"));
        assert_eq!(doc.meta.author.as_deref(), Some("Er Gen"));
        assert_eq!(doc.sections.len(), 4); // glossary + 3 chapter parts
        assert_eq!(doc.sections[1].title, "Глава 1.1 Выживший");
        assert!(doc.sections[1].body.contains("Он открыл глаза."));
    }

    #[test]
    fn heading_number_is_generic() {
        assert_eq!(heading_number("Глава 1.1 Выживший"), Some((1, Some(1))));
        assert_eq!(
            heading_number("Глава 2. Инородная энергия"),
            Some((2, None))
        );
        assert_eq!(heading_number("第1章 活着"), Some((1, None)));
        assert_eq!(heading_number("第九百八十四章 X"), Some((984, None)));
        assert_eq!(heading_number("Chapter 7"), Some((7, None)));
        assert_eq!(heading_number("Глоссарий"), None);
    }
}

/// Resolve the declared cover, never an arbitrary inline illustration.
pub fn embedded_cover(xml: &str) -> Result<Option<(String, Vec<u8>)>> {
    use base64::Engine as _;
    let mut reader = Reader::from_str(xml);
    let mut in_cover = false;
    let mut reference = None;
    loop {
        match reader
            .read_event()
            .context("reading FB2 cover declaration")?
        {
            Event::Start(e) if e.local_name().as_ref() == b"coverpage" => in_cover = true,
            Event::End(e) if e.local_name().as_ref() == b"coverpage" => in_cover = false,
            Event::Start(e) | Event::Empty(e)
                if in_cover && e.local_name().as_ref() == b"image" =>
            {
                for attribute in e.attributes() {
                    let attribute = attribute?;
                    if attribute.key.local_name().as_ref() == b"href" {
                        let value = attribute.unescape_value()?;
                        reference = value.strip_prefix('#').map(str::to_owned);
                    }
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    let Some(reference) = reference else {
        return Ok(None);
    };
    let mut reader = Reader::from_str(xml);
    loop {
        match reader.read_event().context("reading FB2 cover binary")? {
            Event::Start(e) if e.local_name().as_ref() == b"binary" => {
                let mut id = None;
                let mut mime = None;
                for attribute in e.attributes() {
                    let attribute = attribute?;
                    match attribute.key.as_ref() {
                        b"id" => id = Some(attribute.unescape_value()?.into_owned()),
                        b"content-type" => mime = Some(attribute.unescape_value()?.into_owned()),
                        _ => {}
                    }
                }
                if id.as_deref() == Some(&reference) {
                    let encoded = reader.read_text(e.name())?;
                    anyhow::ensure!(encoded.len() <= 32 * 1024 * 1024, "FB2 cover is too large");
                    let encoded: String = encoded.chars().filter(|c| !c.is_whitespace()).collect();
                    let bytes = base64::engine::general_purpose::STANDARD
                        .decode(encoded)
                        .context("decoding FB2 cover")?;
                    anyhow::ensure!(bytes.len() <= 20 * 1024 * 1024, "FB2 cover is too large");
                    return Ok(Some((mime.unwrap_or_default(), bytes)));
                }
            }
            Event::Eof => return Ok(None),
            _ => {}
        }
    }
}

#[cfg(test)]
mod cover_tests {
    use super::embedded_cover;

    #[test]
    fn resolves_declared_cover_instead_of_first_image() {
        let xml = r##"<FictionBook xmlns:l="http://www.w3.org/1999/xlink">
        <description><title-info><coverpage><image l:href="#cover"/></coverpage></title-info></description>
        <binary id="illustration" content-type="image/png">YmFk</binary>
        <binary id="cover" content-type="image/jpeg"> aGVs
bG8= </binary></FictionBook>"##;
        assert_eq!(
            embedded_cover(xml).unwrap(),
            Some(("image/jpeg".into(), b"hello".to_vec()))
        );
        assert!(embedded_cover(&xml.replace("#cover", "#missing"))
            .unwrap()
            .is_none());
        assert!(embedded_cover(&xml.replace("aGVs\nbG8=", "invalid!")).is_err());
        assert!(
            embedded_cover("<FictionBook><binary id='image'>YmFk</binary></FictionBook>")
                .unwrap()
                .is_none()
        );
    }
}
