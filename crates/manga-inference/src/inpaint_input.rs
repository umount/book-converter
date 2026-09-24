//! Symmetric border padding adapted from Koharu's LaMa processor (MIT):
//! https://github.com/koharu-rs/koharu/blob/c697b31eb1de016d9272743a2973f0e6a67eae6c/crates/koharu-ml/src/lama/processor.rs
//! Copyright (c) 2025-2026 Mayo Takanashi and Koharu contributors.
//! See third-party/koharu-LICENSE-MIT. Unlike Koharu's dynamic model, the current
//! experimental ONNX adapter requires a fixed 512-square tensor.
use crate::{validate_mask, Error, Result};
use image::{imageops, GrayImage, RgbImage};

pub struct InpaintInput {
    pub image: RgbImage,
    pub mask: GrayImage,
    pub width: u32,
    pub height: u32,
}
fn symmetric_index(index: u32, length: u32) -> u32 {
    let index = index % (length * 2);
    if index < length {
        index
    } else {
        length * 2 - index - 1
    }
}
impl InpaintInput {
    /// Preserve original crop resolution when it fits. Extend border pixels
    /// symmetrically instead of adding artificial black context or upscaling text.
    pub fn new(original: &RgbImage, mask: &GrayImage, side: u32) -> Result<Self> {
        validate_mask(mask)?;
        if original.dimensions() != mask.dimensions() || side == 0 || side > 1024 {
            return Err(Error::Dimensions);
        }
        let scale = (side as f64 / original.width().max(original.height()) as f64).min(1.0);
        let width = ((original.width() as f64 * scale).round() as u32).clamp(1, side);
        let height = ((original.height() as f64 * scale).round() as u32).clamp(1, side);
        let resized = imageops::resize(original, width, height, imageops::FilterType::CatmullRom);
        // Conservatively retain thin mask pixels when reducing a large crop.
        let mut reduced = GrayImage::new(width, height);
        for (x, y, pixel) in mask.enumerate_pixels() {
            if pixel[0] != 0 {
                let px = (u64::from(x) * u64::from(width) / u64::from(mask.width())) as u32;
                let py = (u64::from(y) * u64::from(height) / u64::from(mask.height())) as u32;
                reduced.put_pixel(px, py, *pixel);
            }
        }
        let image = RgbImage::from_fn(side, side, |x, y| {
            *resized.get_pixel(symmetric_index(x, width), symmetric_index(y, height))
        });
        let mask = GrayImage::from_fn(side, side, |x, y| {
            *reduced.get_pixel(symmetric_index(x, width), symmetric_index(y, height))
        });
        Ok(Self {
            image,
            mask,
            width,
            height,
        })
    }
    pub fn restore(&self, result: &RgbImage, width: u32, height: u32) -> Result<RgbImage> {
        if result.dimensions() != self.image.dimensions() {
            return Err(Error::Output);
        }
        crate::validate_dimensions(width, height)?;
        let crop = imageops::crop_imm(result, 0, 0, self.width, self.height).to_image();
        Ok(imageops::resize(
            &crop,
            width,
            height,
            imageops::FilterType::CatmullRom,
        ))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use image::{Luma, Rgb};
    #[test]
    fn small_crops_keep_their_pixels_and_reflect_edges() {
        let original = RgbImage::from_fn(3, 2, |x, y| Rgb([x as u8, y as u8, 77]));
        let mask = GrayImage::new(3, 2);
        let prepared = InpaintInput::new(&original, &mask, 8).unwrap();
        assert_eq!((prepared.width, prepared.height), (3, 2));
        assert_eq!(*prepared.image.get_pixel(3, 2), *original.get_pixel(2, 1));
        assert_eq!(*prepared.image.get_pixel(5, 3), *original.get_pixel(0, 0));
        assert_eq!(prepared.restore(&prepared.image, 3, 2).unwrap(), original);
    }
    #[test]
    fn reduction_keeps_single_pixel_masks_and_one_pixel_images() {
        let original = RgbImage::new(32, 16);
        let mut mask = GrayImage::new(32, 16);
        mask.put_pixel(3, 3, Luma([255]));
        let prepared = InpaintInput::new(&original, &mask, 8).unwrap();
        assert_eq!(prepared.mask.get_pixel(0, 0)[0], 255);
        assert_eq!((prepared.width, prepared.height), (8, 4));
        let prepared = InpaintInput::new(
            &RgbImage::from_pixel(1, 1, Rgb([2, 3, 4])),
            &GrayImage::new(1, 1),
            8,
        )
        .unwrap();
        assert!(prepared.image.pixels().all(|p| *p == Rgb([2, 3, 4])));
    }
}
