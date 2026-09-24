//! ONNX graph for the pinned ResNet18/U-Net mask weights. The architecture is
//! assembled from standard operators; no Python or downloaded executable code.
//! ONNX field numbers follow https://github.com/onnx/onnx/blob/main/onnx.proto.
use crate::{Error, Result};
use serde::Deserialize;
use std::collections::BTreeMap;

#[derive(Deserialize)]
struct Tensor {
    dtype: String,
    shape: Vec<u64>,
    data_offsets: [usize; 2],
}

fn varint(out: &mut Vec<u8>, mut value: u64) {
    while value >= 128 {
        out.push((value as u8 & 127) | 128);
        value >>= 7;
    }
    out.push(value as u8);
}
fn number(out: &mut Vec<u8>, field: u64, value: u64) {
    varint(out, field << 3);
    varint(out, value);
}
fn bytes(out: &mut Vec<u8>, field: u64, data: &[u8]) {
    varint(out, (field << 3) | 2);
    varint(out, data.len() as u64);
    out.extend_from_slice(data);
}
fn text(out: &mut Vec<u8>, field: u64, value: &str) {
    bytes(out, field, value.as_bytes());
}
fn int_attribute(name: &str, values: &[u64], scalar: bool) -> Vec<u8> {
    let mut out = vec![];
    text(&mut out, 1, name);
    number(&mut out, 20, if scalar { 2 } else { 7 });
    for value in values {
        number(&mut out, if scalar { 3 } else { 8 }, *value);
    }
    out
}
fn string_attribute(name: &str, value: &str) -> Vec<u8> {
    let mut out = vec![];
    text(&mut out, 1, name);
    number(&mut out, 20, 3);
    text(&mut out, 4, value);
    out
}
fn float_attribute(name: &str, value: f32) -> Vec<u8> {
    let mut out = vec![];
    text(&mut out, 1, name);
    number(&mut out, 20, 1);
    varint(&mut out, (2 << 3) | 5);
    out.extend_from_slice(&value.to_le_bytes());
    out
}
fn tensor(name: &str, shape: &[u64], raw: &[u8]) -> Vec<u8> {
    let mut out = vec![];
    for size in shape {
        number(&mut out, 1, *size);
    }
    number(&mut out, 2, 1);
    text(&mut out, 8, name);
    bytes(&mut out, 9, raw);
    out
}
fn value_info(name: &str, dims: &[u64]) -> Vec<u8> {
    let mut shape = vec![];
    for size in dims {
        let mut dim = vec![];
        number(&mut dim, 1, *size);
        bytes(&mut shape, 1, &dim);
    }
    let mut tensor_type = vec![];
    number(&mut tensor_type, 1, 1);
    bytes(&mut tensor_type, 2, &shape);
    let mut kind = vec![];
    bytes(&mut kind, 1, &tensor_type);
    let mut out = vec![];
    text(&mut out, 1, name);
    bytes(&mut out, 2, &kind);
    out
}
struct Graph {
    data: Vec<u8>,
    next: usize,
}
impl Graph {
    fn node(&mut self, op: &str, inputs: &[&str], attributes: &[Vec<u8>]) -> String {
        let output = format!("node{}", self.next);
        self.next += 1;
        let mut node = vec![];
        for input in inputs {
            text(&mut node, 1, input);
        }
        text(&mut node, 2, &output);
        text(&mut node, 3, &output);
        text(&mut node, 4, op);
        for attribute in attributes {
            bytes(&mut node, 5, attribute);
        }
        bytes(&mut self.data, 1, &node);
        output
    }
    fn conv(&mut self, input: &str, prefix: &str, kernel: u64, stride: u64, bias: bool) -> String {
        let weight = format!("{prefix}.weight");
        let bias_name = format!("{prefix}.bias");
        let mut inputs = vec![input, weight.as_str()];
        if bias {
            inputs.push(&bias_name);
        }
        self.node(
            "Conv",
            &inputs,
            &[
                int_attribute("kernel_shape", &[kernel, kernel], false),
                int_attribute("strides", &[stride, stride], false),
                int_attribute("pads", &[kernel / 2; 4], false),
            ],
        )
    }
    fn bn(&mut self, input: &str, prefix: &str) -> String {
        let names = ["weight", "bias", "running_mean", "running_var"]
            .map(|suffix| format!("{prefix}.{suffix}"));
        self.node(
            "BatchNormalization",
            &[input, &names[0], &names[1], &names[2], &names[3]],
            &[float_attribute("epsilon", 1e-5)],
        )
    }
    fn relu(&mut self, input: &str) -> String {
        self.node("Relu", &[input], &[])
    }
}

/// The caller verifies the pinned size/hash before entering this fixed exporter.
pub fn build(weights: &[u8]) -> Result<Vec<u8>> {
    let header_len = u64::from_le_bytes(
        weights
            .get(..8)
            .ok_or(Error::Output)?
            .try_into()
            .map_err(|_| Error::Output)?,
    );
    let header_len = usize::try_from(header_len).map_err(|_| Error::Output)?;
    if header_len > 256 * 1024 {
        return Err(Error::Output);
    }
    let header_end = 8usize.checked_add(header_len).ok_or(Error::Output)?;
    let metadata: BTreeMap<String, serde_json::Value> =
        serde_json::from_slice(weights.get(8..header_end).ok_or(Error::Output)?)
            .map_err(|_| Error::Output)?;
    let raw = weights.get(header_end..).ok_or(Error::Output)?;
    let mut graph = Graph {
        data: vec![],
        next: 0,
    };
    text(&mut graph.data, 2, "comic-text-mask-resnet18-unet-v1");
    for (name, value) in metadata {
        if name == "__metadata__" {
            continue;
        }
        let entry: Tensor = serde_json::from_value(value).map_err(|_| Error::Output)?;
        if name.ends_with("num_batches_tracked") {
            continue;
        }
        if entry.dtype != "F32" {
            return Err(Error::Output);
        }
        let size = entry
            .shape
            .iter()
            .try_fold(4u64, |n, d| n.checked_mul(*d))
            .ok_or(Error::Output)?;
        let data = raw
            .get(entry.data_offsets[0]..entry.data_offsets[1])
            .ok_or(Error::Output)?;
        if data.len() as u64 != size {
            return Err(Error::Output);
        }
        bytes(&mut graph.data, 5, &tensor(&name, &entry.shape, data));
    }
    bytes(
        &mut graph.data,
        5,
        &tensor(
            "upscale",
            &[4],
            &[1f32, 1., 2., 2.]
                .into_iter()
                .flat_map(f32::to_le_bytes)
                .collect::<Vec<_>>(),
        ),
    );
    let mut current = graph.conv("image", "encoder.conv1", 7, 2, false);
    current = graph.bn(&current, "encoder.bn1");
    current = graph.relu(&current);
    let mut skips = vec![current.clone()];
    current = graph.node(
        "MaxPool",
        &[&current],
        &[
            int_attribute("kernel_shape", &[3, 3], false),
            int_attribute("strides", &[2, 2], false),
            int_attribute("pads", &[1, 1, 1, 1], false),
        ],
    );
    for layer in 1..=4 {
        for block in 0..2 {
            let prefix = format!("encoder.layer{layer}.{block}");
            let stride = if layer > 1 && block == 0 { 2 } else { 1 };
            let residual = if stride == 2 {
                let c = graph.conv(&current, &format!("{prefix}.downsample.0"), 1, 2, false);
                graph.bn(&c, &format!("{prefix}.downsample.1"))
            } else {
                current.clone()
            };
            current = graph.conv(&current, &format!("{prefix}.conv1"), 3, stride, false);
            current = graph.bn(&current, &format!("{prefix}.bn1"));
            current = graph.relu(&current);
            current = graph.conv(&current, &format!("{prefix}.conv2"), 3, 1, false);
            current = graph.bn(&current, &format!("{prefix}.bn2"));
            current = graph.node("Add", &[&current, &residual], &[]);
            current = graph.relu(&current);
        }
        if layer < 4 {
            skips.push(current.clone());
        }
    }
    for block in 0..5 {
        current = graph.node(
            "Resize",
            &[&current, "", "upscale"],
            &[
                string_attribute("mode", "nearest"),
                string_attribute("coordinate_transformation_mode", "asymmetric"),
                string_attribute("nearest_mode", "floor"),
            ],
        );
        if let Some(skip) = skips.pop() {
            current = graph.node(
                "Concat",
                &[&current, &skip],
                &[int_attribute("axis", &[1], true)],
            );
        }
        for conv in 1..=2 {
            let prefix = format!("decoder.blocks.{block}.conv{conv}");
            current = graph.conv(&current, &format!("{prefix}.0"), 3, 1, false);
            current = graph.bn(&current, &format!("{prefix}.1"));
            current = graph.relu(&current);
        }
    }
    current = graph.conv(&current, "segmentation_head.0", 3, 1, true);
    current = graph.node("Sigmoid", &[&current], &[]);
    bytes(&mut graph.data, 11, &value_info("image", &[1, 3, 384, 384]));
    bytes(
        &mut graph.data,
        12,
        &value_info(&current, &[1, 1, 384, 384]),
    );
    let mut model = vec![];
    number(&mut model, 1, 8);
    text(&mut model, 2, "book-converter");
    bytes(&mut model, 7, &graph.data);
    let mut opset = vec![];
    number(&mut opset, 2, 13);
    bytes(&mut model, 8, &opset);
    Ok(model)
}
