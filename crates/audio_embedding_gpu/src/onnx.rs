use std::collections::HashMap;
use std::path::Path;

use tract_onnx::pb::{ModelProto, NodeProto, TensorProto};
use tract_onnx::prelude::*;

pub const INPUT_ROWS: usize = 64;
pub const INPUT_LEN: usize = 128 * 96;

fn proto(path: &Path) -> TractResult<ModelProto> {
    tract_onnx::onnx().proto_model_for_path(path)
}

fn values(tensor: &TensorProto) -> Vec<f32> {
    if !tensor.float_data.is_empty() {
        return tensor.float_data.clone();
    }
    tensor
        .raw_data
        .as_chunks::<4>()
        .0
        .iter()
        .map(|b| f32::from_le_bytes(*b))
        .collect()
}

fn convs(model: &ModelProto) -> Vec<&NodeProto> {
    let graph = model.graph.as_ref().expect("a graph");
    graph.node.iter().filter(|n| n.op_type == "Conv").collect()
}

pub fn conv_tensors(path: &Path) -> TractResult<Vec<Vec<f32>>> {
    let model = proto(path)?;
    let graph = model.graph.as_ref().expect("a graph");
    let init: HashMap<&str, &TensorProto> = graph
        .initializer
        .iter()
        .map(|t| (t.name.as_str(), t))
        .collect();
    let mut out = Vec::new();
    for node in convs(&model) {
        for input in &node.input[1..] {
            let tensor = init
                .get(input.as_str())
                .ok_or_else(|| TractError::msg(format!("{}: no initializer {input}", node.name)))?;
            out.push(values(tensor));
        }
    }
    Ok(out)
}

pub fn run(path: &Path, mel: &[f32], outputs: &[String]) -> TractResult<Vec<Vec<f32>>> {
    let mut model = tract_onnx::onnx()
        .model_for_path(path)?
        .with_input_fact(0, f32::fact([INPUT_ROWS, 128, 96]).into())?;
    if !outputs.is_empty() {
        let mut names: Vec<&str> = outputs.iter().map(String::as_str).collect();
        names.push("embeddings");
        model = model.with_outputs_by_name(names)?;
    }
    let plan = model.into_optimized()?.into_runnable()?;
    let mut input = vec![0f32; INPUT_ROWS * INPUT_LEN];
    input[..mel.len()].copy_from_slice(mel);
    let tensor = Tensor::from_shape(&[INPUT_ROWS, 128, 96], &input)?;
    let result = plan.run(tvec!(tensor.into()))?;
    result
        .iter()
        .map(|t| Ok(t.to_plain_array_view::<f32>()?.iter().copied().collect()))
        .collect()
}
