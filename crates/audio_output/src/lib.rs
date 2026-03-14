use audio_common::{AudioBuffer, AudioError, ChannelCount, StreamParams};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{OutputCallbackInfo, Stream, StreamConfig};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

pub trait AudioSink: Send + Sync {
    fn write(&self, buffer: &AudioBuffer) -> Result<(), AudioError>;
    fn drain(&self) -> Result<(), AudioError>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputMode {
    Shared,
    Exclusive,
}

pub struct DeviceInfo {
    pub name: String,
    pub id: String,
}

pub fn list_output_devices() -> Vec<DeviceInfo> {
    let host = cpal::default_host();
    host.output_devices()
        .map(|devices| {
            devices
                .filter_map(|d| {
                    d.name().ok().map(|name| DeviceInfo {
                        name,
                        id: String::new(),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

pub struct CpalOutput {
    stream: Option<Stream>,
    buffer: Arc<Mutex<Vec<f32>>>,
    sample_rate: u32,
    channels: u16,
    bit_depth: u8,
    running: Arc<AtomicBool>,
}

impl CpalOutput {
    pub fn new(params: StreamParams, device: Option<&str>) -> Result<Self, AudioError> {
        let host = cpal::default_host();

        let output_device = if let Some(name) = device {
            host.output_devices()
                .map_err(|e| AudioError::Output(e.to_string()))?
                .find(|d| d.name().map(|n| n.contains(name)).unwrap_or(false))
                .ok_or_else(|| AudioError::DeviceNotFound(name.to_string()))?
        } else {
            host.default_output_device()
                .ok_or_else(|| AudioError::DeviceNotFound("default".to_string()))?
        };

        let _config = output_device
            .default_output_config()
            .map_err(|e| AudioError::Output(e.to_string()))?;

        let sample_rate = params.sample_rate;
        let channels = params.channels.to_u8() as u16;
        let bit_depth = params.bit_depth;

        let stream_config = StreamConfig {
            channels,
            sample_rate: cpal::SampleRate(sample_rate),
            buffer_size: cpal::BufferSize::Default,
        };

        let buffer = Arc::new(Mutex::new(Vec::new()));
        let buffer_clone = buffer.clone();
        let running = Arc::new(AtomicBool::new(false));
        let running_clone = running.clone();

        let err_fn = |err| eprintln!("Audio stream error: {}", err);

        let stream = output_device
            .build_output_stream(
                &stream_config,
                move |data: &mut [f32], _: &OutputCallbackInfo| {
                    if !running_clone.load(Ordering::SeqCst) {
                        data.fill(0.0);
                        return;
                    }

                    let mut buf = buffer_clone.lock().unwrap();
                    let needed = data.len();

                    if buf.len() >= needed {
                        data.copy_from_slice(&buf[..needed]);
                        buf.drain(..needed);
                    } else {
                        data[..buf.len()].copy_from_slice(&buf);
                        data[buf.len()..].fill(0.0);
                        buf.clear();
                    }
                },
                err_fn,
                None,
            )
            .map_err(|e| AudioError::Output(e.to_string()))?;

        Ok(Self {
            stream: Some(stream),
            buffer,
            sample_rate,
            channels,
            bit_depth,
            running,
        })
    }

    pub fn start(&mut self) -> Result<(), AudioError> {
        if let Some(ref stream) = self.stream {
            stream
                .play()
                .map_err(|e| AudioError::Output(e.to_string()))?;
            self.running.store(true, Ordering::SeqCst);
        }
        Ok(())
    }

    pub fn stop(&mut self) -> Result<(), AudioError> {
        self.running.store(false, Ordering::SeqCst);
        if let Some(ref stream) = self.stream {
            stream
                .pause()
                .map_err(|e| AudioError::Output(e.to_string()))?;
        }
        Ok(())
    }

    pub fn stream_params(&self) -> StreamParams {
        StreamParams::new(
            self.sample_rate,
            ChannelCount::from_u8(self.channels as u8),
            self.bit_depth,
        )
    }
}

impl AudioSink for CpalOutput {
    fn write(&self, buffer: &AudioBuffer) -> Result<(), AudioError> {
        let samples = buffer.as_slice();
        let mut buf = self.buffer.lock().unwrap();
        buf.extend(samples);
        Ok(())
    }

    fn drain(&self) -> Result<(), AudioError> {
        Ok(())
    }
}

impl Drop for CpalOutput {
    fn drop(&mut self) {
        self.running.store(false, Ordering::SeqCst);
        if let Some(ref stream) = self.stream {
            let _ = stream.pause();
        }
    }
}

unsafe impl Send for CpalOutput {}
unsafe impl Sync for CpalOutput {}

pub struct SilentSink;

impl SilentSink {
    pub fn new() -> Self {
        Self
    }
}

impl Default for SilentSink {
    fn default() -> Self {
        Self::new()
    }
}

impl AudioSink for SilentSink {
    fn write(&self, _buffer: &AudioBuffer) -> Result<(), AudioError> {
        Ok(())
    }

    fn drain(&self) -> Result<(), AudioError> {
        Ok(())
    }
}
