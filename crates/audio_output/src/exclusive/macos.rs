use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Mutex;

use audio_common::{AudioBatch, AudioError};
use coreaudio::audio_unit::macos_helpers;
use coreaudio::audio_unit::render_callback::data;
use coreaudio::audio_unit::{AudioUnit, Element, Scope};
use coreaudio::audio_unit::sample_format::SampleFormat;
use coreaudio::audio_unit::stream_format::StreamFormat;
use coreaudio::audio_unit::audio_format::LinearPcmFlags;

use crate::cpal_stream::{AudioOutput, OutputConfig, PlaybackState};
use crate::ring_buffer::AudioRingBuffer;

const STATE_IDLE: u8 = 0;
const STATE_PLAYING: u8 = 1;

fn acquire_hog_mode(device_id: u32) -> Result<(), AudioError> {
    let pid = macos_helpers::get_hogging_pid(device_id)
        .map_err(|e| AudioError::DeviceNotFound(format!("Failed to check hog mode: {:?}", e)))?;

    if pid != -1 {
        return Err(AudioError::DeviceNotFound(
            "Device is in use by another application".to_string(),
        ));
    }

    macos_helpers::toggle_hog_mode(device_id)
        .map_err(|e| AudioError::DeviceNotFound(format!("Failed to acquire hog mode: {:?}", e)))?;

    let new_pid = macos_helpers::get_hogging_pid(device_id)
        .map_err(|e| AudioError::DeviceNotFound(format!("Failed to verify hog mode: {:?}", e)))?;

    if new_pid == -1 {
        return Err(AudioError::DeviceNotFound(
            "Failed to acquire exclusive access to device".to_string(),
        ));
    }

    Ok(())
}

fn release_hog_mode(device_id: u32) {
    let _ = macos_helpers::toggle_hog_mode(device_id);
}

pub fn set_device_sample_rate(device_id: u32, rate: f64) -> Result<(), AudioError> {
    macos_helpers::set_device_sample_rate(device_id, rate)
        .map_err(|e| AudioError::UnsupportedFormat(format!("Failed to set device sample rate: {:?}", e)))
}

fn get_device_sample_rate(device_id: u32) -> Result<f64, AudioError> {
    let audio_unit = macos_helpers::audio_unit_from_device_id(device_id, false)
        .map_err(|e| AudioError::DeviceNotFound(format!("Failed to create AudioUnit for device: {:?}", e)))?;
    audio_unit
        .sample_rate()
        .map_err(|e| AudioError::Output(format!("Failed to get device sample rate: {:?}", e)))
}

pub struct ExclusiveOutput {
    audio_unit: Mutex<AudioUnit>,
    buffer: Arc<AudioRingBuffer>,
    pub config: OutputConfig,
    audio_device_id: u32,
    original_sample_rate: f64,
    state: Mutex<PlaybackState>,
    playing_atomic: AtomicU8,
}

impl ExclusiveOutput {
    pub fn new(
        buffer: Arc<AudioRingBuffer>,
        config: OutputConfig,
        audio_device_id: u32,
    ) -> Result<Self, AudioError> {
        let original_sample_rate = get_device_sample_rate(audio_device_id)?;

        acquire_hog_mode(audio_device_id)?;

        let target_rate = config.sample_rate as f64;
        if (target_rate - original_sample_rate).abs() > 0.01
            && let Err(e) = set_device_sample_rate(audio_device_id, target_rate)
        {
            release_hog_mode(audio_device_id);
            return Err(e);
        }

        let channels = config.channels as usize;
        let audio_unit = match create_audio_unit(audio_device_id, &config, buffer.clone(), channels) {
            Ok(au) => au,
            Err(e) => {
                release_hog_mode(audio_device_id);
                if (target_rate - original_sample_rate).abs() > 0.01 {
                    let _ = set_device_sample_rate(audio_device_id, original_sample_rate);
                }
                return Err(e);
            }
        };

        Ok(Self {
            audio_unit: Mutex::new(audio_unit),
            buffer,
            config,
            audio_device_id,
            original_sample_rate,
            state: Mutex::new(PlaybackState::Idle),
            playing_atomic: AtomicU8::new(STATE_IDLE),
        })
    }
}

fn create_audio_unit(
    device_id: u32,
    config: &OutputConfig,
    buffer: Arc<AudioRingBuffer>,
    channels: usize,
) -> Result<AudioUnit, AudioError> {
    let mut audio_unit = macos_helpers::audio_unit_from_device_id(device_id, false)
        .map_err(|e| AudioError::Output(format!("Failed to create HAL AudioUnit: {:?}", e)))?;

    let sample_format = match config.bit_depth {
        16 => SampleFormat::I16,
        24 => SampleFormat::I24,
        _ => SampleFormat::F32,
    };

    let flags = if config.bit_depth == 32 {
        LinearPcmFlags::IS_FLOAT | LinearPcmFlags::IS_PACKED
    } else {
        LinearPcmFlags::IS_SIGNED_INTEGER | LinearPcmFlags::IS_PACKED
    };

    let stream_format = StreamFormat {
        sample_rate: config.sample_rate as f64,
        channels: config.channels as u32,
        sample_format,
        flags,
    };

    audio_unit
        .set_stream_format(stream_format, Scope::Input, Element::Output)
        .map_err(|e| AudioError::Output(format!("Failed to set stream format: {:?}", e)))?;

    audio_unit
        .set_sample_rate(config.sample_rate as f64)
        .map_err(|e| AudioError::Output(format!("Failed to set sample rate: {:?}", e)))?;

    let inner_buffer = buffer.clone();
    audio_unit
        .set_render_callback(move |args| {
            let num_frames = args.num_frames;
            let data::Interleaved { buffer: out_buf, .. } = args.data;
            let total_samples = num_frames * channels;
            let out_len = out_buf.len();
            if out_len < total_samples {
                for sample in out_buf.iter_mut() {
                    *sample = 0.0;
                }
                return Ok(());
            }
            let end = total_samples.min(out_len);
            let dest = &mut out_buf[..end];
            let read = inner_buffer.pop_slice(dest);
            for sample in &mut dest[read..] {
                *sample = 0.0;
            }
            Ok(())
        })
        .map_err(|e| AudioError::Output(format!("Failed to set render callback: {:?}", e)))?;

    audio_unit
        .initialize()
        .map_err(|e| AudioError::Output(format!("Failed to initialize AudioUnit: {:?}", e)))?;

    Ok(audio_unit)
}

impl AudioOutput for ExclusiveOutput {
    fn write(&self, batch: &AudioBatch) -> usize {
        if self.playing_atomic.load(Ordering::Relaxed) != STATE_PLAYING {
            return 0;
        }

        let f32_samples = batch.data.to_f32();
        self.buffer.write_slice_blocking(&f32_samples)
    }

    fn clear(&self) {
        self.buffer.clear();
    }

    fn pause(&self) {
        let mut state = self.state.lock().unwrap();
        if *state != PlaybackState::Playing {
            return;
        }
        let mut audio_unit = self.audio_unit.lock().unwrap();
        let _ = audio_unit.stop();
        *state = PlaybackState::Paused;
        self.playing_atomic.store(STATE_IDLE, Ordering::SeqCst);
    }

    fn resume(&self) {
        let mut state = self.state.lock().unwrap();
        if *state == PlaybackState::Playing {
            return;
        }
        let mut audio_unit = self.audio_unit.lock().unwrap();
        let _ = audio_unit.start();
        *state = PlaybackState::Playing;
        self.playing_atomic.store(STATE_PLAYING, Ordering::SeqCst);
    }

    fn is_playing(&self) -> bool {
        *self.state.lock().unwrap() == PlaybackState::Playing
    }

    fn set_volume(&self, _volume: f32) {}
}

impl Drop for ExclusiveOutput {
    fn drop(&mut self) {
        {
            let mut audio_unit = self.audio_unit.lock().unwrap();
            let _ = audio_unit.stop();
        }

        release_hog_mode(self.audio_device_id);

        let current_rate = get_device_sample_rate(self.audio_device_id).unwrap_or(self.original_sample_rate);
        if (current_rate - self.original_sample_rate).abs() > 0.01 {
            let _ = set_device_sample_rate(self.audio_device_id, self.original_sample_rate);
        }
    }
}

unsafe impl Send for ExclusiveOutput {}
unsafe impl Sync for ExclusiveOutput {}