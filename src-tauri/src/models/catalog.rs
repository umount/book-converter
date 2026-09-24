use serde::Serialize;
use ts_rs::TS;

/// Download provenance is separate from inference capability acceptance.
#[derive(Clone, Debug, Serialize, TS)]
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
    pub fn directory(&self) -> String {
        format!("{}-{}", self.id, self.sha256)
    }
}

pub fn catalog() -> Vec<ModelSpec> {
    vec![ModelSpec {
        id: "lama-onnx-fp32".into(),
        name: "LaMa ONNX · FP32".into(),
        repository: "Carve/LaMa-ONNX".into(),
        revision: "a3ee2fca54baebec351b8fa7786154ffa7555aa6".into(),
        filename: "lama_fp32.onnx".into(),
        sha256: "1faef5301d78db7dda502fe59966957ec4b79dd64e16f03ed96913c7a4eb68d6".into(),
        bytes: 208_044_816,
        license: "Apache-2.0 (model repository declaration)".into(),
        experimental: true,
    }]
}
