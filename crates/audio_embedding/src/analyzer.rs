use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use tract_onnx::prelude::*;

use crate::backend::Backend;
use crate::frontend::{BANDS, PATCH};
use crate::{DIM, EMBEDDING_VERSION, EmbedError, Embedding, Prepared};

pub(crate) const BATCH: usize = 64;

const PATCH_LEN: usize = PATCH * BANDS;
const AGREEMENT: f64 = 0.9999;
const SPOT_CHECK_EVERY: usize = 64;

fn model_error(e: impl std::fmt::Display) -> EmbedError {
    EmbedError::Model(format!("{e:#}"))
}

pub struct Analyzer {
    backend: Backend,
    plan: Arc<dyn Runnable>,
    cpu: Arc<dyn Runnable>,
    degraded: AtomicBool,
    runs: AtomicUsize,
}

impl Analyzer {
    pub fn load(model: &Path) -> Result<Self, EmbedError> {
        let model = tract_onnx::onnx()
            .model_for_path(model)
            .and_then(|m| m.with_input_fact(0, f32::fact([BATCH, PATCH, BANDS]).into()))
            .map_err(model_error)?;
        let cpu = Backend::Cpu.plan(model.clone()).map_err(model_error)?;
        for &gpu in Backend::gpus() {
            match agreeing(gpu, model.clone(), &cpu) {
                Ok((plan, cosine)) => {
                    log::info!(
                        "audio embedding: the model runs on {} (cosine to the CPU {cosine:.7})",
                        gpu.name()
                    );
                    return Ok(Self {
                        backend: gpu,
                        plan,
                        cpu,
                        degraded: AtomicBool::new(false),
                        runs: AtomicUsize::new(0),
                    });
                }
                Err(e) => log::warn!("audio embedding: {} is not used: {e}", gpu.name()),
            }
        }
        Ok(Self {
            backend: Backend::Cpu,
            plan: cpu.clone(),
            cpu,
            degraded: AtomicBool::new(false),
            runs: AtomicUsize::new(0),
        })
    }

    pub fn backend(&self) -> Backend {
        if self.degraded.load(Ordering::Relaxed) {
            Backend::Cpu
        } else {
            self.backend
        }
    }

    pub fn patches_per_run(&self) -> usize {
        BATCH
    }

    pub fn embed(&self, items: &[Prepared]) -> Vec<Result<Embedding, EmbedError>> {
        let mut sums = vec![vec![0f64; DIM]; items.len()];
        let mut failures: Vec<Option<String>> = vec![None; items.len()];
        let mut input = vec![0f32; BATCH * PATCH_LEN];
        let rows = items.iter().map(|item| item.patches.len() / PATCH_LEN);
        for run in pack(rows, BATCH) {
            input.fill(0.0);
            let mut at = 0;
            for segment in &run {
                let from = segment.start * PATCH_LEN;
                let len = segment.len * PATCH_LEN;
                input[at..at + len].copy_from_slice(&items[segment.item].patches[from..from + len]);
                at += len;
            }
            match self.run(&input) {
                Ok(output) => {
                    let mut rows = output.as_chunks::<DIM>().0.iter();
                    for segment in &run {
                        for row in rows.by_ref().take(segment.len) {
                            for (acc, &v) in sums[segment.item].iter_mut().zip(row) {
                                *acc += v as f64;
                            }
                        }
                    }
                }
                Err(e) => {
                    let message = match e {
                        EmbedError::Model(message) => message,
                        other => other.to_string(),
                    };
                    for segment in &run {
                        failures[segment.item] = Some(message.clone());
                    }
                }
            }
        }
        items
            .iter()
            .zip(sums)
            .zip(failures)
            .map(|((item, sum), failure)| match failure {
                Some(message) => Err(EmbedError::Model(message)),
                None => mean(item.count, sum),
            })
            .collect()
    }

    fn run(&self, input: &[f32]) -> Result<Vec<f32>, EmbedError> {
        if !self.backend().is_gpu() {
            return run_plan(&self.cpu, input);
        }
        let failure = match guarded(&self.plan, input) {
            Ok(output) if !output.iter().all(|v| v.is_finite()) => {
                "it returned non-finite numbers".to_string()
            }
            Ok(output)
                if !self
                    .runs
                    .fetch_add(1, Ordering::Relaxed)
                    .is_multiple_of(SPOT_CHECK_EVERY) =>
            {
                return Ok(output);
            }
            Ok(output) => {
                let expected = run_plan(&self.cpu, input)?;
                let cosine = worst_cosine(&expected, &output);
                if cosine >= AGREEMENT {
                    return Ok(expected);
                }
                format!("a spot check against the CPU gave cosine {cosine}")
            }
            Err(e) => e.to_string(),
        };
        if !self.degraded.swap(true, Ordering::Relaxed) {
            log::error!(
                "audio embedding: {} failed ({failure}), the model runs on the CPU from now on",
                self.backend.name()
            );
        }
        run_plan(&self.cpu, input)
    }
}

fn agreeing(
    backend: Backend,
    model: InferenceModel,
    cpu: &Arc<dyn Runnable>,
) -> Result<(Arc<dyn Runnable>, f64), EmbedError> {
    let plan = catch_unwind(AssertUnwindSafe(|| backend.plan(model)))
        .map_err(|_| EmbedError::Model("building it panicked".into()))?
        .map_err(model_error)?;
    let probe = probe();
    let expected = guarded(cpu, &probe)?;
    let got = guarded(&plan, &probe)?;
    let cosine = worst_cosine(&expected, &got);
    if cosine.is_nan() || cosine < AGREEMENT {
        return Err(EmbedError::Model(format!(
            "its output differs from the CPU's (cosine {cosine})"
        )));
    }
    Ok((plan, cosine))
}

fn worst_cosine(expected: &[f32], got: &[f32]) -> f64 {
    expected
        .as_chunks::<DIM>()
        .0
        .iter()
        .zip(got.as_chunks::<DIM>().0)
        .map(|(a, b)| cosine(a, b))
        .fold(1.0f64, |worst, c| {
            if c.is_nan() || worst.is_nan() {
                f64::NAN
            } else {
                worst.min(c)
            }
        })
}

fn guarded(plan: &Arc<dyn Runnable>, input: &[f32]) -> Result<Vec<f32>, EmbedError> {
    catch_unwind(AssertUnwindSafe(|| run_plan(plan, input)))
        .unwrap_or_else(|_| Err(EmbedError::Model("the run panicked".into())))
}

fn run_plan(plan: &Arc<dyn Runnable>, input: &[f32]) -> Result<Vec<f32>, EmbedError> {
    let tensor = Tensor::from_shape(&[BATCH, PATCH, BANDS], input).map_err(model_error)?;
    let outputs = plan.run(tvec!(tensor.into())).map_err(model_error)?;
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
    view.as_slice()
        .map(<[f32]>::to_vec)
        .ok_or_else(|| EmbedError::Model("the model output is not contiguous".into()))
}

fn probe() -> Vec<f32> {
    let mut state = 0x9e37_79b9u32;
    (0..BATCH * PATCH_LEN)
        .map(|_| {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (state >> 8) as f32 / (1u32 << 24) as f32 * 3.0
        })
        .collect()
}

fn cosine(a: &[f32], b: &[f32]) -> f64 {
    let (mut dot, mut aa, mut bb) = (0f64, 0f64, 0f64);
    for (&x, &y) in a.iter().zip(b) {
        dot += x as f64 * y as f64;
        aa += x as f64 * x as f64;
        bb += y as f64 * y as f64;
    }
    if aa == 0.0 || bb == 0.0 {
        return if aa == bb { 1.0 } else { 0.0 };
    }
    dot / (aa.sqrt() * bb.sqrt())
}

fn mean(count: usize, sum: Vec<f64>) -> Result<Embedding, EmbedError> {
    let n = count.max(1) as f64;
    let vector: Box<[f32]> = sum.into_iter().map(|s| (s / n) as f32).collect();
    if !vector.iter().all(|v| v.is_finite()) {
        return Err(EmbedError::NonFinite);
    }
    Ok(Embedding {
        version: EMBEDDING_VERSION,
        vector,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Segment {
    item: usize,
    start: usize,
    len: usize,
}

fn pack(rows: impl IntoIterator<Item = usize>, batch: usize) -> Vec<Vec<Segment>> {
    let mut runs = Vec::new();
    let mut run = Vec::new();
    let mut free = batch;
    for (item, count) in rows.into_iter().enumerate() {
        let mut start = 0;
        while start < count {
            let len = (count - start).min(free);
            run.push(Segment { item, start, len });
            start += len;
            free -= len;
            if free == 0 {
                runs.push(std::mem::take(&mut run));
                free = batch;
            }
        }
    }
    if !run.is_empty() {
        runs.push(run);
    }
    runs
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    fn seg(item: usize, start: usize, len: usize) -> Segment {
        Segment { item, start, len }
    }

    #[rstest]
    #[case::nothing(vec![], vec![])]
    #[case::two_tracks_fill_one_run(vec![32, 32], vec![vec![seg(0, 0, 32), seg(1, 0, 32)]])]
    #[case::short_tracks_share_a_run(
        vec![32, 2, 1],
        vec![vec![seg(0, 0, 32), seg(1, 0, 2), seg(2, 0, 1)]]
    )]
    #[case::a_track_crosses_into_the_next_run(
        vec![40, 40],
        vec![vec![seg(0, 0, 40), seg(1, 0, 24)], vec![seg(1, 24, 16)]]
    )]
    #[case::one_track_longer_than_a_run(
        vec![150],
        vec![vec![seg(0, 0, 64)], vec![seg(0, 64, 64)], vec![seg(0, 128, 22)]]
    )]
    #[case::a_track_without_rows_takes_no_place(
        vec![0, 3],
        vec![vec![seg(1, 0, 3)]]
    )]
    fn tracks_are_packed_into_runs_in_order(
        #[case] rows: Vec<usize>,
        #[case] expected: Vec<Vec<Segment>>,
    ) {
        assert_eq!(pack(rows, 64), expected);
    }

    #[test]
    fn one_bad_row_fails_the_agreement_wherever_it_is() {
        let row = |v: f32| vec![v; DIM];
        let rows = |r: [Vec<f32>; 3]| r.concat();
        let expected = rows([row(1.0), row(2.0), row(3.0)]);
        assert!(worst_cosine(&expected, &expected) > 1.0 - 1e-12);
        let nan_first = rows([row(f32::NAN), row(2.0), row(3.0)]);
        assert!(worst_cosine(&expected, &nan_first).is_nan());
        let nan_middle = rows([row(1.0), row(f32::NAN), row(3.0)]);
        assert!(worst_cosine(&expected, &nan_middle).is_nan());
        let mut flipped = expected.clone();
        flipped[DIM] = -50.0;
        assert!(worst_cosine(&expected, &flipped) < AGREEMENT);
    }

    #[test]
    fn every_row_lands_in_exactly_one_run() {
        let rows = [32, 7, 64, 1, 33, 32, 32];
        let runs = pack(rows, 64);
        assert!(
            runs.iter()
                .all(|run| run.iter().map(|s| s.len).sum::<usize>() <= 64)
        );
        for (item, &count) in rows.iter().enumerate() {
            let mut covered: Vec<(usize, usize)> = runs
                .iter()
                .flatten()
                .filter(|s| s.item == item)
                .map(|s| (s.start, s.len))
                .collect();
            covered.sort();
            let mut next = 0;
            for (start, len) in covered {
                assert_eq!(start, next);
                next += len;
            }
            assert_eq!(next, count);
        }
    }
}
