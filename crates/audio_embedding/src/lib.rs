mod decode;
mod frontend;
pub mod model_file;
pub mod similarity;
mod windows;

#[cfg(test)]
mod oracle_tests;
#[cfg(any(test, feature = "test-support"))]
pub mod test_support;

use std::ops::Range;
use std::time::Duration;

use audio_common::AudioSource;

pub use frontend::{
    BANDS, FRAME, HALF, HOP, MelFilter, PATCH, PATCH_SAMPLES, SAMPLE_RATE, frame_count,
    hann_window, mel_filters, mel_value,
};

pub const EMBEDDING_VERSION: &str = "effnet-multi-gpu-1:7de20391:gpu1:spread32:flacbytes";
pub const DIM: usize = 1280;

pub struct Job {
    pub source: Box<dyn AudioSource>,
    pub range: Option<TrackRange>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TrackRange {
    pub start: Duration,
    pub length: Option<Duration>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Embedding {
    pub version: &'static str,
    pub vector: Box<[f32]>,
}

#[derive(Debug, thiserror::Error)]
pub enum EmbedError {
    #[error("decode: {0}")]
    Decode(String),
    #[error("shorter than one second")]
    TooShort,
    #[error("model: {0}")]
    Model(String),
    #[error("the model gave a non-finite vector")]
    NonFinite,
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("the downloaded model failed its SHA-256 check")]
    Checksum,
    #[error("network: {0}")]
    Network(String),
    #[error("cancelled")]
    Cancelled,
}

pub struct Prepared {
    rate: u32,
    resampled: usize,
    windows: Vec<Vec<f32>>,
    patches: Vec<Patch>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Patch {
    first: usize,
    frames: usize,
    start: u64,
    window: usize,
    offset: usize,
    len: usize,
}

#[derive(Debug, Clone, Copy)]
pub struct PatchInput<'a> {
    pub first_frame: usize,
    pub frames: usize,
    pub start: u64,
    pub samples: &'a [f32],
}

impl Prepared {
    fn new(rate: u32, layout: &windows::Layout, windows: Vec<Vec<f32>>) -> Option<Self> {
        let mut patches = Vec::with_capacity(layout.patches.len());
        for (chosen, needed) in layout.patches.iter().zip(&layout.needed) {
            let window = layout
                .ranges
                .iter()
                .position(|r| r.start <= needed.start && needed.end <= r.end)?;
            let offset = (needed.start - layout.ranges[window].start) as usize;
            let len = (needed.end - needed.start) as usize;
            if windows.get(window)?.len() < offset + len {
                return None;
            }
            patches.push(Patch {
                first: chosen.first,
                frames: chosen.frames,
                start: needed.start,
                window,
                offset,
                len,
            });
        }
        Some(Self {
            rate,
            resampled: layout.resampled,
            windows,
            patches,
        })
    }

    pub fn rate(&self) -> u32 {
        self.rate
    }

    pub fn resampled_len(&self) -> usize {
        self.resampled
    }

    pub fn patch_count(&self) -> usize {
        self.patches.len()
    }

    pub fn patch(&self, ix: usize) -> PatchInput<'_> {
        let patch = &self.patches[ix];
        PatchInput {
            first_frame: patch.first,
            frames: patch.frames,
            start: patch.start,
            samples: &self.windows[patch.window][patch.offset..patch.offset + patch.len],
        }
    }

    pub fn sample_count(&self) -> usize {
        self.patches.iter().map(|p| p.len).sum()
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn reference_mel(&self) -> Vec<f32> {
        use frontend::{MelFrontend, MelScratch, resampled_at, step_for};

        let frontend = MelFrontend::new();
        let mut buffers = MelScratch::new(&frontend);
        let mut out = vec![0f32; self.patches.len() * PATCH * BANDS];
        let step = step_for(self.rate);
        let mut y = Vec::new();
        for (ix, slot) in out
            .as_chunks_mut::<{ PATCH * BANDS }>()
            .0
            .iter_mut()
            .enumerate()
        {
            let patch = self.patch(ix);
            let ys = windows::resampled_range(patch.first_frame, self.resampled);
            let x = |k: isize| -> f32 {
                u64::try_from(k)
                    .ok()
                    .and_then(|k| k.checked_sub(patch.start))
                    .and_then(|k| patch.samples.get(k as usize))
                    .copied()
                    .unwrap_or(0.0)
            };
            y.clear();
            y.extend(ys.clone().map(|i| {
                if self.rate == SAMPLE_RATE {
                    x(i as isize)
                } else {
                    resampled_at(step, i, x)
                }
            }));
            let rows = slot
                .as_chunks_mut::<BANDS>()
                .0
                .iter_mut()
                .take(patch.frames);
            for (row, dst) in rows.enumerate() {
                *dst = frontend.frame(&mut buffers, patch.first_frame + row, |k| {
                    in_range(&ys, k).map_or(0.0, |k| y[k - ys.start])
                });
            }
        }
        out
    }
}

#[cfg(any(test, feature = "test-support"))]
fn in_range(range: &Range<usize>, k: isize) -> Option<usize> {
    usize::try_from(k).ok().filter(|k| range.contains(k))
}

fn expected_resampled(source: &dyn AudioSource, range: Option<TrackRange>) -> usize {
    let Some(total) = source.duration() else {
        return 0;
    };
    let start = range.map_or(Duration::ZERO, |r| r.start);
    let length = range
        .and_then(|r| r.length)
        .map_or(total.saturating_sub(start), |l| {
            l.min(total.saturating_sub(start))
        })
        .min(decode::MAX_ANALYSED);
    (length.as_secs_f64() * SAMPLE_RATE as f64) as usize + SAMPLE_RATE as usize
}

pub fn prepare(job: Job) -> Result<Prepared, EmbedError> {
    let Job { mut source, range } = job;
    if let Some(prepared) = windows::prepare(source.as_mut(), range)? {
        return Ok(prepared);
    }
    let rate = source.params().sample_rate;
    let mut resampler = frontend::Resampler::new(rate);
    let mut resampled = Vec::with_capacity(expected_resampled(source.as_ref(), range));
    let decoded = decode::read_mono(source.as_mut(), range, |chunk| {
        resampler.push(chunk, &mut resampled);
    })?;
    if decoded < rate as u64 {
        return Err(EmbedError::TooShort);
    }
    resampler.finish(&mut resampled);
    let layout = windows::layout(resampled.len() as u64, SAMPLE_RATE);
    let whole: Range<u64> = 0..resampled.len() as u64;
    let windows = if layout.ranges == [whole] {
        resampled.shrink_to_fit();
        vec![resampled]
    } else {
        layout
            .ranges
            .iter()
            .map(|r| resampled[r.start as usize..r.end as usize].to_vec())
            .collect()
    };
    Prepared::new(SAMPLE_RATE, &layout, windows)
        .ok_or_else(|| EmbedError::Decode("the patch windows do not fit the signal".into()))
}
