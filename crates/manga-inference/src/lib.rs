//! Bounded pixel transforms shared by model workers and the desktop application.
//! No network access, automatic downloads or model-free image cleanup.
use image::{imageops, GrayImage, Luma, RgbImage};
use serde::{Deserialize, Serialize};

pub const MAX_PIXELS: u64 = 40_000_000;
pub const MAX_EDGE: u32 = 16_384;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("invalid image dimensions")]
    Dimensions,
    #[error("invalid worker request")]
    Request,
    #[error("worker timed out")]
    Timeout,
    #[error("worker failed")]
    Worker,
    #[error("invalid crop")]
    Crop,
    #[error("invalid model output")]
    Output,
    #[error("invalid pixel mask")]
    Mask,
    #[error("model checksum mismatch")]
    ModelHash,
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Image(#[from] image::ImageError),
    #[cfg(feature = "onnx")]
    #[error("{0}")]
    Runtime(#[from] ort::Error),
}
pub type Result<T> = std::result::Result<T, Error>;

pub fn validate_dimensions(width: u32, height: u32) -> Result<()> {
    if width == 0
        || height == 0
        || width > MAX_EDGE
        || height > MAX_EDGE
        || u64::from(width) * u64::from(height) > MAX_PIXELS
    {
        return Err(Error::Dimensions);
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Crop {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}
impl Crop {
    pub fn validate(self, width: u32, height: u32) -> Result<Self> {
        validate_dimensions(width, height)?;
        if self.width == 0
            || self.height == 0
            || u64::from(self.x) + u64::from(self.width) > u64::from(width)
            || u64::from(self.y) + u64::from(self.height) > u64::from(height)
        {
            return Err(Error::Crop);
        }
        Ok(self)
    }
    pub fn padded(self, amount: u32, width: u32, height: u32) -> Result<Self> {
        self.validate(width, height)?;
        let x = self.x.saturating_sub(amount);
        let y = self.y.saturating_sub(amount);
        Ok(Self {
            x,
            y,
            width: self
                .x
                .saturating_add(self.width)
                .saturating_add(amount)
                .min(width)
                - x,
            height: self
                .y
                .saturating_add(self.height)
                .saturating_add(amount)
                .min(height)
                - y,
        })
    }
}

/// Black letterboxing preserves the model's published input contract. The exact
/// paste rectangle is retained; projection never uses the page or UI zoom ratio.
#[derive(Debug, Clone, Copy)]
pub struct Letterbox {
    crop: Crop,
    paste: Crop,
    side: u32,
}
impl Letterbox {
    pub fn new(crop: Crop, side: u32) -> Result<Self> {
        validate_dimensions(crop.width, crop.height)?;
        if side == 0 || side > 1024 {
            return Err(Error::Dimensions);
        }
        let scale = side as f64 / crop.width.max(crop.height) as f64;
        let width = ((crop.width as f64 * scale).round() as u32).clamp(1, side);
        let height = ((crop.height as f64 * scale).round() as u32).clamp(1, side);
        Ok(Self {
            crop,
            paste: Crop {
                x: (side - width) / 2,
                y: (side - height) / 2,
                width,
                height,
            },
            side,
        })
    }
    pub fn rgb(self, page: &RgbImage) -> Result<RgbImage> {
        self.crop.validate(page.width(), page.height())?;
        let crop = imageops::crop_imm(
            page,
            self.crop.x,
            self.crop.y,
            self.crop.width,
            self.crop.height,
        )
        .to_image();
        let resized = imageops::resize(
            &crop,
            self.paste.width,
            self.paste.height,
            imageops::FilterType::Triangle,
        );
        let mut square = RgbImage::new(self.side, self.side);
        imageops::replace(
            &mut square,
            &resized,
            self.paste.x.into(),
            self.paste.y.into(),
        );
        Ok(square)
    }
    /// Project binary segmentation to crop pixels. Padding predictions are ignored.
    pub fn mask(self, probabilities: &[f32], threshold: f32) -> Result<GrayImage> {
        if probabilities.len() != self.side as usize * self.side as usize
            || !threshold.is_finite()
            || !(0.0..=1.0).contains(&threshold)
            || probabilities
                .iter()
                .any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
        {
            return Err(Error::Output);
        }
        let mut mask = GrayImage::new(self.crop.width, self.crop.height);
        for (x, y, pixel) in mask.enumerate_pixels_mut() {
            let sx = self.paste.x
                + ((u64::from(2 * x + 1) * u64::from(self.paste.width))
                    / (2 * u64::from(self.crop.width))) as u32;
            let sy = self.paste.y
                + ((u64::from(2 * y + 1) * u64::from(self.paste.height))
                    / (2 * u64::from(self.crop.height))) as u32;
            *pixel = Luma([
                if probabilities[(sy * self.side + sx) as usize] > threshold {
                    255
                } else {
                    0
                },
            ]);
        }
        Ok(mask)
    }
}

pub fn validate_mask(mask: &GrayImage) -> Result<()> {
    validate_dimensions(mask.width(), mask.height())?;
    if mask.as_raw().iter().any(|v| *v != 0 && *v != 255) {
        return Err(Error::Mask);
    }
    Ok(())
}

/// Only model-selected pixels are replaced. Decoded artwork outside the mask is
/// preserved exactly, including when the native model changes its whole crop.
pub fn composite(original: &RgbImage, candidate: &RgbImage, mask: &GrayImage) -> Result<RgbImage> {
    validate_mask(mask)?;
    if original.dimensions() != candidate.dimensions() || original.dimensions() != mask.dimensions()
    {
        return Err(Error::Dimensions);
    }
    let mut output = original.clone();
    for (x, y, pixel) in output.enumerate_pixels_mut() {
        if mask.get_pixel(x, y)[0] == 255 {
            *pixel = *candidate.get_pixel(x, y);
        }
    }
    Ok(output)
}

#[cfg(feature = "onnx")]
mod mask_graph;
pub mod page;
pub mod protocol;
#[cfg(feature = "worker")]
pub mod worker;

#[cfg(feature = "onnx")]
pub mod onnx;

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgb;
    #[test]
    fn padding_is_ignored_and_projection_uses_the_saved_paste_rectangle() {
        let mapping = Letterbox::new(
            Crop {
                x: 13,
                y: 7,
                width: 8,
                height: 4,
            },
            8,
        )
        .unwrap();
        assert_eq!(
            mapping.paste,
            Crop {
                x: 0,
                y: 2,
                width: 8,
                height: 4
            }
        );
        let mut probabilities = vec![1.0; 64];
        probabilities[16..48].fill(0.0);
        probabilities[3 * 8 + 2] = 1.0;
        let mask = mapping.mask(&probabilities, 0.5).unwrap();
        assert_eq!(mask.as_raw().iter().filter(|v| **v == 255).count(), 1);
        assert_eq!(mask.get_pixel(2, 1)[0], 255);
        let page = RgbImage::from_pixel(30, 20, Rgb([20, 40, 60]));
        let square = mapping.rgb(&page).unwrap();
        assert_eq!(square.get_pixel(0, 0), &Rgb([0, 0, 0]));
        assert_eq!(square.get_pixel(2, 3), &Rgb([20, 40, 60]));
    }
    #[test]
    fn invalid_geometry_and_nonfinite_predictions_are_rejected() {
        assert!(Crop {
            x: u32::MAX,
            y: 0,
            width: 1,
            height: 1
        }
        .validate(100, 100)
        .is_err());
        assert!(validate_dimensions(10000, 10000).is_err());
        let map = Letterbox::new(
            Crop {
                x: 0,
                y: 0,
                width: 2,
                height: 2,
            },
            2,
        )
        .unwrap();
        assert!(map.mask(&[0.0, 1.0, f32::NAN, 0.0], 0.5).is_err());
        assert!(map.mask(&[0.0; 4], f32::NAN).is_err());
        assert!(map.mask(&[0.0; 3], 0.5).is_err());
        assert!(map.mask(&[2.0; 4], 0.5).is_err());
    }
    #[test]
    fn cleanup_never_changes_unmasked_artwork() {
        let original = RgbImage::from_pixel(8, 6, Rgb([30, 40, 50]));
        let candidate = RgbImage::from_pixel(8, 6, Rgb([250, 251, 252]));
        let mut mask = GrayImage::new(8, 6);
        mask.put_pixel(3, 4, Luma([255]));
        let output = composite(&original, &candidate, &mask).unwrap();
        for (x, y, pixel) in output.enumerate_pixels() {
            assert_eq!(
                pixel,
                if (x, y) == (3, 4) {
                    candidate.get_pixel(x, y)
                } else {
                    original.get_pixel(x, y)
                }
            );
        }
        mask.put_pixel(1, 1, Luma([128]));
        assert!(composite(&original, &candidate, &mask).is_err());
    }
    #[test]
    fn context_padding_clips_at_page_edges() {
        let crop = Crop {
            x: 0,
            y: 95,
            width: 5,
            height: 5,
        };
        assert_eq!(
            crop.padded(10, 100, 100).unwrap(),
            Crop {
                x: 0,
                y: 85,
                width: 15,
                height: 15
            }
        );
    }
}
