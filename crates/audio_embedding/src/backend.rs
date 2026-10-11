use std::sync::Arc;

use tract_onnx::prelude::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    Metal,
    Cpu,
}

impl Backend {
    pub fn name(self) -> &'static str {
        match self {
            Backend::Metal => "Metal",
            Backend::Cpu => "CPU",
        }
    }

    pub fn is_gpu(self) -> bool {
        !matches!(self, Backend::Cpu)
    }

    pub(crate) fn gpus() -> &'static [Backend] {
        if cfg!(target_os = "macos") {
            &[Backend::Metal]
        } else {
            &[]
        }
    }

    pub(crate) fn plan(self, model: InferenceModel) -> TractResult<Arc<dyn Runnable>> {
        match self {
            Backend::Cpu => Ok(Arc::new(model.into_optimized()?.into_runnable()?)),
            Backend::Metal => metal(model),
        }
    }
}

#[cfg(target_os = "macos")]
fn metal(model: InferenceModel) -> TractResult<Arc<dyn Runnable>> {
    use tract_onnx::tract_core::transform::ModelTransform;

    let mut model = model.into_typed()?.into_decluttered()?;
    tract_metal::MetalTransform::default().transform(&mut model)?;
    let options = RunOptions {
        skip_order_opt_ram: true,
        ..Default::default()
    };
    let plan = TypedSimplePlan::build(model.into_optimized()?, &options)?;
    Ok(Arc::new(Arc::new(plan)))
}

#[cfg(not(target_os = "macos"))]
fn metal(_: InferenceModel) -> TractResult<Arc<dyn Runnable>> {
    Err(TractError::msg("Metal exists only on macOS"))
}
