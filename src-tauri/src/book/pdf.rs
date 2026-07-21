//! PDF-specific handling: text extraction, cover art, and table-of-contents based
//! chaptering.
//!
//! PDF text is hard: fonts are often subsetted with custom encodings. Poppler's
//! `pdftotext` decodes them correctly and is used when available; the pure-Rust
//! `pdf-extract` / `lopdf` are the fallback (with a best-effort cipher recovery for
//! the common uniform-shift mis-encoding). A quality gate stops mis-decoded, word-
//! space-less text from becoming garbled chapters.

use std::collections::HashMap;
use std::path::Path;
use std::process::Command;

use lopdf::{Dictionary, Document, Object, ObjectId};

/// Extract a PDF's full text: Poppler (`pdftotext`) first, then the pure-Rust
/// `pdf-extract` as a fallback. May be empty if the PDF has no extractable text.
pub fn text(path: &Path) -> String {
    if let Some(t) = pdftotext(path, None, None) {
        return t;
    }
    std::fs::read(path)
        .ok()
        .and_then(|b| pdf_extract::extract_text_from_mem(&b).ok())
        .unwrap_or_default()
}

/// Best-effort cover extraction: the largest JPEG (DCTDecode) image on the first
/// page, which is almost always the cover art. `(content_type, jpeg_bytes)`.
pub fn cover(bytes: &[u8]) -> Option<(String, Vec<u8>)> {
    let doc = Document::load_mem(bytes).ok()?;
    let pages = doc.get_pages();
    let (_, &page_id) = pages.iter().next()?; // first page
    let page = doc.get_object(page_id).ok()?.as_dict().ok()?;
    let resources = resolve(&doc, page.get(b"Resources").ok()?)?;
    let xobjects = resolve(&doc, resources.get(b"XObject").ok()?)?;

    let mut best: Option<(usize, Vec<u8>)> = None;
    for (_, v) in xobjects.iter() {
        let id = match v {
            Object::Reference(id) => *id,
            _ => continue,
        };
        if let Ok(Object::Stream(stream)) = doc.get_object(id) {
            let d = &stream.dict;
            let is_image = d
                .get(b"Subtype")
                .ok()
                .and_then(|o| o.as_name().ok())
                .map(|n| n == b"Image")
                .unwrap_or(false);
            if !is_image || !is_dct(d) {
                continue;
            }
            let size = stream.content.len();
            if best.as_ref().map_or(true, |(s, _)| size > *s) {
                best = Some((size, stream.content.clone()));
            }
        }
    }
    best.map(|(_, jpeg)| ("image/jpeg".to_string(), jpeg))
}

/// Extract chapters from a PDF's table of contents (outline / bookmarks): far more
/// accurate than a regex over the flattened text. Each top-level bookmark becomes a
/// chapter, its body the text of the pages up to the next one. Returns `None` when
/// there is no usable outline, or when the extracted text is mostly mis-decoded.
pub fn toc_chapters(path: &Path) -> Option<Vec<(String, String)>> {
    let bytes = std::fs::read(path).ok()?;
    let doc = Document::load_mem(&bytes).ok()?;
    let pages = doc.get_pages();
    let total_pages = pages.len() as u32;
    let page_num: HashMap<ObjectId, u32> = pages.iter().map(|(n, id)| (*id, *n)).collect();

    let root = doc.trailer.get(b"Root").ok()?.as_reference().ok()?;
    let catalog = doc.get_object(root).ok()?.as_dict().ok()?;
    let outlines = resolve(&doc, catalog.get(b"Outlines").ok()?)?;

    // Walk the top-level bookmark siblings (First -> Next -> …).
    let mut items: Vec<(String, u32)> = Vec::new();
    let mut cur = outlines.get(b"First").ok().and_then(|o| o.as_reference().ok());
    let mut guard = 0;
    while let Some(id) = cur {
        guard += 1;
        if guard > 5000 {
            break;
        }
        let item = match doc.get_object(id).ok().and_then(|o| o.as_dict().ok()) {
            Some(d) => d,
            None => break,
        };
        let title = item.get(b"Title").ok().and_then(pdf_string).unwrap_or_default();
        if let Some(page) = dest_page(&doc, item, &page_num) {
            let title = title.trim().to_string();
            if !title.is_empty() {
                items.push((title, page));
            }
        }
        cur = item.get(b"Next").ok().and_then(|o| o.as_reference().ok());
    }
    if items.len() < 2 {
        return None;
    }

    let mut chapters = Vec::new();
    for (i, (title, start)) in items.iter().enumerate() {
        let end = items.get(i + 1).map(|(_, p)| *p).unwrap_or(total_pages + 1);
        let last = (end.saturating_sub(1)).max(*start);
        // Prefer Poppler; fall back to lopdf with cipher recovery.
        let body = pdftotext(path, Some(*start), Some(last))
            .filter(|t| looks_like_text(t))
            .or_else(|| {
                let range: Vec<u32> = (*start..end.max(*start + 1)).collect();
                recover_text(&doc.extract_text(&range).unwrap_or_default())
            })
            .unwrap_or_default();
        chapters.push((title.clone(), body.trim().to_string()));
    }

    // If most bodies are mis-decoded, the split is not trustworthy.
    let ok = chapters.iter().filter(|(_, b)| looks_like_text(b)).count();
    if ok * 2 < chapters.len() {
        return None;
    }
    Some(chapters)
}

/// Heuristic: does this look like natural-language text (not empty, not a mis-decoded
/// font)? Requires a reasonable letter ratio and some word spaces.
pub fn looks_like_text(s: &str) -> bool {
    let sample: Vec<char> = s.chars().take(4000).collect();
    let total = sample.iter().filter(|c| !c.is_control()).count();
    if total < 30 {
        return false;
    }
    let spaces = sample.iter().filter(|c| **c == ' ').count();
    let letters = sample.iter().filter(|c| c.is_alphabetic()).count();
    (spaces as f64 / total as f64) > 0.03 && (letters as f64 / total as f64) > 0.5
}

// --- internals ---

/// Run Poppler's `pdftotext` for a 1-based inclusive page range, if available.
fn pdftotext(path: &Path, first: Option<u32>, last: Option<u32>) -> Option<String> {
    let mut cmd = Command::new("pdftotext");
    cmd.arg("-q");
    if let Some(f) = first {
        cmd.arg("-f").arg(f.to_string());
    }
    if let Some(l) = last {
        cmd.arg("-l").arg(l.to_string());
    }
    cmd.arg(path).arg("-"); // write to stdout
    let out = cmd.output().ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    (!text.trim().is_empty()).then_some(text)
}

/// Recover text mis-decoded by a uniform single-byte font offset (e.g. "about" ->
/// "DERXW"): find the byte shift that makes it look like real text. `None` if none.
fn recover_text(s: &str) -> Option<String> {
    if looks_like_text(s) {
        return Some(s.to_string());
    }
    for shift in 1u32..=255 {
        let cand: String = s
            .chars()
            .map(|c| {
                let v = c as u32;
                if v < 256 {
                    char::from_u32((v + shift) & 0xFF).unwrap_or(c)
                } else {
                    c
                }
            })
            .collect();
        if looks_like_text(&cand) {
            return Some(cand);
        }
    }
    None
}

fn pdf_string(obj: &Object) -> Option<String> {
    let raw = obj.as_str().ok()?;
    if raw.starts_with(&[0xFE, 0xFF]) {
        // UTF-16BE with BOM.
        let u16s: Vec<u16> = raw[2..]
            .chunks_exact(2)
            .map(|c| u16::from_be_bytes([c[0], c[1]]))
            .collect();
        Some(String::from_utf16_lossy(&u16s))
    } else {
        Some(raw.iter().map(|&b| b as char).collect())
    }
}

fn resolve<'a>(doc: &'a Document, obj: &'a Object) -> Option<&'a Dictionary> {
    match obj {
        Object::Dictionary(d) => Some(d),
        Object::Reference(id) => doc.get_object(*id).ok()?.as_dict().ok(),
        _ => None,
    }
}

fn is_dct(d: &Dictionary) -> bool {
    match d.get(b"Filter") {
        Ok(Object::Name(n)) => n == b"DCTDecode",
        Ok(Object::Array(a)) => a
            .iter()
            .any(|o| matches!(o.as_name(), Ok(n) if n == b"DCTDecode")),
        _ => false,
    }
}

/// The page an outline item points at, as a 1-based page number.
fn dest_page(doc: &Document, item: &Dictionary, page_num: &HashMap<ObjectId, u32>) -> Option<u32> {
    let dest_obj = item
        .get(b"Dest")
        .ok()
        .cloned()
        .or_else(|| resolve(doc, item.get(b"A").ok()?)?.get(b"D").ok().cloned())?;
    let dest = match &dest_obj {
        Object::Reference(id) => doc.get_object(*id).ok()?.clone(),
        other => other.clone(),
    };
    let arr = dest.as_array().ok()?;
    let page_ref = arr.first()?.as_reference().ok()?;
    page_num.get(&page_ref).copied()
}
