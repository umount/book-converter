//! Region-driven segmentation. Bounding boxes constrain the model, never erase art.
use crate::{validate_dimensions, validate_mask, Crop, Error, Result};
use image::{GrayImage, Luma, RgbImage};

pub const MAX_REGIONS: usize = 256;

/// Small, explicit antialiasing margin around predicted glyphs, not around boxes.
/// A blank prediction remains blank. Radius is in canonical page pixels.
pub fn expand_mask(mask: &GrayImage, radius: u32) -> Result<GrayImage> {
    validate_mask(mask)?;
    if radius > 8 {
        return Err(Error::Mask);
    }
    if radius == 0 {
        return Ok(mask.clone());
    }
    let mut result = GrayImage::new(mask.width(), mask.height());
    for (x, y, pixel) in mask.enumerate_pixels() {
        if pixel[0] == 0 {
            continue;
        }
        for dy in -(radius as i32)..=radius as i32 {
            for dx in -(radius as i32)..=radius as i32 {
                if dx * dx + dy * dy > (radius * radius) as i32 {
                    continue;
                }
                let px = i64::from(x) + i64::from(dx);
                let py = i64::from(y) + i64::from(dy);
                if px >= 0
                    && py >= 0
                    && px < i64::from(mask.width())
                    && py < i64::from(mask.height())
                {
                    result.put_pixel(px as u32, py as u32, Luma([255]));
                }
            }
        }
    }
    Ok(result)
}

/// Validate the entire selection before running the first model invocation. A single
/// model instance can serve all crops; overlapping predictions are unioned.
pub fn segment_page(
    page: &RgbImage,
    regions: &[Crop],
    margin: u32,
    mut predict: impl FnMut(&RgbImage, Crop) -> Result<GrayImage>,
) -> Result<GrayImage> {
    validate_dimensions(page.width(), page.height())?;
    if regions.len() > MAX_REGIONS || margin > 8 {
        return Err(Error::Crop);
    }
    for crop in regions {
        crop.validate(page.width(), page.height())?;
        if crop.width > 2048 || crop.height > 2048 {
            return Err(Error::Crop);
        }
    }
    let mut output = GrayImage::new(page.width(), page.height());
    for &crop in regions {
        let mask = predict(page, crop)?;
        validate_mask(&mask)?;
        if mask.dimensions() != (crop.width, crop.height) {
            return Err(Error::Output);
        }
        for (x, y, value) in mask.enumerate_pixels() {
            if value[0] != 0 {
                output.put_pixel(crop.x + x, crop.y + y, *value);
            }
        }
    }
    expand_mask(&output, margin)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn glyph_margin_is_clipped_and_never_fills_empty_regions() {
        let mut mask = GrayImage::new(5, 5);
        assert_eq!(expand_mask(&mask, 2).unwrap(), mask);
        mask.put_pixel(0, 0, Luma([255]));
        let expanded = expand_mask(&mask, 1).unwrap();
        assert_eq!(expanded.as_raw().iter().filter(|p| **p == 255).count(), 3);
        assert_eq!(expanded.get_pixel(1, 1)[0], 0);
        assert!(expand_mask(&mask, 9).is_err());
    }
    #[test]
    fn regions_union_without_erasing_their_rectangles() {
        let page = RgbImage::new(10, 10);
        let regions = [
            Crop {
                x: 1,
                y: 2,
                width: 3,
                height: 3,
            },
            Crop {
                x: 2,
                y: 2,
                width: 3,
                height: 3,
            },
        ];
        let result = segment_page(&page, &regions, 0, |_, crop| {
            let mut mask = GrayImage::new(crop.width, crop.height);
            mask.put_pixel(1, 1, Luma([255]));
            Ok(mask)
        })
        .unwrap();
        assert_eq!(result.as_raw().iter().filter(|p| **p == 255).count(), 2);
        assert_eq!(result.get_pixel(2, 3)[0], 255);
        assert_eq!(result.get_pixel(3, 3)[0], 255);
        assert_eq!(result.get_pixel(1, 2)[0], 0);
    }
    #[test]
    fn malformed_later_region_prevents_any_inference() {
        let page = RgbImage::new(10, 10);
        let regions = [
            Crop {
                x: 1,
                y: 1,
                width: 3,
                height: 3,
            },
            Crop {
                x: 9,
                y: 9,
                width: 3,
                height: 3,
            },
        ];
        assert!(segment_page(&page, &regions, 0, |_, _| panic!("must validate first")).is_err());
        assert!(segment_page(&page, &regions[..1], 0, |_, _| Ok(GrayImage::new(1, 1))).is_err());
    }
}

/// Bounded crop cleanup with original-image context. Each masked pixel belongs to
/// one core tile; overlapping context is never pasted over already cleaned pixels.
/// This is the fixed-512 counterpart of Koharu's context-crop orchestration.
pub fn clean_page(
    original: &RgbImage,
    mask: &GrayImage,
    mut clean: impl FnMut(&RgbImage, &GrayImage) -> Result<RgbImage>,
) -> Result<RgbImage> {
    validate_mask(mask)?;
    if original.dimensions() != mask.dimensions() {
        return Err(Error::Dimensions);
    }
    if !mask.as_raw().contains(&255) {
        return Ok(original.clone());
    }
    if original.width().max(original.height()) <= 512 {
        return crate::composite(original, &clean(original, mask)?, mask);
    }
    let mut tiles = Vec::new();
    for y in (0..mask.height()).step_by(256) {
        for x in (0..mask.width()).step_by(256) {
            let core = Crop {
                x,
                y,
                width: 256.min(mask.width() - x),
                height: 256.min(mask.height() - y),
            };
            let contains = (y..y + core.height).any(|row| {
                mask.as_raw()[(row * mask.width() + x) as usize
                    ..(row * mask.width() + x + core.width) as usize]
                    .contains(&255)
            });
            if contains {
                tiles.push(core);
            }
        }
    }
    if tiles.len() > 64 {
        return Err(Error::Crop);
    }
    let mut output = original.clone();
    for core in tiles {
        let crop = core.padded(128, original.width(), original.height())?;
        let image =
            image::imageops::crop_imm(original, crop.x, crop.y, crop.width, crop.height).to_image();
        let crop_mask =
            image::imageops::crop_imm(mask, crop.x, crop.y, crop.width, crop.height).to_image();
        let cleaned = clean(&image, &crop_mask)?;
        if cleaned.dimensions() != image.dimensions() {
            return Err(Error::Output);
        }
        for y in core.y..core.y + core.height {
            for x in core.x..core.x + core.width {
                if mask.get_pixel(x, y)[0] == 255 {
                    output.put_pixel(x, y, *cleaned.get_pixel(x - crop.x, y - crop.y));
                }
            }
        }
    }
    Ok(output)
}

#[cfg(test)]
mod cleanup_tests {
    use super::*;
    use image::Rgb;
    #[test]
    fn tile_boundaries_keep_original_context_and_unmasked_pixels() {
        let original = RgbImage::from_pixel(520, 300, Rgb([40, 50, 60]));
        let mut mask = GrayImage::new(520, 300);
        for x in [255, 256, 511, 512] {
            mask.put_pixel(x, 100, Luma([255]));
        }
        let mut calls = 0;
        let result = clean_page(&original, &mask, |crop, _| {
            assert!(crop.width() <= 512 && crop.height() <= 512);
            assert!(crop.pixels().all(|p| *p == Rgb([40, 50, 60])));
            calls += 1;
            Ok(RgbImage::from_pixel(
                crop.width(),
                crop.height(),
                Rgb([calls, 0, 0]),
            ))
        })
        .unwrap();
        assert_eq!(calls, 3);
        for (x, y, p) in result.enumerate_pixels() {
            if mask.get_pixel(x, y)[0] == 0 {
                assert_eq!(*p, Rgb([40, 50, 60]));
            } else {
                assert_ne!(*p, Rgb([40, 50, 60]));
            }
        }
        assert_eq!(
            clean_page(&original, &GrayImage::new(520, 300), |_, _| panic!(
                "empty mask"
            ))
            .unwrap(),
            original
        );
    }
}
