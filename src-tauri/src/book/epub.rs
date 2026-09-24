//! Parse EPUB (a zip of XHTML spine items) into chapters.
//!
//! The dialog filter lists `.epub` next to txt/fb2/pdf/zip, so Linux GTK actually
//! shows those files. This module is the matching importer: OPF metadata + spine
//! order, with a small HTML-to-blocks walk (no extra crate).
//!
//! A page is read as [`Block`]s rather than a string, because an EPUB page can be
//! a whole image: manga, illustrated editions, and comic-style web novels all
//! ship spine items whose entire content is `<img>` or an `<svg><image/>`
//! wrapper. Flattening those to text produced an empty chapter, which the reader
//! then showed as a blank pane.

use std::collections::{HashMap, HashSet};
use std::io::{Read, Seek};
use std::path::Path;

use anyhow::{anyhow, Context, Result};
use quick_xml::events::Event;
use quick_xml::reader::Reader;
use zip::ZipArchive;

use super::blocks::{derive_text, AssetRef, Block, ChapterBlocks, ChapterKind};
use super::fb2::heading_number;
use super::load::{InputFormat, LoadedBook};
use super::parser::{validate, BookMeta, Chapter};
use super::source::{decode_book_bytes, image_mime, percent_decode};

/// Load an `.epub` zip into a [`LoadedBook`].
pub fn load(path: &Path) -> Result<LoadedBook> {
    let file = std::fs::File::open(path).with_context(|| format!("opening {}", path.display()))?;
    load_archive(ZipArchive::new(file).with_context(|| format!("epub zip {}", path.display()))?)
}

fn load_archive<R: Read + Seek>(mut zip: ZipArchive<R>) -> Result<LoadedBook> {
    let opf_path = find_opf_path(&mut zip)?;
    let opf = zip_text(&mut zip, &opf_path)?;
    let package = parse_opf(&opf)?;
    let base_dir = dir_of(&opf_path);

    let mut chapters = Vec::new();
    let mut chapter_blocks: Vec<ChapterBlocks> = Vec::new();
    let mut assets: Vec<AssetRef> = Vec::new();
    let mut seen_assets: HashSet<String> = HashSet::new();
    // Zip entry name → asset id, so a page reused across the spine (and a cover
    // that also appears as a page) is hashed once.
    let mut by_entry: HashMap<String, Option<String>> = HashMap::new();

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
        // Image hrefs inside a page are relative to that page, not to the OPF.
        let page_dir = dir_of(&href);
        let (title, raw) = html_to_blocks(&xhtml);

        let mut blocks: Vec<Block> = Vec::new();
        for part in raw {
            match part {
                RawBlock::Text(text) => blocks.push(Block::text(text)),
                RawBlock::Caption(text) => blocks.push(Block::caption(text)),
                RawBlock::Image(src) => {
                    let entry = join_href(&page_dir, &src);
                    let id = match by_entry.get(&entry) {
                        Some(cached) => cached.clone(),
                        None => {
                            let made = register_asset(&mut zip, &entry, &mut assets, &mut seen_assets);
                            by_entry.insert(entry, made.clone());
                            made
                        }
                    };
                    if let Some(id) = id {
                        blocks.push(Block::image(id));
                    }
                }
            }
        }

        let kind = ChapterKind::classify(&blocks);
        if kind == ChapterKind::Empty && title.is_empty() {
            continue;
        }
        let title = if title.is_empty() {
            format!("Chapter {}", chapters.len() + 1)
        } else {
            title
        };
        let number = heading_number(&title).map(|(n, _)| n);
        let index = chapters.len() + 1;
        // Prose needs no block rows: its only block is the body itself, and
        // storing it twice would double the database on a book-length text.
        if kind != ChapterKind::Text {
            chapter_blocks.push(ChapterBlocks {
                chapter_index: index,
                kind,
                blocks: blocks.clone(),
            });
        }
        chapters.push(Chapter {
            index,
            number,
            title,
            body: derive_text(&blocks),
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
        blocks: chapter_blocks,
        assets,
        embedded_assets: Vec::new(),
    })
}

/// Hash one image entry and remember it as an asset. `None` when the entry is
/// missing from the zip or is not an image, so a broken `<img>` costs its block
/// and not the chapter.
fn register_asset<R: Read + Seek>(
    zip: &mut ZipArchive<R>,
    entry: &str,
    assets: &mut Vec<AssetRef>,
    seen: &mut HashSet<String>,
) -> Option<String> {
    if !looks_like_image_href(entry) {
        return None;
    }
    let bytes = zip_bytes(zip, entry).ok()?;
    if bytes.is_empty() {
        return None;
    }
    let id = asset_id(&bytes);
    // Identical bytes under two names are one asset; the first href wins.
    if seen.insert(id.clone()) {
        assets.push(AssetRef {
            id: id.clone(),
            href: entry.to_string(),
            content_type: image_mime(entry),
            bytes: bytes.len() as u64,
        });
    }
    Some(id)
}

/// Content-addressed id: the first 8 bytes of the SHA-256, as hex.
fn asset_id(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(bytes)
        .iter()
        .take(8)
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Directory part of a zip entry name (`""` at the archive root).
fn dir_of(name: &str) -> String {
    name.rsplit_once('/')
        .map(|(dir, _)| dir.to_string())
        .unwrap_or_default()
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

/// One block as the page provides it: an image href is still relative to the page.
#[derive(Debug, Clone, PartialEq, Eq)]
enum RawBlock {
    Text(String),
    Caption(String),
    Image(String),
}

/// Where a character belongs while walking the page.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Sink {
    DocTitle,
    Heading,
    Caption,
    Body,
    Drop,
}

/// First heading (or `<title>`) plus the page's blocks, in document order.
///
/// Images end their text run, so a page reads back as prose, picture, prose —
/// which is what lets the reader show an illustration where it belongs instead
/// of dropping it.
fn html_to_blocks(html: &str) -> (String, Vec<RawBlock>) {
    let mut title = String::new();
    let mut fallback_title = String::new();
    let mut body = String::new();
    let mut caption = String::new();
    let mut heading_buf = String::new();
    let mut blocks: Vec<RawBlock> = Vec::new();
    let mut skip: Option<&'static str> = None;
    // `<svg>` wrappers are how fixed-layout EPUBs (manga) reference a page
    // image, so the element is walked for its `<image>` and only its text dropped.
    let mut svg_depth = 0usize;
    let mut in_caption = false;
    let mut in_heading = false;
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
                "script" | "style" if !closing => skip = Some(name),
                "svg" => {
                    if closing {
                        svg_depth = svg_depth.saturating_sub(1);
                    } else {
                        svg_depth += 1;
                    }
                }
                "img" | "image" if !closing => {
                    if let Some(src) = image_src(tag) {
                        flush_text(&mut body, &mut blocks);
                        blocks.push(RawBlock::Image(src));
                    }
                }
                "figcaption" if !closing => {
                    flush_text(&mut body, &mut blocks);
                    caption.clear();
                    in_caption = true;
                }
                "figcaption" if closing => {
                    in_caption = false;
                    let text = normalize_ws(&caption);
                    if !text.is_empty() {
                        blocks.push(RawBlock::Caption(text));
                    }
                    caption.clear();
                }
                "title" if svg_depth == 0 => {
                    in_doc_title = !closing;
                    if !closing {
                        fallback_title.clear();
                    }
                }
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
                "p" | "div" | "li" | "tr" | "blockquote" | "hgroup" | "section" | "article"
                | "figure" => {
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
        let sink = if svg_depth > 0 {
            Sink::Drop
        } else if in_doc_title {
            Sink::DocTitle
        } else if in_heading {
            Sink::Heading
        } else if in_caption {
            Sink::Caption
        } else {
            Sink::Body
        };
        let ch = if ch == '&' {
            let (decoded, consumed) = decode_entity(&html[i - len..]);
            i = i - len + consumed;
            decoded
        } else {
            ch
        };
        push_char(
            ch,
            sink,
            &mut fallback_title,
            &mut heading_buf,
            &mut caption,
            &mut body,
        );
    }

    flush_text(&mut body, &mut blocks);
    let trailing = normalize_ws(&caption);
    if !trailing.is_empty() {
        blocks.push(RawBlock::Caption(trailing));
    }
    if title.is_empty() {
        title = normalize_ws(&fallback_title);
    }
    (title, blocks)
}

/// End the current text run, dropping it when it held nothing but whitespace.
fn flush_text(body: &mut String, blocks: &mut Vec<RawBlock>) {
    let text = collapse_blank_lines(body.trim());
    body.clear();
    if !text.is_empty() {
        blocks.push(RawBlock::Text(text));
    }
}

/// The image reference of an `<img>` / SVG `<image>` tag.
fn image_src(tag: &str) -> Option<String> {
    tag_attr(tag, "src")
        .or_else(|| tag_attr(tag, "href"))
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty() && !s.starts_with("data:"))
}

/// One attribute of a raw tag, matched on its local name so `xlink:href` and
/// `href` are the same attribute.
fn tag_attr(tag: &str, want: &str) -> Option<String> {
    let chars: Vec<char> = tag.chars().collect();
    let mut i = 0;
    // Step over the element name.
    while i < chars.len() && !chars[i].is_whitespace() {
        i += 1;
    }
    while i < chars.len() {
        while i < chars.len() && (chars[i].is_whitespace() || chars[i] == '/') {
            i += 1;
        }
        let start = i;
        while i < chars.len()
            && !chars[i].is_whitespace()
            && chars[i] != '='
            && chars[i] != '/'
        {
            i += 1;
        }
        if start == i {
            break;
        }
        let key: String = chars[start..i].iter().collect();
        while i < chars.len() && chars[i].is_whitespace() {
            i += 1;
        }
        if i >= chars.len() || chars[i] != '=' {
            continue; // A valueless attribute (`hidden`); try the next one.
        }
        i += 1;
        while i < chars.len() && chars[i].is_whitespace() {
            i += 1;
        }
        let value: String = if i < chars.len() && (chars[i] == '"' || chars[i] == '\'') {
            let quote = chars[i];
            i += 1;
            let from = i;
            while i < chars.len() && chars[i] != quote {
                i += 1;
            }
            let value = chars[from..i].iter().collect();
            if i < chars.len() {
                i += 1;
            }
            value
        } else {
            let from = i;
            while i < chars.len() && !chars[i].is_whitespace() {
                i += 1;
            }
            chars[from..i].iter().collect()
        };
        let local = key.rsplit(':').next().unwrap_or(&key);
        if local.eq_ignore_ascii_case(want) {
            return Some(decode_entities(&value));
        }
    }
    None
}

/// Decode every entity in a string (attribute values arrive escaped).
fn decode_entities(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() {
        let (ch, len) = next_char(&s[i..]);
        if ch == '&' {
            let (decoded, consumed) = decode_entity(&s[i..]);
            out.push(decoded);
            i += consumed;
            continue;
        }
        out.push(ch);
        i += len;
    }
    out
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
        "img" => "img",
        "image" => "image",
        "figure" => "figure",
        "figcaption" => "figcaption",
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
    sink: Sink,
    fallback_title: &mut String,
    heading_buf: &mut String,
    caption: &mut String,
    body: &mut String,
) {
    match sink {
        Sink::DocTitle => fallback_title.push(ch),
        Sink::Heading => heading_buf.push(ch),
        Sink::Caption => caption.push(ch),
        Sink::Body => body.push(ch),
        Sink::Drop => {}
    }
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
    use super::super::blocks::BlockKind;
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
    <item id="ch3" href="pages/ch3.xhtml" media-type="application/xhtml+xml"/>
    <item id="ch4" href="ch4.xhtml" media-type="application/xhtml+xml"/>
    <item id="page01" href="pages/img/page01.jpg" media-type="image/jpeg"/>
    <item id="plate" href="img/plate.png" media-type="image/png"/>
    <item id="cover" href="cover.jpg" media-type="image/jpeg"/>
  </manifest>
  <spine>
    <itemref idref="nav"/>
    <itemref idref="ch1"/>
    <itemref idref="ch2"/>
    <itemref idref="ch3"/>
    <itemref idref="ch4"/>
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
        // A fixed-layout page: the whole chapter is one image inside an <svg>,
        // referenced relative to the page's own directory.
        zip.start_file("OEBPS/pages/ch3.xhtml", opt).unwrap();
        zip.write_all(
            br#"<html xmlns="http://www.w3.org/1999/xhtml"><head><title>p1</title></head><body>
<div class="page"><svg xmlns:xlink="http://www.w3.org/1999/xlink" viewBox="0 0 800 1200">
<title>page 1</title><image width="800" height="1200" xlink:href="img/page01.jpg"/>
</svg></div></body></html>"#,
        )
        .unwrap();
        // A mixed page: prose, an illustration with a caption, then more prose.
        zip.start_file("OEBPS/ch4.xhtml", opt).unwrap();
        zip.write_all(
            br#"<html><body><h1>Chapter 4</h1><p>Before the plate.</p>
<figure><img src="img/plate.png" alt="x"/><figcaption>Fig. 1 &mdash; a plate.</figcaption></figure>
<p>After the plate.</p></body></html>"#,
        )
        .unwrap();
        zip.start_file("OEBPS/pages/img/page01.jpg", opt).unwrap();
        zip.write_all(&[0xFF, 0xD8, 0x01, 0x02, 0xFF, 0xD9]).unwrap();
        zip.start_file("OEBPS/img/plate.png", opt).unwrap();
        zip.write_all(&[0x89, b'P', b'N', b'G', 0x03]).unwrap();
        zip.start_file("OEBPS/cover.jpg", opt).unwrap();
        zip.write_all(&[0xFF, 0xD8, 0xFF, 0xD9]).unwrap();
        let cursor = zip.finish().unwrap();
        cursor.into_inner()
    }

    /// Text of a page, the way it used to be returned before blocks.
    fn page_text(html: &str) -> (String, String) {
        let (title, raw) = html_to_blocks(html);
        let blocks: Vec<Block> = raw
            .into_iter()
            .map(|b| match b {
                RawBlock::Text(t) => Block::text(t),
                RawBlock::Caption(t) => Block::caption(t),
                RawBlock::Image(href) => Block::image(href),
            })
            .collect();
        (title, derive_text(&blocks))
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
        assert_eq!(book.chapters.len(), 4);
        assert_eq!(book.chapters[0].title, "Chapter 1 The Start");
        assert!(book.chapters[0].body.contains("Once upon a time."));
        assert!(!book.chapters[0].body.contains("Chapter 1 The Start"));
        assert_eq!(book.chapters[1].title, "Chapter 2");
        assert!(book.chapters[1].body.contains("The & next day…"));
        assert!(book.cover.is_some());
    }

    /// The page that used to import as an empty chapter: an `<svg><image/>`
    /// wrapper with no prose at all.
    #[test]
    fn an_image_only_page_becomes_an_image_chapter() {
        let zip = ZipArchive::new(Cursor::new(sample_epub_bytes())).unwrap();
        let book = load_archive(zip).unwrap();

        let blocks = book
            .blocks
            .iter()
            .find(|b| b.chapter_index == 3)
            .expect("blocks for the fixed-layout page");
        assert_eq!(blocks.kind, ChapterKind::Image);
        assert_eq!(blocks.blocks.len(), 1);
        assert_eq!(blocks.blocks[0].kind, BlockKind::Image);

        // The body is the marker, so the chapter is no longer blank.
        let id = blocks.blocks[0].asset_id.clone().unwrap();
        assert_eq!(book.chapters[2].body, format!("[[img:{id}]]"));
        // The href resolved against the page's directory, not the OPF's.
        let asset = book.assets.iter().find(|a| a.id == id).unwrap();
        assert_eq!(asset.href, "OEBPS/pages/img/page01.jpg");
        assert_eq!(asset.content_type, "image/jpeg");
    }

    #[test]
    fn a_mixed_page_keeps_prose_picture_and_caption_in_order() {
        let zip = ZipArchive::new(Cursor::new(sample_epub_bytes())).unwrap();
        let book = load_archive(zip).unwrap();

        let blocks = book
            .blocks
            .iter()
            .find(|b| b.chapter_index == 4)
            .expect("blocks for the illustrated page");
        assert_eq!(blocks.kind, ChapterKind::Mixed);
        let kinds: Vec<BlockKind> = blocks.blocks.iter().map(|b| b.kind).collect();
        assert_eq!(
            kinds,
            vec![
                BlockKind::Text,
                BlockKind::Image,
                BlockKind::Caption,
                BlockKind::Text
            ]
        );
        assert_eq!(blocks.blocks[2].text, "Fig. 1 — a plate.");

        let body = &book.chapters[3].body;
        assert!(body.starts_with("Before the plate."));
        assert!(body.ends_with("After the plate."));
        assert_eq!(body.lines().filter(|line| line.starts_with("[[img:")).count(), 1);
    }

    /// Prose chapters cost no block rows: their only block is the body itself.
    #[test]
    fn prose_chapters_store_no_blocks() {
        let zip = ZipArchive::new(Cursor::new(sample_epub_bytes())).unwrap();
        let book = load_archive(zip).unwrap();
        assert!(book.blocks.iter().all(|b| b.chapter_index > 2));
    }

    /// Two references to the same bytes are one asset, which is what keeps a
    /// manga volume from being unpacked twice.
    #[test]
    fn identical_images_collapse_into_one_asset() {
        let (_, raw) = html_to_blocks(
            r#"<body><img src="a.png"/><p>x</p><img src="a.png"/></body>"#,
        );
        let images: Vec<&RawBlock> = raw
            .iter()
            .filter(|b| matches!(b, RawBlock::Image(_)))
            .collect();
        assert_eq!(images.len(), 2, "both references are kept as blocks");

        let zip = ZipArchive::new(Cursor::new(sample_epub_bytes())).unwrap();
        let book = load_archive(zip).unwrap();
        let ids: HashSet<&String> = book.assets.iter().map(|a| &a.id).collect();
        assert_eq!(ids.len(), book.assets.len(), "asset ids are unique");
    }

    #[test]
    fn html_heading_is_title_not_duplicated() {
        let (title, body) = page_text(
            "<html><body><h1>Chapter 1 The Start</h1><p>Once upon a time.</p></body></html>",
        );
        assert_eq!(title, "Chapter 1 The Start");
        assert_eq!(body, "Once upon a time.");
    }

    #[test]
    fn html_decodes_entities() {
        let (_, body) = page_text("<p>A &amp; B&nbsp;C&hellip;</p>");
        assert!(body.contains("A & B"));
        assert!(body.contains('…'));
    }

    #[test]
    fn tag_attr_reads_namespaced_quoted_and_escaped_values() {
        assert_eq!(
            tag_attr(r#"image xlink:href="img/a&amp;b.jpg" width="8""#, "href").as_deref(),
            Some("img/a&b.jpg")
        );
        assert_eq!(
            tag_attr(r#"img alt='x' src=plain.png /"#, "src").as_deref(),
            Some("plain.png")
        );
        // A valueless attribute must not swallow the one we are after.
        assert_eq!(
            tag_attr(r#"img hidden src="a.png""#, "src").as_deref(),
            Some("a.png")
        );
        assert_eq!(tag_attr("img alt=\"x\"", "src"), None);
    }

    /// An inline data URI is not an asset, and a missing file is not a chapter
    /// killer.
    #[test]
    fn unusable_image_references_are_dropped() {
        let (_, raw) = html_to_blocks(r#"<body><img src="data:image/png;base64,AA"/><p>x</p></body>"#);
        assert!(!raw.iter().any(|b| matches!(b, RawBlock::Image(_))));

        let mut zip = ZipArchive::new(Cursor::new(sample_epub_bytes())).unwrap();
        let mut assets = Vec::new();
        let mut seen = HashSet::new();
        assert_eq!(
            register_asset(&mut zip, "OEBPS/img/missing.png", &mut assets, &mut seen),
            None
        );
        assert!(assets.is_empty());
    }
}
