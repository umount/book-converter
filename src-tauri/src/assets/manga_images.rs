//! Canonical page orientation and small lossless list previews, generated once at import.
use image::{metadata::Orientation, ImageDecoder, ImageFormat};
use std::io::Cursor;
pub struct PreparedPage {
    pub original: Vec<u8>,
    pub extension: &'static str,
    pub thumbnail: Vec<u8>,
    pub width: u32,
    pub height: u32,
}
pub fn prepare(bytes: &[u8]) -> anyhow::Result<PreparedPage> {
    let format = image::guess_format(bytes)?;
    let extension = match format {
        ImageFormat::Png => "png",
        ImageFormat::Jpeg => "jpg",
        ImageFormat::Gif => "gif",
        ImageFormat::WebP => "webp",
        _ => anyhow::bail!("Unsupported image"),
    };
    let mut reader = image::ImageReader::with_format(Cursor::new(bytes), format);
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(16000);
    limits.max_image_height = Some(16000);
    limits.max_alloc = Some(128 * 1024 * 1024);
    reader.limits(limits);
    let mut decoder = reader.into_decoder()?;
    let (width, height) = decoder.dimensions();
    anyhow::ensure!(
        width > 0 && height > 0 && u64::from(width) * u64::from(height) <= 32_000_000,
        "Page dimensions exceed decoding budget"
    );
    anyhow::ensure!(
        decoder.total_bytes() <= 128 * 1024 * 1024,
        "Decoded page exceeds allocation budget"
    );
    let orientation = decoder.orientation()?;
    let mut decoded = image::DynamicImage::from_decoder(decoder)?;
    decoded.apply_orientation(orientation);
    let (width, height) = (decoded.width(), decoded.height());
    let mut thumbnail = Cursor::new(Vec::new());
    decoded
        .thumbnail(200, 240)
        .write_to(&mut thumbnail, ImageFormat::Png)?;
    let (original, extension) = if orientation != Orientation::NoTransforms {
        let mut normalized = Cursor::new(Vec::new());
        decoded.write_to(&mut normalized, ImageFormat::Png)?;
        (normalized.into_inner(), "png")
    } else {
        (bytes.to_vec(), extension)
    };
    Ok(PreparedPage {
        original,
        extension,
        thumbnail: thumbnail.into_inner(),
        width,
        height,
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn normalizes_exif_without_recompressing_unrotated_originals() {
        let mut jpeg = Cursor::new(Vec::new());
        image::RgbImage::new(400, 200)
            .write_to(&mut jpeg, ImageFormat::Jpeg)
            .unwrap();
        let plain = jpeg.into_inner();
        assert_eq!(prepare(&plain).unwrap().original, plain);
        // Minimal little-endian EXIF IFD containing orientation=6 (clockwise 90).
        let exif = b"Exif\0\0II\x2a\0\x08\0\0\0\x01\0\x12\x01\x03\0\x01\0\0\0\x06\0\0\0\0\0\0\0";
        let mut rotated = vec![0xff, 0xd8, 0xff, 0xe1];
        rotated.extend_from_slice(&((exif.len() + 2) as u16).to_be_bytes());
        rotated.extend_from_slice(exif);
        rotated.extend_from_slice(&plain[2..]);
        let prepared = prepare(&rotated).unwrap();
        assert_eq!((prepared.width, prepared.height), (200, 400));
        assert_eq!(prepared.extension, "png");
        let thumbnail = image::load_from_memory(&prepared.thumbnail).unwrap();
        assert_eq!((thumbnail.width(), thumbnail.height()), (120, 240));
        let reimported = prepare(&prepared.original).unwrap();
        assert_eq!((reimported.width, reimported.height), (200, 400));
        assert_eq!(reimported.original, prepared.original);
        assert!(prepare(b"invalid image").is_err());
    }
}
