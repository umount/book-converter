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
        let text = super::pdf::text(path);
        return Ok(DecodedText {
            text,
            encoding: "PDF",
            had_errors: false,
        });
    }

    let bytes = if is_zip(path) {
        read_first_book_from_zip(path)?
    } else {
        std::fs::read(path)?
    };
    Ok(decode_book_bytes(&bytes))
}

fn has_ext(path: &Path, ext: &str) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case(ext))
        .unwrap_or(false)
}

/// Decode `%XX` escapes. Hrefs inside an EPUB are URLs (`ch%202.xhtml`), and so
/// are the asset URLs the webview asks for.
pub fn percent_decode(s: &str) -> String {
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

/// MIME type of an image, from its file name.
pub fn image_mime(name: &str) -> String {
    let n = name.to_ascii_lowercase();
    if n.ends_with(".png") {
        "image/png".into()
    } else if n.ends_with(".gif") {
        "image/gif".into()
    } else if n.ends_with(".webp") {
        "image/webp".into()
    } else if n.ends_with(".svg") {
        "image/svg+xml".into()
    } else {
        "image/jpeg".into()
    }
}

/// True if `path` looks like a zip archive (by extension).
pub fn is_zip(path: &Path) -> bool {
    has_ext(path, "zip")
}

/// True if `path` is an EPUB (a zip of XHTML, not a text file).
pub fn is_epub(path: &Path) -> bool {
    has_ext(path, "epub")
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
        assert!(
            std::str::from_utf8(&bytes).is_err(),
            "sample must not be valid UTF-8"
        );
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
