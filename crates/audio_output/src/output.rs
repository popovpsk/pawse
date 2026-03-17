pub mod ring_buffer;
pub use ring_buffer::AudioRingBuffer;

use audio_common::{AudioBatch, AudioError};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{OutputCallbackInfo, Stream, StreamConfig};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;

const STATE_IDLE: u8 = 0;
const STATE_PLAYING: u8 = 1;
const STATE_PAUSED: u8 = 2;

const BUFFER_LENGTH: u32 = 128; // ms

#[derive(Clone, Copy, Debug)]
pub struct OutputConfig {
    pub sample_rate: u32,
    pub channels: u8,
    pub bit_depth: u8,
}

impl Default for OutputConfig {
    fn default() -> Self {
        Self {
            channels: 2,
            sample_rate: 44100,
            bit_depth: 32,
        }
    }
}

pub trait AudioOutput: Send + Sync {
    fn write(&self, samples: AudioBatch);
    fn clear(&self);
    fn buffer(&self) -> Arc<AudioRingBuffer>;
    fn pause(&self);
    fn resume(&self);
    fn is_playing(&self) -> bool;
}

pub struct SelectedOutputDevice {
    pub host: Arc<cpal::Host>,
    pub device: Arc<cpal::Device>,
}

pub struct Output {
    _host: Arc<cpal::Host>,
    _device: Arc<cpal::Device>,
    buffer: Arc<AudioRingBuffer>,
    state: Arc<AtomicU8>,
    _stream: Stream,
}

fn calc_buffer_size(cfg: &OutputConfig) -> usize {
    (cfg.bit_depth as usize)
        * (cfg.channels as usize)
        * (cfg.sample_rate as usize)
        * (BUFFER_LENGTH as usize)
        / 8
}

impl Output {
    pub fn new(
        output_config: OutputConfig,
        device: SelectedOutputDevice,
    ) -> Result<Self, AudioError> {
        let buffer = Arc::new(AudioRingBuffer::new(calc_buffer_size(&output_config)));
        let state = Arc::new(AtomicU8::new(STATE_IDLE));

        let buffer_for_callback = buffer.clone();

        let cpal_callback = move |data: &mut [f32], _: &OutputCallbackInfo| {
            let samples_read = buffer_for_callback.pop_slice(data);

            //ToDo: notification warning
            for sample in &mut data[samples_read..] {
                *sample = 0.0;
            }
        };

        let error_callback = |err| eprintln!("Audio stream error: {}", err);

        let stream_config = StreamConfig {
            channels: output_config.channels as u16,
            sample_rate: output_config.sample_rate,
            buffer_size: cpal::BufferSize::Fixed(1024),
        };

        let output_stream = device
            .device
            .build_output_stream(&stream_config, cpal_callback, error_callback, None)
            .map_err(|e| AudioError::Output(e.to_string()))?;

        Ok(Self {
            _host: device.host.clone(),
            _device: device.device.clone(),
            buffer: buffer.clone(),
            state,
            _stream: output_stream,
        })
    }
}

impl AudioOutput for Output {
    fn write(&self, samples: AudioBatch) {
        if self.state.load(Ordering::SeqCst) != STATE_PLAYING {
            return;
        }

        let f32_samples = samples.data.to_f32();
        self.buffer.push_slice(&f32_samples);
    }

    fn clear(&self) {
        self.buffer.clear();
    }

    fn buffer(&self) -> Arc<AudioRingBuffer> {
        self.buffer.clone()
    }

    fn pause(&self) {
        if self.state.load(Ordering::SeqCst) != STATE_PLAYING {
            return; //ToDo: race condition
        }
        self.state.store(STATE_PAUSED, Ordering::SeqCst);
        self._stream.pause().unwrap_or_default();
    }

    fn resume(&self) {
        if self.state.load(Ordering::SeqCst) == STATE_PLAYING {
            return; //ToDo: race condition
        }

        self.state.store(STATE_PLAYING, Ordering::SeqCst);
        self._stream.play().unwrap_or_default();
    }

    fn is_playing(&self) -> bool {
        self.state.load(Ordering::SeqCst) == STATE_PLAYING
    }
}

pub fn make_test_device() -> SelectedOutputDevice {
    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .ok_or_else(|| AudioError::DeviceNotFound("default".to_string()))
        .unwrap();

    SelectedOutputDevice {
        host: Arc::new(host),
        device: Arc::new(device),
    }
}

pub fn make_test_config() -> OutputConfig {
    OutputConfig {
        sample_rate: 44100,
        channels: 2,
        bit_depth: 24,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use audio_common::{AudioSamples, ChannelCount, Metadata};
    use std::thread;

    fn make_test_batch(samples: Vec<f32>) -> AudioBatch {
        AudioBatch {
            data: AudioSamples::F32(samples),
            metadata: Metadata {
                sample_rate: 44100,
                channels: ChannelCount::Stereo,
                bit_depth: 32,
            },
        }
    }

    #[test]
    fn test_default_output() {
        let output = Output::new(make_test_config(), make_test_device());
        assert!(output.is_ok(), "Should create default output");
    }

    #[test]
    fn test_pause_resume() {
        let output = Output::new(make_test_config(), make_test_device()).unwrap();
        output.resume();
        assert!(output.is_playing());

        output.pause();
        thread::sleep(std::time::Duration::from_millis(10));
        assert!(!output.is_playing());

        output.resume();
        thread::sleep(std::time::Duration::from_millis(10));
        assert!(output.is_playing());
    }

    #[test]
    fn test_write_when_paused() {
        let output = Output::new(make_test_config(), make_test_device()).unwrap();
        output.pause();
        thread::sleep(std::time::Duration::from_millis(10));

        output.write(make_test_batch(vec![0.5, -0.5]));

        let buf = output.buffer();
        assert!(buf.is_empty(), "Should not write when paused");
    }

    #[test]
    fn test_clear() {
        let output = Output::new(make_test_config(), make_test_device()).unwrap();
        output.resume();
        output.write(make_test_batch(vec![0.5, -0.5, 0.3, -0.3]));

        let buf = output.buffer();
        assert_eq!(buf.len(), 4);

        output.clear();

        assert!(buf.is_empty());
    }

    #[test]
    fn test_convert_s16_to_f32() {
        let samples = AudioSamples::S16(vec![0, 32767, -32768, 0]);
        let f32_samples = samples.to_f32();

        assert_eq!(f32_samples.len(), 4);
        assert!((f32_samples[0] - 0.0).abs() < 0.001);
        assert!((f32_samples[1] - 1.0).abs() < 0.001);
        assert!((f32_samples[2] - (-1.0)).abs() < 0.001);
        assert!((f32_samples[3] - 0.0).abs() < 0.001);
    }
}
