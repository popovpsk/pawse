pub mod output_stream;
pub mod ring_buffer;

use std::sync::Arc;

use audio_common::{AudioBatch, AudioError, Metadata};
use cpal::traits::HostTrait;
pub use output_stream::{AudioOutput, OutputConfig, OutputStream, SelectedOutputDevice};
use parking_lot::RwLock;

use crate::ring_buffer::AudioRingBuffer;

pub struct Output {
    host: Arc<cpal::Host>,
    device: Arc<cpal::Device>,
    stream: RwLock<OutputStream>,
}

fn calc_buffer_size(cfg: &OutputConfig) -> usize {
    (cfg.bit_depth / 8) as usize // at bytes
        * cfg.channels as usize
        * (cfg.sample_rate / 8/*1/8=125ms */) as usize
}

impl Output {
    pub fn new() -> Self {
        let host = Arc::new(cpal::default_host());
        let device = Arc::new(
            host.default_output_device()
                .ok_or_else(|| AudioError::DeviceNotFound("default".to_string()))
                .unwrap(),
        );

        let selected_output_device = SelectedOutputDevice {
            host: host.clone(),
            device: device.clone(),
        };

        let output_config = OutputConfig {
            sample_rate: 44100,
            channels: 2,
            bit_depth: 16,
        };
        let buffer = Arc::new(AudioRingBuffer::new(calc_buffer_size(&output_config)));

        Self {
            host,
            device,
            stream: RwLock::new(
                OutputStream::new(buffer.clone(), output_config, selected_output_device)
                    .expect("Failed to create audio output stream"),
            ),
        }
    }

    fn recreate_stream(&self, metadata: Metadata) {
        let device = SelectedOutputDevice {
            host: self.host.clone(),
            device: self.device.clone(),
        };

        let output_config = OutputConfig {
            sample_rate: metadata.sample_rate,
            channels: metadata.channels.to_u8(),
            bit_depth: metadata.bit_depth,
        };

        let buffer = Arc::new(AudioRingBuffer::new(calc_buffer_size(&output_config)));

        //ToDo: wait current stream here.

        let stream = OutputStream::new(buffer.clone(), output_config, device)
            .expect("Failed to create new audio output stream");
        let mut guarded = self.stream.write();
        *guarded = stream;
    }
}

impl Default for Output {
    fn default() -> Self {
        Self::new()
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
            let stream = self.stream.read();
            !is_stream_config_equal(&stream, &batch.metadata)
        };

        if needs_recreate {
            self.recreate_stream(batch.metadata.clone());
        }

        self.stream.read().write(batch)
    }

    fn clear(&self) {
        self.stream.read().clear()
    }

    fn pause(&self) {
        self.stream.read().pause()
    }

    fn resume(&self) {
        self.stream.read().resume()
    }

    fn set_volume(&self, volume: f32) {
        self.stream.read().set_volume(volume)
    }

    fn is_playing(&self) -> bool {
        self.stream.read().is_playing()
    }
}
