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
    pub fn artifact_name(&self) -> &'static str {
        if self.filename.ends_with(".safetensors") {
            "weights.safetensors"
        } else {
            "weights.onnx"
        }
    }
    pub fn partial_name(&self) -> String {
        format!("{}.part", self.artifact_name())
    }
    pub fn directory(&self) -> String {
        format!("{}-{}", self.id, self.sha256)
    }
}

pub fn catalog() -> Vec<ModelSpec> {
    vec![
        ModelSpec {
            id: "lama-onnx-fp32".into(),
            name: "LaMa ONNX · FP32".into(),
            repository: "Carve/LaMa-ONNX".into(),
            revision: "a3ee2fca54baebec351b8fa7786154ffa7555aa6".into(),
            filename: "lama_fp32.onnx".into(),
            sha256: "1faef5301d78db7dda502fe59966957ec4b79dd64e16f03ed96913c7a4eb68d6".into(),
            bytes: 208_044_816,
            license: "Apache-2.0 (model repository declaration)".into(),
            experimental: true,
        },
        ModelSpec {
            id: "comic-text-mask-resnet18".into(),
            name: "Comic Text Mask · ResNet18".into(),
            repository: "TareHimself/comic-text-mask".into(),
            revision: "ebd37f1a9ae9298519678fb96a796573fb602977".into(),
            filename: "model.safetensors".into(),
            sha256: "ab28dd8450462c4f87ddfd05d13601813bb90414f26bea5590bf6a0a7540f988".into(),
            bytes: 57_377_484,
            license: "MIT (model repository declaration)".into(),
            experimental: true,
        },
    ]
}
