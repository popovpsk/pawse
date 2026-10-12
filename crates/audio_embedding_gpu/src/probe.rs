use std::time::Duration;

use audio_common::{
    AudioBatch, AudioError, AudioSamples, AudioSource, ChannelCount, Metadata, StreamParams,
};
use audio_embedding::{DIM, Job, Prepared, prepare};

pub const AGREEMENT: f64 = 0.999;

struct Memory {
    rate: u32,
    samples: Vec<f32>,
    position: usize,
    exact: bool,
}

impl AudioSource for Memory {
    fn params(&self) -> StreamParams {
        StreamParams::new(self.rate, ChannelCount::from_u8(1), 32)
    }

    fn next_buffer(&mut self) -> Result<Option<AudioBatch>, AudioError> {
        if self.position >= self.samples.len() {
            return Ok(None);
        }
        let end = (self.position + 4096).min(self.samples.len());
        let data = self.samples[self.position..end].to_vec();
        self.position = end;
        Ok(Some(AudioBatch {
            data: AudioSamples::F32(data),
            metadata: Metadata {
                sample_rate: self.rate,
                channels: ChannelCount::from_u8(1),
                bit_depth: 32,
            },
        }))
    }

    fn seek(&mut self, position: f32) -> Result<Duration, AudioError> {
        self.position = (position as f64 * self.samples.len() as f64) as usize;
        Ok(Duration::from_secs_f64(
            self.position as f64 / self.rate as f64,
        ))
    }

    fn duration(&self) -> Option<Duration> {
        Some(Duration::from_secs_f64(
            self.samples.len() as f64 / self.rate as f64,
        ))
    }

    fn exact_frames(&self) -> Option<u64> {
        self.exact.then_some(self.samples.len() as u64)
    }

    fn seek_frame(&mut self, frame: u64) -> Result<u64, AudioError> {
        self.position = (frame as usize).min(self.samples.len());
        Ok(self.position as u64)
    }
}

fn signal(seed: u32, len: usize) -> Vec<f32> {
    let mut state = seed;
    let mut low = 0f32;
    (0..len)
        .map(|i| {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let noise = (state >> 8) as f32 / 16_777_216.0 * 2.0 - 1.0;
            low = low * 0.95 + noise * 0.05;
            let tone = (i % 397) as f32 / 397.0 - 0.5;
            let high = (i % 23) as f32 / 23.0 - 0.5;
            let beat = ((i / 5_000) % 3) as f32;
            0.6 * low + 0.25 * tone + 0.1 * high * beat
        })
        .collect()
}

pub fn probe_input() -> Vec<Prepared> {
    [(44_100u32, 130usize, true, 7u32), (16_000, 80, false, 11)]
        .into_iter()
        .map(|(rate, seconds, exact, seed)| {
            let source = Memory {
                rate,
                samples: signal(seed, rate as usize * seconds),
                position: 0,
                exact,
            };
            prepare(Job {
                source: Box::new(source),
                range: None,
            })
            .expect("the probe signal is long enough")
        })
        .collect()
}

pub(crate) fn worst_cosine(expected: &[f32], got: &[f32]) -> f64 {
    if expected.len() != got.len() {
        return f64::NAN;
    }
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

pub(crate) fn cosine(a: &[f32], b: &[f32]) -> f64 {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_probe_is_one_full_run_through_both_paths() {
        let items = probe_input();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].rate(), 44_100);
        assert_eq!(items[1].rate(), 16_000);
        assert_eq!(
            items.iter().map(Prepared::patch_count).sum::<usize>(),
            crate::arch::BATCH
        );
    }

    #[test]
    fn one_bad_row_fails_the_agreement_wherever_it_is() {
        let row = |v: f32| vec![v; DIM];
        let rows = |r: [Vec<f32>; 3]| r.concat();
        let expected = rows([row(1.0), row(2.0), row(3.0)]);
        assert!(worst_cosine(&expected, &expected) > 1.0 - 1e-12);
        let nan_middle = rows([row(1.0), row(f32::NAN), row(3.0)]);
        assert!(worst_cosine(&expected, &nan_middle).is_nan());
        let mut flipped = expected.clone();
        flipped[DIM] = -50.0;
        assert!(worst_cosine(&expected, &flipped) < AGREEMENT);
        assert!(worst_cosine(&expected, &expected[..DIM]).is_nan());
    }
}
