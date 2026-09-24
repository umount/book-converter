//! Export to EPUB: one XHTML document per chapter + a table of contents.

use std::collections::HashMap;
use std::path::Path;

use anyhow::{Context, Result};
use epub_builder::{EpubBuilder, EpubContent, ReferenceType, ZipLibrary};

use super::{OutputMeta, Piece, TranslatedChapter};

/// Where embedded pictures live inside the `.epub`, relative to the content
/// documents (which sit in the same directory).
const IMAGE_DIR: &str = "images";

/// Assemble the book into an `.epub` with a per-chapter TOC.
pub fn export(chapters: &[TranslatedChapter], meta: &OutputMeta, out_path: &Path) -> Result<()> {
    let zip = ZipLibrary::new().map_err(|e| anyhow::anyhow!("epub zip backend: {e}"))?;
    let mut builder = EpubBuilder::new(zip).map_err(|e| anyhow::anyhow!("epub builder: {e}"))?;

    builder
        .metadata("title", &meta.title)
        .and_then(|b| b.metadata("author", &meta.author))
        .and_then(|b| b.metadata("lang", &meta.lang))
        .map_err(|e| anyhow::anyhow!("epub metadata: {e}"))?;

    // Cover image, if any (decode the stored base64).
    if let Some(cover) = &meta.cover {
        use base64::Engine as _;
        if let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(cover.base64.trim()) {
            let name = format!("cover.{}", cover_ext(&cover.content_type));
            builder
                .add_cover_image(&name, bytes.as_slice(), &cover.content_type)
                .map_err(|e| anyhow::anyhow!("epub cover: {e}"))?;
        }
    }

    // Pictures the chapters point at. Added before the content documents so a
    // chapter that is nothing but a page image (manga) has something to show.
    let embedded = embed_images(&mut builder, chapters, meta);

    builder.inline_toc();

    for ch in chapters {
        let xhtml = chapter_xhtml(ch, &embedded);
        let title = ch.title.trim();
        builder
            .add_content(
                EpubContent::new(format!("chapter_{}.xhtml", ch.index), xhtml.as_bytes())
                    .title(title)
                    .reftype(ReferenceType::Text),
            )
            .map_err(|e| anyhow::anyhow!("epub add chapter {}: {e}", ch.index))?;
    }

    let mut file = std::fs::File::create(out_path)
        .with_context(|| format!("creating {}", out_path.display()))?;
    builder
        .generate(&mut file)
        .map_err(|e| anyhow::anyhow!("epub generate: {e}"))?;
    Ok(())
}

/// Copy each referenced picture into the book once, and report the file name it
/// got. A picture that cannot be read is left out rather than failing the export.
fn embed_images(
    builder: &mut EpubBuilder<ZipLibrary>,
    chapters: &[TranslatedChapter],
    meta: &OutputMeta,
) -> HashMap<String, String> {
    let mut names = HashMap::new();
    if meta.images.is_empty() {
        return names;
    }
    for ch in chapters {
        for piece in ch.body.pieces() {
            let Piece::Image(id) = piece else { continue };
            if names.contains_key(id) {
                continue;
            }
            let Some(image) = meta.images.get(id) else {
                continue;
            };
            let Ok(bytes) = std::fs::read(&image.path) else {
                tracing::warn!(path = %image.path.display(), "epub export: image unreadable");
                continue;
            };
            let name = format!("{IMAGE_DIR}/{id}.{}", cover_ext(&image.content_type));
            match builder.add_resource(&name, bytes.as_slice(), &image.content_type) {
                Ok(_) => {
                    names.insert(id.to_string(), name);
                }
                Err(e) => tracing::warn!("epub export: adding image {id} failed: {e}"),
            }
        }
    }
    names
}

/// Render one chapter as a standalone XHTML document.
fn chapter_xhtml(ch: &TranslatedChapter, images: &HashMap<String, String>) -> String {
    let mut body = String::new();
    let title = ch.title.trim();
    if !title.is_empty() {
        body.push_str(&format!("<h1>{}</h1>\n", esc(title)));
    }
    for piece in ch.body.pieces() {
        match piece {
            Piece::Para(text) => body.push_str(&format!("<p>{}</p>\n", esc(text))),
            Piece::Image(id) => {
                if let Some(name) = images.get(id) {
                    body.push_str(&format!(
                        "<p class=\"image\"><img src=\"{}\" alt=\"\"/></p>\n",
                        esc(name)
                    ));
                }
            }
        }
    }
    format!(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n\
         <!DOCTYPE html>\n\
         <html xmlns=\"http://www.w3.org/1999/xhtml\">\n\
         <head><title>{}</title></head>\n<body>\n{}</body>\n</html>\n",
        esc(title),
        body,
    )
}

/// File extension for an image MIME type.
fn cover_ext(content_type: &str) -> &str {
    match content_type {
        "image/png" => "png",
        "image/gif" => "gif",
        "image/webp" => "webp",
        _ => "jpg",
    }
}

/// Escape XML/XHTML text content.
fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read as _;

    use crate::export::ExportImage;

    /// A page of pictures must arrive in the reader's hands as a picture: the
    /// file itself inside the book, and an `<img>` where the marker was.
    #[test]
    fn a_picture_page_is_embedded_as_an_image() {
        let dir = std::env::temp_dir().join(format!("bc_epub_out_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let png = dir.join("page.png");
        std::fs::write(&png, b"\x89PNG-not-really").unwrap();

        let chapters = vec![TranslatedChapter {
            index: 1,
            number: Some(1),
            title: "Страница 1".into(),
            body: "[[img:ab12]]".into(),
        }];
        let meta = OutputMeta {
            images: HashMap::from([(
                "ab12".to_string(),
                ExportImage {
                    path: png,
                    content_type: "image/png".into(),
                },
            )]),
            ..OutputMeta::default()
        };
        let out = dir.join("book.epub");
        export(&chapters, &meta, &out).unwrap();

        let mut zip = zip::ZipArchive::new(std::fs::File::open(&out).unwrap()).unwrap();
        let names: Vec<String> = zip.file_names().map(str::to_string).collect();
        assert!(
            names.iter().any(|n| n.ends_with("images/ab12.png")),
            "the picture itself must be in the book: {names:?}"
        );
        let mut xhtml = String::new();
        let entry = names
            .iter()
            .find(|n| n.ends_with("chapter_1.xhtml"))
            .expect("chapter document");
        zip.by_name(entry)
            .unwrap()
            .read_to_string(&mut xhtml)
            .unwrap();
        assert!(xhtml.contains("<img src=\"images/ab12.png\""), "{xhtml}");
        assert!(!xhtml.contains("[[img:"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A picture the project lost costs that picture, not the whole export.
    #[test]
    fn a_missing_picture_leaves_the_chapter_exportable() {
        let dir = std::env::temp_dir().join(format!("bc_epub_miss_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let chapters = vec![TranslatedChapter {
            index: 1,
            number: Some(1),
            title: "Глава 1".into(),
            body: "До.\n\n[[img:ab12]]\n\nПосле.".into(),
        }];
        let out = dir.join("book.epub");
        export(&chapters, &OutputMeta::default(), &out).unwrap();
        assert!(out.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
