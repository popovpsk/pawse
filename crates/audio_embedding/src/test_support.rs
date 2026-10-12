use std::f64::consts::PI;
use std::time::Duration;

use audio_common::{
    AudioBatch, AudioError, AudioSamples, AudioSource, ChannelCount, Metadata, StreamParams,
};

const CHORD: [f64; 4] = [1.0, 1.25, 1.5, 1.335];

pub fn signal(rate: u32, length: usize) -> Vec<f32> {
    let mut state: u32 = 0x1234_5678;
    (0..length)
        .map(|i| {
            let t = i as f64 / rate as f64;
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let noise = (state >> 8) as f64 / 16_777_216.0 * 2.0 - 1.0;
            let k = CHORD[(t / 2.0).floor() as usize % 4];
            let env = 0.6 + 0.4 * (2.0 * PI * 0.5 * t).sin();
            let tone = 0.3 * (2.0 * PI * 220.0 * k * t).sin()
                + 0.2 * (2.0 * PI * 554.37 * k * t).sin()
                + 0.1 * (2.0 * PI * 1760.0 * k * t + 0.3).sin();
            (env * tone + 0.05 * noise) as f32
        })
        .collect()
}

pub struct FakeSource {
    pub rate: u32,
    pub channels: u8,
    pub samples: Vec<f32>,
    pub batch: usize,
    pub position: usize,
    pub seek_granule: usize,
    pub report_duration: bool,
    pub exact: Option<u64>,
}

impl FakeSource {
    pub fn mono(rate: u32, samples: Vec<f32>) -> Self {
        Self {
            rate,
            channels: 1,
            samples,
            batch: 1152,
            position: 0,
            seek_granule: 1,
            report_duration: true,
            exact: None,
        }
    }

    fn frames(&self) -> usize {
        self.samples.len() / self.channels as usize
    }
}

impl AudioSource for FakeSource {
    fn params(&self) -> StreamParams {
        StreamParams::new(self.rate, ChannelCount::from_u8(self.channels), 32)
    }

    fn next_buffer(&mut self) -> Result<Option<AudioBatch>, AudioError> {
        let ch = self.channels as usize;
        if self.position >= self.frames() {
            return Ok(None);
        }
        let end = (self.position + self.batch).min(self.frames());
        let data = self.samples[self.position * ch..end * ch].to_vec();
        self.position = end;
        Ok(Some(AudioBatch {
            data: AudioSamples::F32(data),
            metadata: Metadata {
                sample_rate: self.rate,
                channels: ChannelCount::from_u8(self.channels),
                bit_depth: 32,
            },
        }))
    }

    fn seek(&mut self, position: f32) -> Result<Duration, AudioError> {
        let target = (position as f64 * self.frames() as f64) as usize;
        self.position = target / self.seek_granule * self.seek_granule;
        Ok(Duration::from_secs_f64(
            self.position as f64 / self.rate as f64,
        ))
    }

    fn duration(&self) -> Option<Duration> {
        self.report_duration
            .then(|| Duration::from_secs_f64(self.frames() as f64 / self.rate as f64))
    }

    fn exact_frames(&self) -> Option<u64> {
        self.exact
    }

    fn seek_frame(&mut self, frame: u64) -> Result<u64, AudioError> {
        if self.exact.is_none() {
            return Err(AudioError::Decoder("not exact".into()));
        }
        self.position = (frame as usize).min(self.frames()) / self.seek_granule * self.seek_granule;
        Ok(self.position as u64)
    }
}
