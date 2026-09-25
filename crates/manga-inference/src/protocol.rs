//! Versioned, bounded worker protocol. Paths are supplied by the trusted desktop
//! service after project asset resolution, never directly from the webview.
use crate::{Crop, Error, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    io::Read,
    path::{Path, PathBuf},
};

pub const MAX_REQUEST_BYTES: u64 = 2 * 1024 * 1024;
pub const MAX_ASSET_BYTES: u64 = 32 * 1024 * 1024;

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Asset {
    pub path: PathBuf,
    pub sha256: String,
}
impl Asset {
    pub fn read(&self) -> Result<image::DynamicImage> {
        if !self.path.is_absolute() || self.sha256.len() != 64 {
            return Err(Error::Request);
        }
        let metadata = std::fs::symlink_metadata(&self.path)?;
        if !metadata.is_file() || metadata.len() > MAX_ASSET_BYTES {
            return Err(Error::Request);
        }
        let mut bytes = Vec::new();
        std::fs::File::open(&self.path)?
            .take(MAX_ASSET_BYTES + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() as u64 > MAX_ASSET_BYTES
            || format!("{:x}", Sha256::digest(&bytes)) != self.sha256
        {
            return Err(Error::Request);
        }
        let dimensions = image::ImageReader::new(std::io::Cursor::new(&bytes))
            .with_guessed_format()?
            .into_dimensions()?;
        crate::validate_dimensions(dimensions.0, dimensions.1)?;
        let mut reader =
            image::ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format()?;
        let mut limits = image::Limits::default();
        limits.max_image_width = Some(crate::MAX_EDGE);
        limits.max_image_height = Some(crate::MAX_EDGE);
        limits.max_alloc = Some(crate::MAX_PIXELS * 8);
        reader.limits(limits);

        Ok(reader.decode()?)
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "stage", rename_all = "snake_case", deny_unknown_fields)]
pub enum Operation {
    Masks {
        regions: Vec<Crop>,
        #[serde(default)]
        rectangles: Vec<Crop>,
        margin: u32,
    },
    Inpainting {
        mask: Asset,
    },
    Lettering {
        regions: Vec<crate::text::TextRegion>,
    },
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub version: u32,
    pub runtime: PathBuf,
    pub model: PathBuf,
    pub input: Asset,
    pub output: PathBuf,
    pub operation: Operation,
}
impl Request {
    pub fn validate(&self) -> Result<()> {
        if self.version != 1
            || !self.runtime.is_absolute()
            || !self.model.is_absolute()
            || !self.input.path.is_absolute()
            || !self.output.is_absolute()
            || self.output == self.input.path
            || self.output == self.model
            || self.output == self.runtime
        {
            return Err(Error::Request);
        }
        match &self.operation {
            Operation::Masks {
                regions,
                margin,
                rectangles,
            } if regions.len() + rectangles.len() > crate::page::MAX_REGIONS || *margin > 8 => {
                Err(Error::Request)
            }
            Operation::Lettering { regions } if regions.len() > crate::page::MAX_REGIONS => {
                Err(Error::Request)
            }
            Operation::Inpainting { mask }
                if !mask.path.is_absolute() || mask.path == self.output =>
            {
                Err(Error::Request)
            }
            _ => Ok(()),
        }
    }
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Response {
    pub version: u32,
    pub width: u32,
    pub height: u32,
    pub load_millis: u64,
    pub inference_millis: u64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub layouts: Vec<crate::text::TextLayout>,
}

/// Never overwrite an asset or follow an existing output symlink. The parent
/// publishes a successful private output into the content-addressed asset store.
pub fn save_new(image: &image::DynamicImage, path: &Path) -> Result<()> {
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    let result = image
        .write_to(&mut file, image::ImageFormat::Png)
        .map_err(Error::from)
        .and_then(|_| file.sync_all().map_err(Error::from));
    drop(file);
    if result.is_err() {
        let _ = std::fs::remove_file(path);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn source_hash_and_create_new_protect_existing_images() {
        let path =
            std::env::temp_dir().join(format!("manga-worker-asset-{}.png", std::process::id()));
        let image = image::DynamicImage::ImageLuma8(image::GrayImage::new(4, 3));
        let _ = std::fs::remove_file(&path);
        save_new(&image, &path).unwrap();
        let original = std::fs::read(&path).unwrap();
        assert!(save_new(&image, &path).is_err());
        let mut asset = Asset {
            path: path.clone(),
            sha256: format!("{:x}", Sha256::digest(&original)),
        };
        assert_eq!(asset.read().unwrap().width(), 4);
        asset.sha256 = "0".repeat(64);
        assert!(asset.read().is_err());
        assert_eq!(std::fs::read(&path).unwrap(), original);
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn request_rejects_unknown_fields_and_output_alias() {
        assert!(serde_json::from_str::<Operation>(
            r#"{"stage":"masks","regions":[],"margin":2,"fallback":"rectangle"}"#
        )
        .is_err());
        let request = Request {
            version: 1,
            runtime: "/runtime".into(),
            model: "/model".into(),
            input: Asset {
                path: "/original".into(),
                sha256: "0".repeat(64),
            },
            output: "/original".into(),
            operation: Operation::Masks {
                regions: vec![],
                rectangles: vec![],
                margin: 2,
            },
        };
        assert!(request.validate().is_err());
    }
}
