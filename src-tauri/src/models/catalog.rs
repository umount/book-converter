use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Pinned upstream files; no code is loaded from model repositories.
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ModelSpec {
    pub id: String,
    pub name: String,
    pub repository: String,
    pub revision: String,
    pub filename: String,
    pub sha256: String,
    pub bytes: u32,
    pub license: String,
    pub experimental: bool,
}
impl ModelSpec {
    pub fn url(&self) -> String {
        format!(
            "https://huggingface.co/{}/resolve/{}/{}",
            self.repository, self.revision, self.filename
        )
    }
    pub fn artifact_name(&self) -> &'static str {
        "artifact"
    }
    pub fn partial_name(&self) -> String {
        "artifact.part".into()
    }
    pub fn directory(&self) -> String {
        format!("{}-{}", self.id, self.sha256)
    }
}

pub fn catalog() -> Vec<ModelSpec> {
    serde_json::from_str(include_str!("qwen3-tts-06.json")).expect("embedded model catalog")
}
