//! Reading a book file and decoding it to UTF-8.
//!
//! Chinese `.txt` novels come in several encodings — UTF-8, but very often
//! GBK / GB18030 (simplified) or Big5 (traditional). We can't assume UTF-8:
//! one of the sample books is GB18030. This layer detects the encoding and
//! decodes to a Rust `String`, reporting what it found so the UI can surface it.
//!
//! Pipeline: BOM → valid UTF-8 → `chardetng` guess (decoded via `encoding_rs`).
//! Self-contained (only `encoding_rs` + `chardetng` + std) so it stays testable
//! without the Tauri crate.

use std::path::Path;

use chardetng::EncodingDetector;
use encoding_rs::{Encoding, UTF_8};

/// A decoded book plus what the detector concluded.
#[derive(Debug, Clone)]
pub struct DecodedText {
    /// The decoded text (UTF-8).
    pub text: String,
    /// Name of the encoding used, e.g. "UTF-8", "GBK", "gb18030", "Big5".
    pub encoding: &'static str,
    /// True if decoding hit bytes that mapped to the replacement char — a hint
    /// that detection was wrong or the file is corrupt.
    pub had_errors: bool,
}

/// Read a book file from disk and decode it to UTF-8.
///
/// A `.zip` input is transparently unpacked: the first `.fb2`/`.txt` entry (or,
/// failing that, the first file) is read from the archive.
pub fn read_book_file(path: &Path) -> std::io::Result<DecodedText> {
    // PDF: extract text directly (not a byte-encoded text file).
    if has_ext(path, "pdf") {
        let bytes = std::fs::read(path)?;
        let text = pdf_extract::extract_text_from_mem(&bytes)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()))?;
        return Ok(DecodedText { text, encoding: "PDF", had_errors: false });
    }

    let bytes = if is_zip(path) {
        read_first_book_from_zip(path)?
    } else {
        std::fs::read(path)?
    };
    Ok(decode_book_bytes(&bytes))
}

/// Best-effort cover extraction from a PDF: the largest JPEG (DCTDecode) image on
/// the first page, which is almost always the cover art. Returns `(content_type,
/// jpeg_bytes)`. `None` if the first page has no embedded JPEG.
pub fn extract_pdf_cover(bytes: &[u8]) -> Option<(String, Vec<u8>)> {
    use lopdf::{Dictionary, Document, Object};

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

/// Extract chapters from a PDF's table of contents (its outline / bookmarks): far
/// more accurate than a regex over the flattened text when the PDF has an outline.
/// Each top-level bookmark becomes a chapter, its body being the text of the pages
/// from that bookmark up to the next one. Returns `None` if there is no usable
/// outline (fewer than 2 resolvable entries).
pub fn extract_pdf_toc_chapters(bytes: &[u8]) -> Option<Vec<(String, String)>> {
    use std::collections::HashMap;

    use lopdf::{Dictionary, Document, Object, ObjectId};

    fn pdf_string(obj: &Object) -> Option<String> {
        let raw = obj.as_str().ok()?;
        // UTF-16BE (with BOM) or PDFDocEncoding/Latin-1-ish.
        if raw.starts_with(&[0xFE, 0xFF]) {
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

    // The page an outline item points at, as a 1-based page number.
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

    let doc = Document::load_mem(bytes).ok()?;
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
        let page_range: Vec<u32> = (*start..end.max(*start + 1)).collect();
        let body = doc.extract_text(&page_range).unwrap_or_default();
        chapters.push((title.clone(), body.trim().to_string()));
    }
    Some(chapters)
}

fn has_ext(path: &Path, ext: &str) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case(ext))
        .unwrap_or(false)
}

/// True if `path` looks like a zip archive (by extension).
pub fn is_zip(path: &Path) -> bool {
    has_ext(path, "zip")
}

/// Read the bytes of the first book entry inside a zip archive.
fn read_first_book_from_zip(path: &Path) -> std::io::Result<Vec<u8>> {
    use std::io::Read;

    let file = std::fs::File::open(path)?;
    let mut zip = zip::ZipArchive::new(file)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

    // Prefer a .fb2/.txt entry; fall back to the first regular file.
    let mut fallback: Option<usize> = None;
    let mut chosen: Option<usize> = None;
    for i in 0..zip.len() {
        let entry = zip
            .by_index(i)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        if !entry.is_file() {
            continue;
        }
        let name = entry.name().to_ascii_lowercase();
        if name.ends_with(".fb2") || name.ends_with(".txt") {
            chosen = Some(i);
            break;
        }
        fallback.get_or_insert(i);
    }

    let idx = chosen
        .or(fallback)
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "empty zip archive"))?;
    let mut entry = zip
        .by_index(idx)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    let mut buf = Vec::new();
    entry.read_to_end(&mut buf)?;
    Ok(buf)
}

/// Detect the encoding of `bytes` and decode to UTF-8.
pub fn decode_book_bytes(bytes: &[u8]) -> DecodedText {
    let encoding = detect_encoding(bytes);
    // `decode` sniffs and strips a BOM itself and reports the effective encoding.
    let (text, actual, had_errors) = encoding.decode(bytes);
    DecodedText {
        text: text.into_owned(),
        encoding: actual.name(),
        had_errors,
    }
}

/// Pick the most likely encoding for `bytes`.
///
/// Order: an explicit BOM wins; otherwise valid UTF-8 is trusted; otherwise the
/// `chardetng` statistical detector decides (it covers GBK, Big5, Shift_JIS,
/// EUC-KR, the Windows code pages, …).
pub fn detect_encoding(bytes: &[u8]) -> &'static Encoding {
    if let Some((enc, _)) = Encoding::for_bom(bytes) {
        return enc;
    }
    if std::str::from_utf8(bytes).is_ok() {
        return UTF_8;
    }
    let mut detector = EncodingDetector::new();
    detector.feed(bytes, true);
    // allow_utf8 = true: let it still return UTF-8 if that's the best guess.
    detector.guess(None, true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use encoding_rs::{BIG5, GB18030};

    // A paragraph, long enough for the detector to be confident.
    const SAMPLE: &str = "第一章 灾变前夕的最后宁静。这是一个关于末世降临之后，\
         黑暗召唤师在废墟之上寻找生路的故事。风暴来袭，天地变色，众生挣扎求存。";

    #[test]
    fn detects_utf8() {
        let decoded = decode_book_bytes(SAMPLE.as_bytes());
        assert_eq!(decoded.text, SAMPLE);
        assert_eq!(decoded.encoding, "UTF-8");
        assert!(!decoded.had_errors);
    }

    #[test]
    fn decodes_gb18030() {
        let (bytes, _, _) = GB18030.encode(SAMPLE);
        assert!(std::str::from_utf8(&bytes).is_err(), "sample must not be valid UTF-8");
        let decoded = decode_book_bytes(&bytes);
        assert_eq!(decoded.text, SAMPLE);
        assert!(!decoded.had_errors);
    }

    #[test]
    fn decodes_big5() {
        // Traditional-Chinese sample so Big5 can encode it.
        let traditional = "第一章 風暴來襲，天地變色，眾生掙扎求存，尋找生路。";
        let (bytes, _, _) = BIG5.encode(traditional);
        let decoded = decode_book_bytes(&bytes);
        assert_eq!(decoded.text, traditional);
    }

    #[test]
    fn strips_utf8_bom() {
        let mut bytes = vec![0xEF, 0xBB, 0xBF];
        bytes.extend_from_slice("第一章".as_bytes());
        let decoded = decode_book_bytes(&bytes);
        assert_eq!(decoded.text, "第一章"); // no leading U+FEFF
    }
}
