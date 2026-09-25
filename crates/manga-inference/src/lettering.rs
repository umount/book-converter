//! Horizontal Unicode shaping and bounded CPU rasterization. Unsupported glyphs
//! or overflowing text fail explicitly; no clipped or replacement-box output.
use crate::text::FONT;
pub use crate::text::{font_hash, VERSION};
use crate::{
    fit::largest_fitting_font_size,
    text::{TextLayout, TextRegion},
    validate_dimensions, Error, Result,
};
use fontdue::{Font, FontSettings};
use image::{GrayImage, RgbImage};
use std::collections::HashSet;
struct Glyph {
    id: u16,
    x: f32,
    y: f32,
}
struct Line {
    glyphs: Vec<Glyph>,
    width: f32,
}
struct Layout {
    lines: Vec<Line>,
    size: f32,
    line_height: f32,
    ascent: f32,
    fits: bool,
}
pub struct RenderedPage {
    pub image: RgbImage,
    pub layouts: Vec<TextLayout>,
}

fn shape(face: &rustybuzz::Face<'_>, font: &Font, text: &str, size: f32) -> Result<Line> {
    let mut buffer = rustybuzz::UnicodeBuffer::new();
    buffer.push_str(text);
    buffer.guess_segment_properties();
    let output = rustybuzz::shape(face, &[], buffer);
    let scale = size / face.units_per_em() as f32;
    let mut pen = 0.0;
    let mut left = 0.0f32;
    let mut right = 0.0f32;
    let mut glyphs = Vec::new();
    for (info, pos) in output.glyph_infos().iter().zip(output.glyph_positions()) {
        if info.glyph_id == 0 {
            return Err(Error::FontCoverage);
        }
        let id = u16::try_from(info.glyph_id).map_err(|_| Error::FontCoverage)?;
        let metrics = font.metrics_indexed(id, size);
        let x = pen + pos.x_offset as f32 * scale + metrics.xmin as f32;
        let y = -(pos.y_offset as f32 * scale) - metrics.ymin as f32 - metrics.height as f32;
        left = left.min(x);
        right = right.max(x + metrics.width as f32);
        glyphs.push(Glyph { id, x, y });
        pen += pos.x_advance as f32 * scale;
    }
    for glyph in &mut glyphs {
        glyph.x -= left;
    }
    Ok(Line {
        glyphs,
        width: pen.max(right) - left,
    })
}
fn layout(
    face: &rustybuzz::Face<'_>,
    font: &Font,
    text: &str,
    size: f32,
    width: f32,
    height: f32,
) -> Result<Layout> {
    let metrics = font
        .horizontal_line_metrics(size)
        .ok_or(Error::FontCoverage)?;

    let mut lines = Vec::new();
    let mut fits = true;
    for paragraph in text.split('\n') {
        if paragraph.is_empty() {
            lines.push(shape(face, font, "", size)?);
            continue;
        }
        let mut begin = 0;
        let mut last = 0;
        for (end, _) in unicode_linebreak::linebreaks(paragraph) {
            let candidate = shape(face, font, paragraph[begin..end].trim_end(), size)?;
            if candidate.width > width && last > begin {
                lines.push(shape(face, font, paragraph[begin..last].trim_end(), size)?);
                begin = last;
            }
            last = end;
        }
        if begin < paragraph.len() {
            lines.push(shape(face, font, paragraph[begin..].trim_end(), size)?);
        }
    }
    // Rasterized descenders/accents can exceed nominal font metrics at small sizes.
    // Expand the line box to actual ink instead of falsely rejecting a fitting font.
    let mut ascent = metrics.ascent;
    let mut descent = 0.0f32;
    for line in &lines {
        for glyph in &line.glyphs {
            let m = font.metrics_indexed(glyph.id, size);
            ascent = ascent.max(-glyph.y);
            descent = descent.max(glyph.y + m.height as f32);
        }
    }
    let line_height = metrics.new_line_size.max(size * 1.15).max(ascent + descent);
    if lines.len() > 64
        || lines.iter().any(|l| l.width > width)
        || line_height * lines.len() as f32 > height
    {
        fits = false;
    }
    Ok(Layout {
        lines,
        size,
        line_height,
        ascent,
        fits,
    })
}
pub fn render(original: &RgbImage, regions: &[TextRegion]) -> Result<RenderedPage> {
    validate_dimensions(original.width(), original.height())?;
    if regions.len() > 256 {
        return Err(Error::Request);
    }
    let mut ids = HashSet::new();
    for region in regions {
        region
            .bounds
            .validate(original.width(), original.height())?;
        if region.id.len() > 128
            || region.id.is_empty()
            || !ids.insert(&region.id)
            || region.text.trim().is_empty()
            || region.text.len() > 4096
            || region.bounds.width > 2048
            || region.bounds.height > 2048
        {
            return Err(Error::Request);
        }
    }
    let face = rustybuzz::Face::from_slice(FONT, 0).ok_or(Error::FontCoverage)?;
    let font = Font::from_bytes(FONT, FontSettings::default()).map_err(|_| Error::FontCoverage)?;
    let mut prepared = Vec::new();
    for region in regions {
        // Reserve two pixels on every edge for antialiasing and the white stroke.
        let (w, h) = if region.vertical {
            (region.bounds.height, region.bounds.width)
        } else {
            (region.bounds.width, region.bounds.height)
        };
        let width = w.saturating_sub(4) as f32;
        let height = h.saturating_sub(4) as f32;
        let maximum = 96.0f32.min(height).max(8.0);
        let result = largest_fitting_font_size(
            8.0,
            maximum,
            |size| layout(&face, &font, &region.text, size, width, height),
            |l| l.fits,
        )?
        .ok_or(Error::TextOverflow)?;
        prepared.push(result);
    }
    let mut image = original.clone();
    let mut layouts = Vec::new();
    for (region, layout) in regions.iter().zip(prepared) {
        let (w, h) = if region.vertical {
            (region.bounds.height, region.bounds.width)
        } else {
            (region.bounds.width, region.bounds.height)
        };
        let mut alpha = GrayImage::new(w, h);
        let top = (h as f32 - layout.line_height * layout.lines.len() as f32) / 2.0;
        for (index, line) in layout.lines.iter().enumerate() {
            let left = (w as f32 - line.width) / 2.0;
            for glyph in &line.glyphs {
                let (metrics, coverage) = font.rasterize_indexed(glyph.id, layout.size);
                let x = (left + glyph.x).round() as i32;
                let y = (top + index as f32 * layout.line_height + layout.ascent + glyph.y).round()
                    as i32;
                for dy in 0..metrics.height {
                    for dx in 0..metrics.width {
                        let value = coverage[dy * metrics.width + dx];
                        if value == 0 {
                            continue;
                        }
                        let px = x + dx as i32;
                        let py = y + dy as i32;
                        if px < 1
                            || py < 1
                            || px >= alpha.width() as i32 - 1
                            || py >= alpha.height() as i32 - 1
                        {
                            return Err(Error::TextOverflow);
                        }
                        let pixel = alpha.get_pixel_mut(px as u32, py as u32);
                        pixel[0] = pixel[0].max(value);
                    }
                }
            }
        }
        let alpha = if region.vertical {
            image::imageops::rotate90(&alpha)
        } else {
            alpha
        };
        // A one-pixel light stroke keeps black text legible on patterned backgrounds.
        for (x, y, pixel) in alpha.enumerate_pixels() {
            let mut stroke = 0;
            for dy in -1i32..=1 {
                for dx in -1i32..=1 {
                    let px = x as i32 + dx;
                    let py = y as i32 + dy;
                    if px >= 0 && py >= 0 && px < alpha.width() as i32 && py < alpha.height() as i32
                    {
                        stroke = stroke.max(alpha.get_pixel(px as u32, py as u32)[0]);
                    }
                }
            }
            let out = image.get_pixel_mut(region.bounds.x + x, region.bounds.y + y);
            for channel in &mut out.0 {
                let light = (u32::from(*channel) * (255 - u32::from(stroke))
                    + 255 * u32::from(stroke)
                    + 127)
                    / 255;
                *channel = ((light * (255 - u32::from(pixel[0])) + 127) / 255) as u8;
            }
        }
        layouts.push(TextLayout {
            id: region.id.clone(),
            font: "DejaVu Sans".into(),
            font_sha256: font_hash(),
            font_size: layout.size,
            line_height: layout.line_height,
            alignment: "center".into(),
            stroke: 1,
        });
    }
    Ok(RenderedPage { image, layouts })
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::Crop;
    use image::Rgb;
    #[test]
    fn vertical_text_is_rotated_and_stays_inside_region() {
        let original = RgbImage::from_pixel(100, 320, Rgb([220, 230, 240]));
        let region = TextRegion {
            id: "vertical".into(),
            text: "Перевод строки".into(),
            vertical: true,
            bounds: Crop {
                x: 20,
                y: 20,
                width: 50,
                height: 280,
            },
        };
        let rendered = render(&original, &[region]).unwrap();
        assert!(rendered
            .image
            .pixels()
            .zip(original.pixels())
            .any(|(a, b)| a != b));
        for (x, y, pixel) in rendered.image.enumerate_pixels() {
            if !(20..70).contains(&x) || !(20..300).contains(&y) {
                assert_eq!(pixel, original.get_pixel(x, y));
            }
        }
    }
    #[test]
    fn russian_text_fits_without_touching_pixels_outside_the_region() {
        let original = RgbImage::from_pixel(400, 300, Rgb([210, 220, 230]));
        let region = TextRegion {
            vertical: false,
            id: "dialogue".into(),
            bounds: Crop {
                x: 40,
                y: 50,
                width: 260,
                height: 140,
            },
            text: "Привет, мир!\nПродолжаем перевод.".into(),
        };
        let output = render(&original, std::slice::from_ref(&region)).unwrap();
        assert!(output
            .image
            .pixels()
            .zip(original.pixels())
            .any(|(a, b)| a != b));
        for (x, y, pixel) in output.image.enumerate_pixels() {
            if !(40..300).contains(&x) || !(50..190).contains(&y) {
                assert_eq!(pixel, original.get_pixel(x, y));
            }
        }
        assert_eq!(output.layouts.len(), 1);
        assert!(output.layouts[0].font_size >= 8.0);
    }
    #[test]
    fn small_cyrillic_descenders_fit_with_actual_ink_metrics() {
        let original = RgbImage::new(72, 72);
        let region = TextRegion {
            vertical: false,
            id: "small".into(),
            bounds: Crop {
                x: 0,
                y: 0,
                width: 72,
                height: 72,
            },
            text: "Перевод".into(),
        };
        let rendered = render(&original, &[region]).unwrap();
        assert!(rendered.layouts[0].font_size >= 8.0);
        assert!(rendered.layouts[0].font_size < 20.0);
    }
    #[test]
    fn missing_glyphs_and_overflow_never_produce_partial_pages() {
        let original = RgbImage::new(100, 100);
        let mut region = TextRegion {
            vertical: false,
            id: "a".into(),
            bounds: Crop {
                x: 1,
                y: 1,
                width: 90,
                height: 90,
            },
            text: "漢字".into(),
        };
        assert!(matches!(
            render(&original, &[region.clone()]),
            Err(Error::FontCoverage)
        ));
        region.text = "Long text that cannot fit".into();
        region.bounds.width = 4;
        region.bounds.height = 4;
        assert!(matches!(
            render(&original, &[region]),
            Err(Error::TextOverflow)
        ));
        assert_eq!(render(&original, &[]).unwrap().image, original);
    }
}
