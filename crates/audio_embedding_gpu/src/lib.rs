mod arch;
mod engine;
mod gpu;
mod input;
mod kernels;
mod probe;
pub mod weights;

#[cfg(test)]
mod onnx;
#[cfg(test)]
mod tests;

use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, PoisonError};

use audio_embedding::{DIM, EMBEDDING_VERSION, EmbedError, Embedding, Prepared};

use crate::arch::{BATCH, Role};
use crate::engine::{Engine, Pending};
use crate::input::{PCM_CAP, Segment, pack, patch_info, supported};
use crate::weights::Weights;

pub use gpu::adapter_name;
pub use probe::{AGREEMENT, probe_input};

const CHECK_EVERY: usize = 64;

pub enum Availability {
    Ready(Box<Device>),
    NoGpu(String),
    Failed(String),
}

pub fn open(weights: &Path) -> Result<Availability, EmbedError> {
    let weights = Weights::read(weights).map_err(EmbedError::Model)?;
    let adapters = gpu::candidates(false);
    if adapters.is_empty() {
        return Ok(Availability::NoGpu("no hardware graphics adapter".into()));
    }
    let mut failures = Vec::new();
    for adapter in &adapters {
        let name = gpu::describe(&adapter.get_info());
        match Device::build(adapter, &weights) {
            Ok(device) => return Ok(Availability::Ready(Box::new(device))),
            Err(e) => {
                log::warn!("audio embedding: {name} is not used: {e}");
                failures.push(format!("{name}: {e}"));
            }
        }
    }
    Ok(Availability::Failed(failures.join("; ")))
}

pub struct Device {
    engine: Engine,
    probe: Vec<Prepared>,
    expected: Vec<f32>,
    runs: AtomicUsize,
    broken: Mutex<Option<String>>,
    agreement: f64,
}

impl Device {
    fn build(adapter: &wgpu::Adapter, weights: &Weights) -> Result<Self, String> {
        let gpu = gpu::open(adapter)?;
        let scope = gpu.device.push_error_scope(wgpu::ErrorFilter::Validation);
        let engine = Engine::new(gpu, Some(weights));
        if let Some(e) = pollster::block_on(scope.pop()) {
            return Err(format!("the kernels do not build: {e}"));
        }
        let mut device = Self {
            engine,
            probe: probe_input(),
            expected: weights.floats(Role::ProbeRows),
            runs: AtomicUsize::new(0),
            broken: Mutex::new(None),
            agreement: 0.0,
        };
        device.agreement = device.check()?;
        Ok(device)
    }

    pub fn name(&self) -> String {
        self.engine.gpu.name()
    }

    pub fn agreement(&self) -> f64 {
        self.agreement
    }

    pub fn patches_per_run(&self) -> usize {
        BATCH
    }

    pub fn embed(&self, items: &[Prepared]) -> Vec<Result<Embedding, EmbedError>> {
        self.submit(items).wait()
    }

    pub fn submit<'a>(&'a self, items: &[Prepared]) -> Submitted<'a> {
        let rejected: Vec<Option<EmbedError>> = items.iter().map(reject).collect();
        let accepted: Vec<usize> = (0..items.len())
            .filter(|&i| rejected[i].is_none())
            .collect();
        let chosen: Vec<&Prepared> = accepted.iter().map(|&i| &items[i]).collect();
        let runs = pack(&chosen, BATCH, PCM_CAP);
        let before = self.runs.fetch_add(runs.len(), Ordering::Relaxed);
        if before / CHECK_EVERY != (before + runs.len()) / CHECK_EVERY
            && let Err(e) = self.check()
        {
            self.give_up(e);
        }
        let broken = self.broken();
        let runs = runs
            .into_iter()
            .map(|run| {
                let pending = match &broken {
                    Some(e) => Err(e.clone()),
                    None => self.send(&chosen, &run),
                };
                let run = run
                    .into_iter()
                    .map(|s| Segment {
                        item: accepted[s.item],
                        ..s
                    })
                    .collect();
                (run, pending)
            })
            .collect();
        Submitted {
            device: self,
            counts: items.iter().map(Prepared::patch_count).collect(),
            rejected,
            runs,
        }
    }

    fn send(&self, items: &[&Prepared], run: &[Segment]) -> Result<Pending, String> {
        let mut infos = Vec::with_capacity(BATCH);
        let mut pcm = Vec::with_capacity(BATCH);
        let mut offset = 0;
        for segment in run {
            let item = items[segment.item];
            for ix in segment.start..segment.start + segment.len {
                let patch = item.patch(ix);
                infos.push(patch_info(
                    item.rate(),
                    item.resampled_len(),
                    &patch,
                    offset,
                ));
                offset += patch.samples.len();
                pcm.push(patch.samples);
            }
        }
        self.engine.submit(&infos, &pcm)
    }

    fn rows(&self, items: &[Prepared]) -> Result<Vec<f32>, String> {
        let refs: Vec<&Prepared> = items.iter().collect();
        let mut rows = Vec::new();
        for run in pack(&refs, BATCH, PCM_CAP) {
            let used: usize = run.iter().map(|s| s.len).sum();
            let out = self.engine.wait(self.send(&refs, &run)?)?;
            rows.extend_from_slice(&out[0][..used * DIM]);
        }
        Ok(rows)
    }

    fn check(&self) -> Result<f64, String> {
        let rows = self.rows(&self.probe)?;
        let cosine = probe::worst_cosine(&self.expected, &rows);
        if cosine.is_nan() || cosine < AGREEMENT {
            return Err(format!(
                "its output differs from the reference (cosine {cosine})"
            ));
        }
        Ok(cosine)
    }

    fn broken(&self) -> Option<String> {
        self.broken
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    fn give_up(&self, reason: String) {
        let mut broken = self.broken.lock().unwrap_or_else(PoisonError::into_inner);
        if broken.is_none() {
            log::error!("audio embedding: {} failed: {reason}", self.name());
            *broken = Some(reason);
        }
    }
}

fn reject(item: &Prepared) -> Option<EmbedError> {
    if item.patch_count() == 0 {
        return Some(EmbedError::TooShort);
    }
    if !supported(item.rate()) {
        return Some(EmbedError::Decode(format!(
            "unsupported sample rate {}",
            item.rate()
        )));
    }
    (0..item.patch_count())
        .any(|p| item.patch(p).samples.len() > PCM_CAP)
        .then(|| EmbedError::Decode("a patch window is too long".into()))
}

pub struct Submitted<'a> {
    device: &'a Device,
    counts: Vec<usize>,
    rejected: Vec<Option<EmbedError>>,
    runs: Vec<(Vec<Segment>, Result<Pending, String>)>,
}

impl Submitted<'_> {
    pub fn wait(self) -> Vec<Result<Embedding, EmbedError>> {
        let Submitted {
            device,
            counts,
            mut rejected,
            runs,
        } = self;
        let mut sums = vec![vec![0f64; DIM]; counts.len()];
        let mut failure: Option<String> = None;
        for (run, pending) in runs {
            let out = pending.and_then(|p| device.engine.wait(p));
            let rows = match out {
                Ok(mut out) => out.swap_remove(0),
                Err(e) => {
                    device.give_up(e.clone());
                    failure.get_or_insert(e);
                    continue;
                }
            };
            let mut rows = rows.as_chunks::<DIM>().0.iter();
            for segment in &run {
                for row in rows.by_ref().take(segment.len) {
                    for (acc, &v) in sums[segment.item].iter_mut().zip(row) {
                        *acc += v as f64;
                    }
                }
            }
        }
        let mut results: Vec<Result<Embedding, EmbedError>> = counts
            .iter()
            .zip(sums)
            .zip(rejected.iter_mut())
            .map(|((&count, sum), rejected)| {
                if let Some(e) = rejected.take() {
                    return Err(e);
                }
                if let Some(e) = &failure {
                    return Err(EmbedError::Model(e.clone()));
                }
                mean(count, sum)
            })
            .collect();
        if let Some(e) = device.broken() {
            for result in &mut results {
                if result.is_ok() {
                    *result = Err(EmbedError::Model(e.clone()));
                }
            }
            return results;
        }
        let non_finite = results
            .iter()
            .any(|r| matches!(r, Err(EmbedError::NonFinite)));
        if non_finite
            && failure.is_none()
            && let Err(e) = device.check()
        {
            device.give_up(e.clone());
            for result in &mut results {
                if matches!(result, Err(EmbedError::NonFinite) | Ok(_)) {
                    *result = Err(EmbedError::Model(e.clone()));
                }
            }
        }
        results
    }
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
