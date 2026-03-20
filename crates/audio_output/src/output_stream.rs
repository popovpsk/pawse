pub use crate::ring_buffer::AudioRingBuffer;

use audio_common::{AudioBatch, AudioError};
use cpal::traits::{DeviceTrait, StreamTrait};
use cpal::{OutputCallbackInfo, Stream, StreamConfig};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;

const STATE_IDLE: u8 = 0;
const STATE_PLAYING: u8 = 1;
const STATE_PAUSED: u8 = 2;

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
    fn write(&self, samples: &AudioBatch) -> usize;
    fn clear(&self);
    fn pause(&self);
    fn resume(&self);
    fn is_playing(&self) -> bool;
    fn set_volume(&self, volume: u8);
}

pub struct SelectedOutputDevice<'a> {
    pub host: &'a cpal::Host,
    pub device: &'a cpal::Device,
}

pub struct OutputStream {
    buffer: Arc<AudioRingBuffer>,
    state: Arc<AtomicU8>,
    volume: Arc<AtomicU8>,
    pub config: OutputConfig,
    _stream: Stream,
}

fn apply_volume(volume: &AtomicU8, b: &mut [f32]) {
    let vol = volume.load(Ordering::Relaxed);
    if vol >= 98 {
        return;
    }

    let linear_vol = (vol as f32) / 100.0;
    let multiplier = {
        if vol < 10 {
            linear_vol
        } else {
            linear_vol.powi(3)
        }
    };

    for sample in b {
        *sample *= multiplier;
    }
}

impl OutputStream {
    pub fn new(
        buffer: Arc<AudioRingBuffer>,
        output_config: OutputConfig,
        device: SelectedOutputDevice,
    ) -> Result<Self, AudioError> {
        let state = Arc::new(AtomicU8::new(STATE_IDLE));

        let buffer_for_callback = buffer.clone();

        let volume = Arc::new(AtomicU8::new(100));
        let volume_ref = volume.clone();

        let cpal_callback = move |data: &mut [f32], _: &OutputCallbackInfo| {
            let samples_read = buffer_for_callback.pop_slice(data);

            apply_volume(volume_ref.as_ref(), &mut data[samples_read..]);

            //ToDo: notification warning
            for sample in &mut data[samples_read..] {
                *sample = 0.0;
            }
        };

        let error_callback = |err| eprintln!("Audio stream error: {}", err);

        let stream_config = StreamConfig {
            channels: output_config.channels as u16,
            sample_rate: output_config.sample_rate,
            buffer_size: cpal::BufferSize::Default,
        };

        let output_stream = device
            .device
            .build_output_stream(&stream_config, cpal_callback, error_callback, None)
            .map_err(|e| AudioError::Output(e.to_string()))?;

        Ok(Self {
            buffer: buffer.clone(),
            state,
            _stream: output_stream,
            config: output_config,
            volume: volume.clone(),
        })
    }
}

impl AudioOutput for OutputStream {
    fn write(&self, samples: &AudioBatch) -> usize {
        if self.state.load(Ordering::Relaxed) != STATE_PLAYING {
            return 0 as usize;
        }

        let f32_samples = samples.data.to_f32();
        self.buffer.write_slice_blocking(&f32_samples)
    }

    fn clear(&self) {
        self.buffer.clear();
    }

    fn pause(&self) {
        if self.state.load(Ordering::Relaxed) != STATE_PLAYING {
            return; //ToDo: race condition
        }
        self.state.store(STATE_PAUSED, Ordering::Relaxed);
        self._stream.pause().unwrap_or_default();
    }

    fn resume(&self) {
        if self.state.load(Ordering::Relaxed) == STATE_PLAYING {
            return; //ToDo: race condition
        }

        self.state.store(STATE_PLAYING, Ordering::SeqCst);
        self._stream.play().unwrap_or_default();
    }

    fn is_playing(&self) -> bool {
        self.state.load(Ordering::Relaxed) == STATE_PLAYING
    }

    fn set_volume(&self, value: u8) {
        if value > 100 {
            panic!("Volume must be between 0 and 100");
        }

        self.volume.store(value, Ordering::SeqCst);
    }
}

#[cfg(test)]
mod tests {
    use cpal::traits::HostTrait;

    fn make_test_device() -> (cpal::Host, cpal::Device) {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or_else(|| AudioError::DeviceNotFound("default".to_string()))
            .unwrap();

        return (host, device);
    }

    fn make_test_config() -> OutputConfig {
        OutputConfig {
            sample_rate: 44100,
            channels: 2,
            bit_depth: 24,
        }
    }

    fn make_test_buffer() -> Arc<AudioRingBuffer> {
        Arc::new(AudioRingBuffer::new(100 * 2 * 44100 * 24))
    }

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
        let (h, d) = make_test_device();
        let selected_device = SelectedOutputDevice {
            host: &h,
            device: &d,
        };
        let output = OutputStream::new(make_test_buffer(), make_test_config(), selected_device);
        assert!(output.is_ok(), "Should create default output");
    }

    #[test]
    fn test_pause_resume() {
        let (h, d) = make_test_device();
        let selected_device = SelectedOutputDevice {
            host: &h,
            device: &d,
        };
        let output =
            OutputStream::new(make_test_buffer(), make_test_config(), selected_device).unwrap();
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
        let (h, d) = make_test_device();
        let selected_device = SelectedOutputDevice {
            host: &h,
            device: &d,
        };
        let output =
            OutputStream::new(make_test_buffer(), make_test_config(), selected_device).unwrap();
        output.pause();
        thread::sleep(std::time::Duration::from_millis(10));

        output.write(&make_test_batch(vec![0.5, -0.5]));

        let buf = &output.buffer;
        assert!(buf.is_empty(), "Should not write when paused");
    }

    #[test]
    fn test_clear() {
        let (h, d) = make_test_device();
        let selected_device = SelectedOutputDevice {
            host: &h,
            device: &d,
        };
        let output =
            OutputStream::new(make_test_buffer(), make_test_config(), selected_device).unwrap();
        output.resume();
        output.write(&make_test_batch(vec![0.5, -0.5, 0.3, -0.3]));

        let buf = &output.buffer;
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
