//! Parse EPUB (a zip of XHTML spine items) into chapters.
//!
//! The dialog filter lists `.epub` next to txt/fb2/pdf/zip, so Linux GTK actually
//! shows those files. This module is the matching importer: OPF metadata + spine
//! order, with a small HTML-to-text walk (no extra crate).

use std::io::{Read, Seek};
use std::path::Path;

use anyhow::{anyhow, Context, Result};
use quick_xml::events::Event;
use quick_xml::reader::Reader;
use zip::ZipArchive;

use super::fb2::heading_number;
use super::load::{InputFormat, LoadedBook};
use super::parser::{validate, BookMeta, Chapter};
use super::source::decode_book_bytes;

/// Load an `.epub` zip into a [`LoadedBook`].
pub fn load(path: &Path) -> Result<LoadedBook> {
    let file = std::fs::File::open(path).with_context(|| format!("opening {}", path.display()))?;
    load_archive(ZipArchive::new(file).with_context(|| format!("epub zip {}", path.display()))?)
}

fn load_archive<R: Read + Seek>(mut zip: ZipArchive<R>) -> Result<LoadedBook> {
    let opf_path = find_opf_path(&mut zip)?;
    let opf = zip_text(&mut zip, &opf_path)?;
    let package = parse_opf(&opf)?;
    let base_dir = opf_path
        .rsplit_once('/')
        .map(|(dir, _)| dir.to_string())
        .unwrap_or_default();

    let mut chapters = Vec::new();
    for idref in &package.spine {
        let Some(item) = package.manifest.iter().find(|item| item.id == *idref) else {
            continue;
        };
        if !item.is_html() || item.is_nav() {
            continue;
        }
        let href = join_href(&base_dir, &item.href);
        let Ok(xhtml) = zip_text(&mut zip, &href) else {
            continue;
        };
        let (title, body) = html_to_chapter(&xhtml);
        if title.is_empty() && body.trim().is_empty() {
            continue;
        }
        let title = if title.is_empty() {
            format!("Chapter {}", chapters.len() + 1)
        } else {
            title
        };
        let number = heading_number(&title).map(|(n, _)| n);
        chapters.push(Chapter {
            index: chapters.len() + 1,
            number,
            title,
            body,
        });
    }

    let cover = extract_cover(&mut zip, &package, &base_dir);
    let report = validate(&chapters, &package.meta);
    Ok(LoadedBook {
        format: InputFormat::Epub,
        encoding: "EPUB".into(),
        meta: package.meta,
        chapters,
        report,
        needs_delimiter: false,
        encoding_had_errors: false,
        cover,
    })
}

struct ManifestItem {
    id: String,
    href: String,
    media_type: String,
    properties: String,
}

impl ManifestItem {
    fn is_html(&self) -> bool {
        let mt = self.media_type.to_ascii_lowercase();
        if mt.contains("ncx") || mt.contains("opf") {
            return false;
        }
        mt.contains("html") || self.href_is_html()
    }

    fn href_is_html(&self) -> bool {
        let h = self.href.to_ascii_lowercase();
        h.contains(".xhtml") || h.contains(".html") || h.contains(".htm")
    }

    fn is_nav(&self) -> bool {
        self.properties
            .split_whitespace()
            .any(|p| p.eq_ignore_ascii_case("nav"))
            || self.media_type.to_ascii_lowercase().contains("ncx")
    }

    fn is_cover_image(&self) -> bool {
        self.properties
            .split_whitespace()
            .any(|p| p.eq_ignore_ascii_case("cover-image"))
            || self.media_type.to_ascii_lowercase().starts_with("image/")
                && self.id.to_ascii_lowercase().contains("cover")
    }
}

struct OpfPackage {
    meta: BookMeta,
    manifest: Vec<ManifestItem>,
    spine: Vec<String>,
    cover_id: Option<String>,
}

fn find_opf_path<R: Read + Seek>(zip: &mut ZipArchive<R>) -> Result<String> {
    let idx = find_zip_index(zip, "META-INF/container.xml")
        .or_else(|| {
            (0..zip.len()).find(|&i| {
                zip.by_index(i)
                    .map(|e| {
                        normalize_zip_name(e.name())
                            .to_ascii_lowercase()
                            .ends_with("container.xml")
                    })
                    .unwrap_or(false)
            })
        })
        .ok_or_else(|| anyhow!("epub has no META-INF/container.xml"))?;
    let xml = zip_text_at(zip, idx)?;
    container_rootfile(&xml).ok_or_else(|| anyhow!("epub container.xml has no rootfile"))
}

fn container_rootfile(xml: &str) -> Option<String> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    loop {
        match reader.read_event().ok()? {
            Event::Start(e) | Event::Empty(e) => {
                if local_name(e.name().as_ref()) == b"rootfile" {
                    if let Some(path) = xml_attr(&e, "full-path") {
                        return Some(normalize_zip_name(&path));
                    }
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    None
}

fn parse_opf(xml: &str) -> Result<OpfPackage> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);

    let mut meta = BookMeta::default();
    let mut manifest = Vec::new();
    let mut spine = Vec::new();
    let mut cover_id = None;
    let mut buf = Vec::new();

    let mut in_metadata = false;
    let mut capture: Option<&'static str> = None;
    let mut text = String::new();

    loop {
        match reader.read_event_into(&mut buf).context("reading OPF")? {
            Event::Start(e) => {
                let name = elem_local(&e);
                match name.as_slice() {
                    b"metadata" => in_metadata = true,
                    b"title" if in_metadata => {
                        capture = Some("title");
                        text.clear();
                    }
                    b"creator" if in_metadata => {
                        capture = Some("creator");
                        text.clear();
                    }
                    b"description" if in_metadata => {
                        capture = Some("description");
                        text.clear();
                    }
                    b"meta" if in_metadata => {
                        if xml_attr(&e, "name").as_deref() == Some("cover") {
                            cover_id = xml_attr(&e, "content");
                        }
                    }
                    b"item" => {
                        if let Some(item) = manifest_item(&e) {
                            manifest.push(item);
                        }
                    }
                    b"itemref" => {
                        if let Some(idref) = xml_attr(&e, "idref") {
                            spine.push(idref);
                        }
                    }
                    _ => {}
                }
            }
            Event::Empty(e) => {
                let name = elem_local(&e);
                match name.as_slice() {
                    b"meta" if in_metadata => {
                        if xml_attr(&e, "name").as_deref() == Some("cover") {
                            cover_id = xml_attr(&e, "content");
                        }
                    }
                    b"item" => {
                        if let Some(item) = manifest_item(&e) {
                            manifest.push(item);
                        }
                    }
                    b"itemref" => {
                        if let Some(idref) = xml_attr(&e, "idref") {
                            spine.push(idref);
                        }
                    }
                    _ => {}
                }
            }
            Event::Text(e) => {
                if capture.is_some() {
                    text.push_str(&e.unescape().unwrap_or_default());
                }
            }
            Event::CData(e) => {
                if capture.is_some() {
                    if let Ok(s) = std::str::from_utf8(e.as_ref()) {
                        text.push_str(s);
                    }
                }
            }
            Event::End(e) => {
                let name = end_local(&e);
                match name.as_slice() {
                    b"metadata" => in_metadata = false,
                    b"title" if capture == Some("title") => {
                        if meta.title.is_none() {
                            let t = text.trim();
                            if !t.is_empty() {
                                meta.title = Some(t.to_string());
                            }
                        }
                        capture = None;
                    }
                    b"creator" if capture == Some("creator") => {
                        if meta.author.is_none() {
                            let t = text.trim();
                            if !t.is_empty() {
                                meta.author = Some(t.to_string());
                            }
                        }
                        capture = None;
                    }
                    b"description" if capture == Some("description") => {
                        if meta.summary.is_none() {
                            let t = text.trim();
                            if !t.is_empty() {
                                meta.summary = Some(t.to_string());
                            }
                        }
                        capture = None;
                    }
                    _ => {}
                }
            }
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }

    Ok(OpfPackage {
        meta,
        manifest,
        spine,
        cover_id,
    })
}

fn manifest_item(e: &quick_xml::events::BytesStart<'_>) -> Option<ManifestItem> {
    Some(ManifestItem {
        id: xml_attr(e, "id")?,
        href: xml_attr(e, "href").unwrap_or_default(),
        media_type: xml_attr(e, "media-type").unwrap_or_default(),
        properties: xml_attr(e, "properties").unwrap_or_default(),
    })
}

fn extract_cover<R: Read + Seek>(
    zip: &mut ZipArchive<R>,
    package: &OpfPackage,
    base_dir: &str,
) -> Option<(String, Vec<u8>)> {
    let item = package
        .cover_id
        .as_ref()
        .and_then(|id| package.manifest.iter().find(|item| item.id == *id))
        .or_else(|| package.manifest.iter().find(|item| item.is_cover_image()))?;
    if !item.media_type.to_ascii_lowercase().starts_with("image/")
        && !looks_like_image_href(&item.href)
    {
        return None;
    }
    let href = join_href(base_dir, &item.href);
    let bytes = zip_bytes(zip, &href).ok()?;
    if bytes.is_empty() {
        return None;
    }
    let content_type = if item.media_type.to_ascii_lowercase().starts_with("image/") {
        item.media_type.clone()
    } else {
        image_mime(&item.href)
    };
    Some((content_type, bytes))
}

fn looks_like_image_href(href: &str) -> bool {
    let h = href.to_ascii_lowercase();
    h.ends_with(".jpg")
        || h.ends_with(".jpeg")
        || h.ends_with(".png")
        || h.ends_with(".gif")
        || h.ends_with(".webp")
        || h.ends_with(".svg")
}

fn image_mime(href: &str) -> String {
    let h = href.to_ascii_lowercase();
    if h.ends_with(".png") {
        "image/png".into()
    } else if h.ends_with(".gif") {
        "image/gif".into()
    } else if h.ends_with(".webp") {
        "image/webp".into()
    } else if h.ends_with(".svg") {
        "image/svg+xml".into()
    } else {
        "image/jpeg".into()
    }
}

/// First heading (or `<title>`) plus remaining body text.
fn html_to_chapter(html: &str) -> (String, String) {
    let mut title = String::new();
    let mut fallback_title = String::new();
    let mut body = String::new();
    let mut skip: Option<&'static str> = None;
    let mut in_heading = false;
    let mut heading_buf = String::new();
    let mut in_doc_title = false;
    let mut i = 0;
    let bytes = html.as_bytes();

    while i < bytes.len() {
        if bytes[i] == b'<' {
            let rel_end = html[i..].find('>').unwrap_or(html.len() - i);
            let tag = &html[i + 1..i + rel_end];
            let closing = tag.trim_start().starts_with('/');
            let name = tag_name(tag);
            i += rel_end + 1;

            if let Some(until) = skip {
                if closing && name == until {
                    skip = None;
                }
                continue;
            }

            match name {
                "script" | "style" | "svg" if !closing => skip = Some(name),
                "title" if !closing => {
                    in_doc_title = true;
                    fallback_title.clear();
                }
                "title" if closing => in_doc_title = false,
                "br" => push_break(&mut body),
                name if is_heading(name) && !closing => {
                    in_heading = true;
                    heading_buf.clear();
                    push_break(&mut body);
                }
                name if is_heading(name) && closing => {
                    in_heading = false;
                    let heading = normalize_ws(&heading_buf);
                    if title.is_empty() && !heading.is_empty() {
                        title = heading;
                    } else if !heading.is_empty() {
                        if !body.is_empty() && !body.ends_with('\n') {
                            body.push('\n');
                        }
                        body.push_str(&heading);
                        body.push('\n');
                    }
                }
                "p" | "div" | "li" | "tr" | "blockquote" | "hgroup" | "section" | "article" => {
                    push_break(&mut body);
                }
                _ => {}
            }
            continue;
        }

        let (ch, len) = next_char(&html[i..]);
        i += len;
        if skip.is_some() {
            continue;
        }
        if ch == '&' {
            let (decoded, consumed) = decode_entity(&html[i - len..]);
            i = i - len + consumed;
            push_char(
                decoded,
                in_doc_title,
                in_heading,
                &mut fallback_title,
                &mut heading_buf,
                &mut body,
            );
            continue;
        }
        push_char(
            ch,
            in_doc_title,
            in_heading,
            &mut fallback_title,
            &mut heading_buf,
            &mut body,
        );
    }

    if title.is_empty() {
        title = normalize_ws(&fallback_title);
    }
    (title, collapse_blank_lines(body.trim()))
}

fn tag_name(tag: &str) -> &'static str {
    let raw = tag.trim().trim_start_matches('/').trim_end_matches('/');
    let name = raw
        .split(|c: char| c.is_ascii_whitespace() || c == '>' || c == '/')
        .next()
        .unwrap_or("");
    match name.to_ascii_lowercase().as_str() {
        "script" => "script",
        "style" => "style",
        "svg" => "svg",
        "title" => "title",
        "br" => "br",
        "h1" => "h1",
        "h2" => "h2",
        "h3" => "h3",
        "h4" => "h4",
        "h5" => "h5",
        "h6" => "h6",
        "p" => "p",
        "div" => "div",
        "li" => "li",
        "tr" => "tr",
        "blockquote" => "blockquote",
        "hgroup" => "hgroup",
        "section" => "section",
        "article" => "article",
        _ => "",
    }
}

fn is_heading(name: &str) -> bool {
    matches!(name, "h1" | "h2" | "h3" | "h4" | "h5" | "h6")
}

fn push_break(body: &mut String) {
    if body.is_empty() || body.ends_with("\n\n") {
        return;
    }
    if body.ends_with('\n') {
        body.push('\n');
    } else {
        body.push_str("\n\n");
    }
}

fn push_char(
    ch: char,
    in_doc_title: bool,
    in_heading: bool,
    fallback_title: &mut String,
    heading_buf: &mut String,
    body: &mut String,
) {
    if in_doc_title {
        fallback_title.push(ch);
        return;
    }
    if in_heading {
        heading_buf.push(ch);
        return;
    }
    body.push(ch);
}

fn next_char(s: &str) -> (char, usize) {
    s.chars()
        .next()
        .map(|c| (c, c.len_utf8()))
        .unwrap_or(('\0', 1))
}

fn decode_entity(s: &str) -> (char, usize) {
    let rest = &s[1..];
    let Some(end) = rest.find(';') else {
        return ('&', 1);
    };
    let spec = &rest[..end];
    let consumed = end + 2; // '&' + spec + ';'
    let ch = if let Some(hex) = spec.strip_prefix("#x").or_else(|| spec.strip_prefix("#X")) {
        u32::from_str_radix(hex, 16).ok().and_then(char::from_u32)
    } else if let Some(num) = spec.strip_prefix('#') {
        num.parse::<u32>().ok().and_then(char::from_u32)
    } else {
        match spec {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            "nbsp" => Some('\u{a0}'),
            "mdash" => Some('—'),
            "ndash" => Some('–'),
            "hellip" => Some('…'),
            _ => None,
        }
    };
    match ch {
        Some(c) => (c, consumed),
        None => ('&', 1),
    }
}

fn normalize_ws(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn collapse_blank_lines(s: &str) -> String {
    let mut out = String::new();
    let mut blank = 0usize;
    for line in s.lines() {
        let t = line.trim();
        if t.is_empty() {
            blank += 1;
            if blank == 1 && !out.is_empty() {
                out.push('\n');
            }
            continue;
        }
        blank = 0;
        if !out.is_empty() && !out.ends_with('\n') {
            out.push('\n');
        }
        out.push_str(t);
    }
    out
}

fn xml_attr(e: &quick_xml::events::BytesStart<'_>, name: &str) -> Option<String> {
    e.attributes().with_checks(false).flatten().find_map(|a| {
        let key = a.key.as_ref();
        let key = std::str::from_utf8(key).ok()?;
        let local = key.rsplit_once(':').map(|(_, l)| l).unwrap_or(key);
        if local.eq_ignore_ascii_case(name) {
            Some(a.unescape_value().ok()?.into_owned())
        } else {
            None
        }
    })
}

fn elem_local(e: &quick_xml::events::BytesStart<'_>) -> Vec<u8> {
    local_name(e.name().as_ref()).to_vec()
}

fn end_local(e: &quick_xml::events::BytesEnd<'_>) -> Vec<u8> {
    local_name(e.name().as_ref()).to_vec()
}

fn local_name(qname: &[u8]) -> &[u8] {
    qname.rsplit(|&b| b == b':').next().unwrap_or(qname)
}

fn normalize_zip_name(name: &str) -> String {
    name.replace('\\', "/")
        .trim_start_matches("./")
        .trim_start_matches('/')
        .to_string()
}

fn join_href(base_dir: &str, href: &str) -> String {
    let href = percent_decode(href.split('#').next().unwrap_or(href)).replace('\\', "/");
    let combined = if href.starts_with('/') {
        href.trim_start_matches('/').to_string()
    } else if base_dir.is_empty() {
        href
    } else {
        format!("{base_dir}/{href}")
    };
    let mut stack: Vec<&str> = Vec::new();
    for part in combined.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                stack.pop();
            }
            p => stack.push(p),
        }
    }
    stack.join("/")
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(b) =
                u8::from_str_radix(std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or(""), 16)
            {
                out.push(b);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn find_zip_index<R: Read + Seek>(zip: &mut ZipArchive<R>, name: &str) -> Option<usize> {
    let want = normalize_zip_name(name);
    for i in 0..zip.len() {
        let n = match zip.by_index(i).ok().map(|e| normalize_zip_name(e.name())) {
            Some(n) => n,
            None => continue,
        };
        if n.eq_ignore_ascii_case(&want) {
            return Some(i);
        }
    }
    None
}

fn zip_bytes<R: Read + Seek>(zip: &mut ZipArchive<R>, name: &str) -> Result<Vec<u8>> {
    let idx = find_zip_index(zip, name).ok_or_else(|| anyhow!("epub missing {name}"))?;
    let mut entry = zip
        .by_index(idx)
        .map_err(|e| anyhow!("epub entry {name}: {e}"))?;
    let mut buf = Vec::new();
    entry.read_to_end(&mut buf)?;
    Ok(buf)
}

fn zip_text_at<R: Read + Seek>(zip: &mut ZipArchive<R>, idx: usize) -> Result<String> {
    let mut entry = zip
        .by_index(idx)
        .map_err(|e| anyhow!("epub zip index {idx}: {e}"))?;
    let mut buf = Vec::new();
    entry.read_to_end(&mut buf)?;
    Ok(bytes_to_text(&buf))
}

fn zip_text<R: Read + Seek>(zip: &mut ZipArchive<R>, name: &str) -> Result<String> {
    Ok(bytes_to_text(&zip_bytes(zip, name)?))
}

fn bytes_to_text(bytes: &[u8]) -> String {
    if let Ok(s) = std::str::from_utf8(bytes) {
        s.to_string()
    } else {
        decode_book_bytes(bytes).text
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Cursor, Write};
    use zip::write::SimpleFileOptions;
    use zip::ZipWriter;

    fn sample_epub_bytes() -> Vec<u8> {
        let cursor = Cursor::new(Vec::new());
        let mut zip = ZipWriter::new(cursor);
        let opt = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        zip.start_file("mimetype", opt).unwrap();
        zip.write_all(b"application/epub+zip").unwrap();
        zip.start_file("META-INF/container.xml", opt).unwrap();
        zip.write_all(
            br#"<?xml version="1.0"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles>
    <rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/>
  </rootfiles>
</container>"#,
        )
        .unwrap();
        zip.start_file("OEBPS/content.opf", opt).unwrap();
        zip.write_all(
            br#"<?xml version="1.0"?>
<package xmlns="http://www.idpf.org/2007/opf" unique-identifier="id" version="2.0">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:title>Test Book</dc:title>
    <dc:creator>Jane Doe</dc:creator>
    <dc:description>A short blurb.</dc:description>
    <meta name="cover" content="cover"/>
  </metadata>
  <manifest>
    <item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/>
    <item id="ch1" href="ch1.xhtml" media-type="application/xhtml+xml"/>
    <item id="ch2" href="ch%202.xhtml" media-type="application/xhtml+xml"/>
    <item id="cover" href="cover.jpg" media-type="image/jpeg"/>
  </manifest>
  <spine>
    <itemref idref="nav"/>
    <itemref idref="ch1"/>
    <itemref idref="ch2"/>
  </spine>
</package>"#,
        )
        .unwrap();
        zip.start_file("OEBPS/nav.xhtml", opt).unwrap();
        zip.write_all(b"<html><body><nav>Contents</nav></body></html>")
            .unwrap();
        zip.start_file("OEBPS/ch1.xhtml", opt).unwrap();
        zip.write_all(
            b"<html xmlns=\"http://www.w3.org/1999/xhtml\"><head><title>Ignored</title></head><body><h1>Chapter 1 The Start</h1><p>Once upon a time.</p></body></html>",
        )
        .unwrap();
        zip.start_file("OEBPS/ch 2.xhtml", opt).unwrap();
        zip.write_all(
            b"<html><body><h1>Chapter 2</h1><p>The &amp; next day&hellip;</p></body></html>",
        )
        .unwrap();
        zip.start_file("OEBPS/cover.jpg", opt).unwrap();
        zip.write_all(&[0xFF, 0xD8, 0xFF, 0xD9]).unwrap();
        let cursor = zip.finish().unwrap();
        cursor.into_inner()
    }

    #[test]
    fn parses_spine_metadata_and_skips_nav() {
        let bytes = sample_epub_bytes();
        let zip = ZipArchive::new(Cursor::new(bytes)).unwrap();
        let book = load_archive(zip).unwrap();
        assert_eq!(book.format, InputFormat::Epub);
        assert_eq!(book.meta.title.as_deref(), Some("Test Book"));
        assert_eq!(book.meta.author.as_deref(), Some("Jane Doe"));
        assert_eq!(book.meta.summary.as_deref(), Some("A short blurb."));
        assert_eq!(book.chapters.len(), 2);
        assert_eq!(book.chapters[0].title, "Chapter 1 The Start");
        assert!(book.chapters[0].body.contains("Once upon a time."));
        assert!(!book.chapters[0].body.contains("Chapter 1 The Start"));
        assert_eq!(book.chapters[1].title, "Chapter 2");
        assert!(book.chapters[1].body.contains("The & next day…"));
        assert!(book.cover.is_some());
    }

    #[test]
    fn html_heading_is_title_not_duplicated() {
        let (title, body) = html_to_chapter(
            "<html><body><h1>Chapter 1 The Start</h1><p>Once upon a time.</p></body></html>",
        );
        assert_eq!(title, "Chapter 1 The Start");
        assert_eq!(body, "Once upon a time.");
    }

    #[test]
    fn html_decodes_entities() {
        let (_, body) = html_to_chapter("<p>A &amp; B&nbsp;C&hellip;</p>");
        assert!(body.contains("A & B"));
        assert!(body.contains('…'));
    }
}
