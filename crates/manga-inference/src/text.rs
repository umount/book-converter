//! Serializable lettering inputs and the exact style chosen for each region.
use crate::Crop;
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TextRegion {
    pub id: String,
    pub bounds: Crop,
    pub text: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TextLayout {
    pub id: String,
    pub font: String,
    pub font_sha256: String,
    pub font_size: f32,
    pub line_height: f32,
    pub alignment: String,
    pub stroke: u32,
}

pub const VERSION: &str = "manga-lettering-v1";
pub(crate) const FONT: &[u8] = include_bytes!("../../../src-tauri/assets/DejaVuSans.ttf");
pub fn font_hash() -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(FONT))
}
