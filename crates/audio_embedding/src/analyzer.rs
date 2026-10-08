use std::path::Path;
use std::sync::Arc;

use tract_onnx::prelude::*;

use crate::frontend::{BANDS, PATCH};
use crate::{DIM, EMBEDDING_VERSION, EmbedError, Embedding, Prepared};

pub(crate) const BATCH: usize = 64;

fn model_error(e: impl std::fmt::Display) -> EmbedError {
    EmbedError::Model(format!("{e:#}"))
}

pub struct Analyzer {
    plan: Arc<TypedRunnableModel>,
}

impl Analyzer {
    pub fn load(model: &Path) -> Result<Self, EmbedError> {
        let plan = tract_onnx::onnx()
            .model_for_path(model)
            .and_then(|m| m.with_input_fact(0, f32::fact([BATCH, PATCH, BANDS]).into()))
            .and_then(|m| m.into_optimized())
            .and_then(|m| m.into_runnable())
            .map_err(model_error)?;
        Ok(Self { plan })
    }

    pub fn embed(&self, items: &[Prepared]) -> Vec<Result<Embedding, EmbedError>> {
        items.iter().map(|item| self.embed_one(item)).collect()
    }

    fn embed_one(&self, item: &Prepared) -> Result<Embedding, EmbedError> {
        let patch_len = PATCH * BANDS;
        let mut sum = vec![0f64; DIM];
        for chunk in item.patches.chunks(BATCH * patch_len) {
            let used = chunk.len() / patch_len;
            let mut data = vec![0f32; BATCH * patch_len];
            data[..chunk.len()].copy_from_slice(chunk);
            let input = Tensor::from_shape(&[BATCH, PATCH, BANDS], &data).map_err(model_error)?;
            let outputs = self.plan.run(tvec!(input.into())).map_err(model_error)?;
            let output = outputs
                .first()
                .ok_or_else(|| EmbedError::Model("the model returned no output".into()))?;
            let view = output.to_plain_array_view::<f32>().map_err(model_error)?;
            if view.shape() != [BATCH, DIM] {
                return Err(EmbedError::Model(format!(
                    "unexpected output shape {:?}",
                    view.shape()
                )));
            }
            let rows = view
                .as_slice()
                .ok_or_else(|| EmbedError::Model("the model output is not contiguous".into()))?;
            for row in rows.as_chunks::<DIM>().0.iter().take(used) {
                for (acc, &v) in sum.iter_mut().zip(row) {
                    *acc += v as f64;
                }
            }
        }
        let n = item.count.max(1) as f64;
        let vector: Box<[f32]> = sum.into_iter().map(|s| (s / n) as f32).collect();
        if !vector.iter().all(|v| v.is_finite()) {
            return Err(EmbedError::NonFinite);
        }
        Ok(Embedding {
            version: EMBEDDING_VERSION,
            vector,
        })
    }
}
