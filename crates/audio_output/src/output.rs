pub mod output_stream;
pub mod ring_buffer;

use std::{
    sync::{Arc, RwLock},
    u8,
};

use anyhow::Context;
use audio_common::{AudioBatch, AudioError, Metadata};
use cpal::traits::HostTrait;
pub use output_stream::{AudioOutput, OutputConfig, OutputStream, SelectedOutputDevice};

use crate::ring_buffer::AudioRingBuffer;

pub struct Output {
    host: cpal::Host,
    device: cpal::Device,
    stream: RwLock<Option<OutputStream>>,
}

const BUFFER_DURATION_MS: u32 = 128;

fn calc_buffer_size(cfg: &OutputConfig) -> usize {
    (cfg.bit_depth as usize)
        * (cfg.channels as usize)
        * (cfg.sample_rate as usize)
        * (BUFFER_DURATION_MS as usize)
        / 8
}

impl Output {
    pub fn new() -> Self {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or_else(|| AudioError::DeviceNotFound("default".to_string()))
            .unwrap();

        Self {
            host,
            device,
            stream: RwLock::new(None),
        }
    }

    fn recreate_stream(&self, metadata: Metadata) {
        let device = SelectedOutputDevice {
            host: &self.host,
            device: &self.device,
        };

        let output_config = OutputConfig {
            sample_rate: metadata.sample_rate,
            channels: metadata.channels.to_u8(),
            bit_depth: metadata.bit_depth,
        };

        let buffer = Arc::new(AudioRingBuffer::new(calc_buffer_size(&output_config)));

        //ToDo: wait and stop current stream here.

        let stream = OutputStream::new(buffer.clone(), output_config, device)
            .context("recreate_stream")
            .unwrap();
        self.stream.write().unwrap().replace(stream);
    }
}

fn is_stream_config_equal(stream: &OutputStream, metadata: &Metadata) -> bool {
    stream.config.bit_depth == metadata.bit_depth
        && stream.config.sample_rate == metadata.sample_rate
        && stream.config.channels == metadata.channels.to_u8()
}

impl AudioOutput for Output {
    fn write(&self, batch: &AudioBatch) -> usize {
        let needs_recreate = {
            let stream = self.stream.read().unwrap();
            stream.is_none() || !is_stream_config_equal(stream.as_ref().unwrap(), &batch.metadata)
        };

        if needs_recreate {
            self.recreate_stream(batch.metadata.clone());
        }

        self.stream.read().unwrap().as_ref().unwrap().write(batch)
    }

    fn clear(&self) {
        if let Some(ref s) = *self.stream.read().unwrap() {
            s.clear();
        }
    }

    fn pause(&self) {
        if let Some(ref s) = *self.stream.read().unwrap() {
            s.pause();
        }
    }

    fn resume(&self) {
        if let Some(ref s) = *self.stream.read().unwrap() {
            s.resume();
        }
    }

    fn set_volume(&self, volume: u8) {
        if let Some(ref s) = *self.stream.read().unwrap() {
            s.set_volume(volume);
        }
    }

    fn is_playing(&self) -> bool {
        if let Some(ref s) = *self.stream.read().unwrap() {
            return s.is_playing();
        }
        false
    }
}
