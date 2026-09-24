//! CPU-only ONNX adapters. Load in an isolated worker, never the webview process.
use crate::{composite, mask_graph, validate_mask, Crop, Error, Letterbox, Result};
use image::{imageops, GrayImage, Rgb, RgbImage};
use ort::{session::Session, value::Tensor};
use sha2::{Digest, Sha256};
use std::{io::Read, path::Path};

pub const MASK_BYTES: u64 = 57_377_484;
pub const MASK_SHA256: &str = "ab28dd8450462c4f87ddfd05d13601813bb90414f26bea5590bf6a0a7540f988";
pub const LAMA_BYTES: u64 = 208_044_816;
pub const LAMA_SHA256: &str = "1faef5301d78db7dda502fe59966957ec4b79dd64e16f03ed96913c7a4eb68d6";

pub fn initialize(runtime: &Path) -> Result<()> {
    if !runtime.is_absolute() || !std::fs::symlink_metadata(runtime)?.is_file() {
        return Err(Error::ModelHash);
    }
    ort::init_from(runtime.to_string_lossy())
        .with_name("manga-inference")
        .commit()?;
    Ok(())
}
fn model_bytes(path: &Path, size: u64, hash: &str) -> Result<Vec<u8>> {
    let metadata = std::fs::symlink_metadata(path)?;
    if !metadata.is_file() || metadata.len() != size {
        return Err(Error::ModelHash);
    }
    let mut data = Vec::with_capacity(size as usize);
    std::fs::File::open(path)?
        .take(size + 1)
        .read_to_end(&mut data)?;
    if data.len() as u64 != size || format!("{:x}", Sha256::digest(&data)) != hash {
        return Err(Error::ModelHash);
    }
    Ok(data)
}
fn session(bytes: &[u8]) -> Result<Session> {
    Ok(Session::builder()?
        .with_intra_threads(2)?
        .with_inter_threads(1)?
        .commit_from_memory(bytes)?)
}
fn channels(image: &RgbImage, normalize: bool) -> Vec<f32> {
    let size = (image.width() * image.height()) as usize;
    let mut values = vec![0.0; size * 3];
    for (i, pixel) in image.pixels().enumerate() {
        for channel in 0..3 {
            let value = f32::from(pixel[channel]) / 255.0;
            values[channel * size + i] = if normalize {
                (value - [0.485, 0.456, 0.406][channel]) / [0.229, 0.224, 0.225][channel]
            } else {
                value
            };
        }
    }
    values
}

pub struct TextMask {
    session: Session,
}
impl TextMask {
    pub fn load(weights: &Path) -> Result<Self> {
        let weights = model_bytes(weights, MASK_BYTES, MASK_SHA256)?;
        let graph = mask_graph::build(&weights)?;
        drop(weights);
        Ok(Self {
            session: session(&graph)?,
        })
    }
    pub fn probabilities(&mut self, square: &RgbImage) -> Result<Vec<f32>> {
        if square.dimensions() != (384, 384) {
            return Err(Error::Dimensions);
        }
        let input = Tensor::from_array(([1usize, 3, 384, 384], channels(square, true)))?;
        let output = self.session.run(ort::inputs!["image"=>input])?;
        let (shape, values) = output[0].try_extract_tensor::<f32>()?;
        if shape.as_ref() != [1, 1, 384, 384]
            || values
                .iter()
                .any(|v| !v.is_finite() || !(-1e-6..=1.000001).contains(v))
        {
            return Err(Error::Output);
        }
        // ORT sigmoid fusion may differ from [0,1] by a few float32 ULPs.
        Ok(values.iter().map(|v| v.clamp(0.0, 1.0)).collect())
    }
    pub fn segment(&mut self, page: &RgbImage, crop: Crop) -> Result<GrayImage> {
        crop.validate(page.width(), page.height())?;
        let mapping = Letterbox::new(crop, 384)?;
        let probability = self.probabilities(&mapping.rgb(page)?)?;
        mapping.mask(&probability, 0.5)
    }
}

pub struct Lama {
    session: Session,
}
impl Lama {
    pub fn load(weights: &Path) -> Result<Self> {
        let weights = model_bytes(weights, LAMA_BYTES, LAMA_SHA256)?;
        let session = session(&weights)?;
        Ok(Self { session })
    }
    pub fn clean(&mut self, original: &RgbImage, mask: &GrayImage) -> Result<RgbImage> {
        validate_mask(mask)?;
        if original.dimensions() != mask.dimensions()
            || original.width() > 2048
            || original.height() > 2048
        {
            return Err(Error::Dimensions);
        }
        if !mask.as_raw().contains(&255) {
            return Ok(original.clone());
        }
        let mapping = Letterbox::new(
            Crop {
                x: 0,
                y: 0,
                width: original.width(),
                height: original.height(),
            },
            512,
        )?;
        let square = mapping.rgb(original)?;
        let resized = imageops::resize(
            mask,
            mapping.paste.width,
            mapping.paste.height,
            imageops::FilterType::Nearest,
        );
        let mut square_mask = GrayImage::new(512, 512);
        imageops::replace(
            &mut square_mask,
            &resized,
            mapping.paste.x.into(),
            mapping.paste.y.into(),
        );
        let image = Tensor::from_array(([1usize, 3, 512, 512], channels(&square, false)))?;
        let mask_input = Tensor::from_array((
            [1usize, 1, 512, 512],
            square_mask
                .as_raw()
                .iter()
                .map(|v| f32::from(*v) / 255.0)
                .collect::<Vec<_>>(),
        ))?;
        let output = self
            .session
            .run(ort::inputs!["image"=>image,"mask"=>mask_input])?;
        let (shape, values) = output[0].try_extract_tensor::<f32>()?;
        if shape.as_ref() != [1, 3, 512, 512] || values.iter().any(|v| !v.is_finite()) {
            return Err(Error::Output);
        }
        let mut cleaned = RgbImage::new(512, 512);
        for (x, y, pixel) in cleaned.enumerate_pixels_mut() {
            let offset = (y * 512 + x) as usize;
            *pixel = Rgb(std::array::from_fn(|channel| {
                values[channel * 512 * 512 + offset]
                    .round()
                    .clamp(0.0, 255.0) as u8
            }));
        }
        let crop = imageops::crop_imm(
            &cleaned,
            mapping.paste.x,
            mapping.paste.y,
            mapping.paste.width,
            mapping.paste.height,
        )
        .to_image();
        let candidate = imageops::resize(
            &crop,
            original.width(),
            original.height(),
            imageops::FilterType::Triangle,
        );
        composite(original, &candidate, mask)
    }
}
