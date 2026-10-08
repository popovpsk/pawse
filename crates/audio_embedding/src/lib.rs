mod analyzer;
mod decode;
mod frontend;
pub mod model_file;
pub mod similarity;

#[cfg(test)]
mod oracle_tests;

use std::time::Duration;

use audio_common::AudioSource;

pub use analyzer::Analyzer;

pub const EMBEDDING_VERSION: &str = "effnet-multi-1:65cfde30:fe1:spread32";
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
    patches: Vec<f32>,
    count: usize,
}

impl Prepared {
    pub fn patch_count(&self) -> usize {
        self.count
    }
}

pub fn prepare(job: Job) -> Result<Prepared, EmbedError> {
    let Job { mut source, range } = job;
    let rate = source.params().sample_rate;
    let frontend = frontend::MelFrontend::new();
    let mut resampler = frontend::Resampler::new(rate);
    let mut mel = frontend::MelStream::new(&frontend);
    let mut resampled = Vec::new();
    let decoded = decode::read_mono(source.as_mut(), range, |chunk| {
        resampled.clear();
        resampler.push(chunk, &mut resampled);
        mel.push(&resampled);
    })?;
    if decoded < rate as u64 {
        return Err(EmbedError::TooShort);
    }
    resampled.clear();
    resampler.finish(&mut resampled);
    mel.push(&resampled);
    let (patches, count) = frontend::select_patches(&mel.finish());
    Ok(Prepared { patches, count })
}
