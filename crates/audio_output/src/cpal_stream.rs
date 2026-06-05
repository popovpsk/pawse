pub use crate::ring_buffer::AudioRingBuffer;

use atomic_float::AtomicF32;
use audio_common::{AudioBatch, AudioError};
use cpal::traits::{DeviceTrait, StreamTrait};
use cpal::{FromSample, OutputCallbackInfo, SampleFormat, SizedSample, Stream, StreamConfig};
use parking_lot::RwLock;
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};

use crate::FadeEvent;

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
    fn set_volume(&self, volume: f32);
}

pub struct SelectedOutputDevice {
    pub host: Arc<cpal::Host>,
    pub device: Arc<cpal::Device>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaybackState {
    Idle,
    Playing,
    Paused,
}

struct CpalOutputStreamInner {
    state: PlaybackState,
    stream: Stream,
}

pub struct CpalOutputStream {
    inner: RwLock<CpalOutputStreamInner>,
    buffer: Arc<AudioRingBuffer>,
    volume: Arc<AtomicF32>,
    fade: Arc<FadeState>,
    pub config: OutputConfig,
}

/// Lock-free fade envelope shared between the control side (`begin_fade`) and
/// the real-time callback. The callback ramps `gain` toward `target` by `step`
/// (once per frame) and posts an `event` when it lands on the target.
pub(crate) struct FadeState {
    gain: AtomicF32,
    step: AtomicF32,
    target: AtomicF32,
    event: AtomicU8,
}

impl FadeState {
    pub(crate) fn new() -> Self {
        Self {
            gain: AtomicF32::new(1.0),
            step: AtomicF32::new(0.0),
            target: AtomicF32::new(1.0),
            event: AtomicU8::new(0),
        }
    }

    /// Starts a ramp toward `target` over `duration_ms`. If `start` is given,
    /// the gain jumps there first (used to fade in from silence on resume).
    pub(crate) fn begin(
        &self,
        sample_rate: u32,
        start: Option<f32>,
        target: f32,
        duration_ms: u32,
    ) {
        if let Some(s) = start {
            self.gain.store(s, Ordering::SeqCst);
        }
        let from = self.gain.load(Ordering::SeqCst);
        let frames = sample_rate as f32 * (duration_ms as f32 / 1000.0);
        let mag = if frames >= 1.0 { 1.0 / frames } else { 1.0 };
        let step = if target >= from { mag } else { -mag };
        self.target.store(target, Ordering::SeqCst);
        self.event.store(0, Ordering::SeqCst);
        // Store step last so the callback never sees a stale target/gain.
        self.step.store(step, Ordering::SeqCst);
    }

    pub(crate) fn take_event(&self) -> Option<FadeEvent> {
        match self.event.swap(0, Ordering::SeqCst) {
            1 => Some(FadeEvent::FadedIn),
            2 => Some(FadeEvent::FadedOut),
            _ => None,
        }
    }

    /// Cancels any ramp and pins the gain at unity. Used when (re)loading or
    /// stopping a track so fresh content never inherits a stale/frozen gain.
    pub(crate) fn reset(&self) {
        self.step.store(0.0, Ordering::SeqCst);
        self.target.store(1.0, Ordering::SeqCst);
        self.gain.store(1.0, Ordering::SeqCst);
        self.event.store(0, Ordering::SeqCst);
    }

    /// A completed fade-out (gain pinned at 0 with no active ramp) means the
    /// callback should emit silence WITHOUT draining the ring buffer, so the
    /// un-played samples survive for a seamless fade-in on resume.
    pub(crate) fn is_frozen(&self) -> bool {
        self.gain.load(Ordering::Relaxed) == 0.0 && self.step.load(Ordering::Relaxed) == 0.0
    }
}

/// Applies `base_volume` and any active fade ramp to an interleaved buffer,
/// stepping the fade gain once per frame and signalling completion.
pub(crate) fn apply_fade_gain(
    fade: &FadeState,
    base_volume: f32,
    channels: usize,
    buf: &mut [f32],
) {
    let mut gain = fade.gain.load(Ordering::Relaxed);
    let step = fade.step.load(Ordering::Relaxed);

    if step == 0.0 {
        // Steady gain: skip the multiply near unity so bit-perfect output is
        // preserved (matches the tolerance the bit-perfect indicator uses).
        let m = base_volume * gain;
        if m >= 1.0 - crate::bit_perfect::UNITY_VOLUME_TOLERANCE {
            return;
        }
        for s in buf.iter_mut() {
            *s *= m;
        }
        return;
    }

    let target = fade.target.load(Ordering::Relaxed);
    let ch = channels.max(1);
    let mut completed = 0u8;
    for (i, s) in buf.iter_mut().enumerate() {
        *s *= base_volume * gain;
        if completed == 0 && i % ch == ch - 1 {
            gain += step;
            if (step > 0.0 && gain >= target) || (step < 0.0 && gain <= target) {
                gain = target;
                completed = if target <= 0.0 { 2 } else { 1 };
            }
        }
    }
    fade.gain.store(gain, Ordering::Relaxed);
    if completed != 0 {
        fade.step.store(0.0, Ordering::Relaxed);
        fade.event.store(completed, Ordering::Relaxed);
    }
}

/// Renders one callback's worth of audio into an interleaved f32 `data` slice:
/// emits silence while frozen mid-fade, otherwise drains the ring buffer,
/// applies volume + fade gain, and zero-pads any underrun tail. Shared by the
/// native-f32 callback and the format-converting callbacks below.
fn fill_f32(
    fade: &FadeState,
    buffer: &AudioRingBuffer,
    volume: &AtomicF32,
    channels: usize,
    data: &mut [f32],
) {
    // Frozen after a fade-out: emit silence but leave the buffer intact so
    // resume can fade those same samples back in seamlessly.
    if fade.is_frozen() {
        for sample in data.iter_mut() {
            *sample = 0.0;
        }
        return;
    }

    let samples_read = buffer.pop_slice(data);
    let vol = volume.load(Ordering::Relaxed);
    apply_fade_gain(fade, vol, channels, &mut data[..samples_read]);
    for sample in &mut data[samples_read..] {
        *sample = 0.0;
    }
}

/// Picks the sample format to open the device with. Prefers the device's own
/// default format: it's F32 on the common shared path (no conversion, keeps the
/// signal intact) but is an integer format (e.g. I32/I16) on digital outputs
/// like S/PDIF and HDMI that reject F32. We then convert our internal f32
/// samples to that format in the callback.
fn negotiate_sample_format(device: &cpal::Device) -> SampleFormat {
    device
        .default_output_config()
        .map(|c| c.sample_format())
        .unwrap_or(SampleFormat::F32)
}

/// Builds an output stream whose hardware sample type is `T`, converting each
/// rendered f32 sample to `T`. A reusable scratch buffer holds the f32 frame so
/// the shared `fill_f32` logic stays format-agnostic.
fn build_converting_stream<T>(
    device: &cpal::Device,
    config: &StreamConfig,
    buffer: Arc<AudioRingBuffer>,
    volume: Arc<AtomicF32>,
    fade: Arc<FadeState>,
    channels: usize,
) -> Result<Stream, cpal::BuildStreamError>
where
    T: SizedSample + FromSample<f32>,
{
    // Pre-allocate the f32 scratch buffer outside the realtime callback so the
    // hot path stays allocation-free: `resize` only reallocs if it has to grow
    // past capacity, so reserve a generous worst-case period up front. (cpal can
    // still hand a larger buffer than this in rare configs, costing one realloc;
    // that's the only remaining allocation and it doesn't repeat.)
    const SCRATCH_PREALLOC: usize = 1 << 15; // 32768 f32 samples (16384 stereo frames)
    let mut scratch: Vec<f32> = Vec::with_capacity(SCRATCH_PREALLOC);
    device.build_output_stream(
        config,
        move |data: &mut [T], _: &OutputCallbackInfo| {
            if scratch.len() != data.len() {
                scratch.resize(data.len(), 0.0);
            }
            fill_f32(&fade, &buffer, &volume, channels, &mut scratch);
            for (out, sample) in data.iter_mut().zip(scratch.iter()) {
                *out = T::from_sample(*sample);
            }
        },
        |err| log::error!("Audio stream error: {}", err),
        None,
    )
}

impl CpalOutputStream {
    pub fn new(
        buffer: Arc<AudioRingBuffer>,
        output_config: OutputConfig,
        device: SelectedOutputDevice,
    ) -> Result<Self, AudioError> {
        let volume = Arc::new(AtomicF32::new(1.0));
        let fade = Arc::new(FadeState::new());
        let channels = output_config.channels as usize;

        let stream_config = StreamConfig {
            channels: output_config.channels as u16,
            sample_rate: output_config.sample_rate,
            buffer_size: cpal::BufferSize::Default,
        };

        let dev = &device.device;
        let format = negotiate_sample_format(dev);

        // F32 takes a direct path (no per-sample conversion); integer formats
        // go through the converting builder.
        let build = || -> Result<Stream, cpal::BuildStreamError> {
            match format {
                SampleFormat::F32 => {
                    let (buffer, volume, fade) = (buffer.clone(), volume.clone(), fade.clone());
                    dev.build_output_stream(
                        &stream_config,
                        move |data: &mut [f32], _: &OutputCallbackInfo| {
                            fill_f32(&fade, &buffer, &volume, channels, data);
                        },
                        |err| log::error!("Audio stream error: {}", err),
                        None,
                    )
                }
                SampleFormat::I32 => build_converting_stream::<i32>(
                    dev,
                    &stream_config,
                    buffer.clone(),
                    volume.clone(),
                    fade.clone(),
                    channels,
                ),
                SampleFormat::I16 => build_converting_stream::<i16>(
                    dev,
                    &stream_config,
                    buffer.clone(),
                    volume.clone(),
                    fade.clone(),
                    channels,
                ),
                SampleFormat::U16 => build_converting_stream::<u16>(
                    dev,
                    &stream_config,
                    buffer.clone(),
                    volume.clone(),
                    fade.clone(),
                    channels,
                ),
                SampleFormat::U8 => build_converting_stream::<u8>(
                    dev,
                    &stream_config,
                    buffer.clone(),
                    volume.clone(),
                    fade.clone(),
                    channels,
                ),
                // Formats we don't convert to (I24, I64, F64, …). Surface as an
                // error; the caller installs a fallback rather than dying.
                _ => Err(cpal::BuildStreamError::StreamConfigNotSupported),
            }
        };

        let output_stream =
            build().map_err(|e| AudioError::Output(format!("{e} (sample format {format:?})")))?;

        Ok(Self {
            inner: RwLock::new(CpalOutputStreamInner {
                state: PlaybackState::Idle,
                stream: output_stream,
            }),
            buffer: buffer.clone(),
            config: output_config,
            volume,
            fade,
        })
    }

    /// Starts a fade ramp toward `target` (0.0 = out, 1.0 = in) over
    /// `duration_ms`. `start`, if given, seeds the gain before ramping.
    pub fn begin_fade(&self, start: Option<f32>, target: f32, duration_ms: u32) {
        self.fade
            .begin(self.config.sample_rate, start, target, duration_ms);
    }

    /// Returns and clears a pending fade-completion signal from the callback.
    pub fn take_fade_event(&self) -> Option<FadeEvent> {
        self.fade.take_event()
    }

    /// Cancels any active fade and restores full gain.
    pub fn reset_fade(&self) {
        self.fade.reset();
    }
}

impl AudioOutput for CpalOutputStream {
    fn write(&self, samples: &AudioBatch) -> usize {
        if self.inner.read().state != PlaybackState::Playing {
            return 0;
        }

        let f32_samples = samples.data.to_f32();
        self.buffer.write_slice_blocking(&f32_samples)
    }

    fn clear(&self) {
        self.buffer.clear();
    }

    fn pause(&self) {
        let mut inner = self.inner.write();
        if inner.state != PlaybackState::Playing {
            return;
        }
        if let Err(e) = inner.stream.pause() {
            log::error!("audio output: failed to pause stream: {}", e);
        }
        inner.state = PlaybackState::Paused;
    }

    fn resume(&self) {
        let mut inner = self.inner.write();
        if inner.state == PlaybackState::Playing {
            return;
        }
        if let Err(e) = inner.stream.play() {
            log::error!("audio output: failed to resume stream: {}", e);
        }
        inner.state = PlaybackState::Playing;
    }

    fn is_playing(&self) -> bool {
        self.inner.read().state == PlaybackState::Playing
    }

    fn set_volume(&self, value: f32) {
        if !(0.0..=1.0).contains(&value) {
            panic!("Volume must be between 0.0 and 1.0");
        }

        let calculated_value = calculate_volume_scaled(value);
        self.volume.store(calculated_value, Ordering::SeqCst);
    }
}

fn calculate_volume_scaled(volume: f32) -> f32 {
    let volume = volume as f64;

    let result = {
        if volume >= 0.99 {
            1.0
        } else if volume > 0.1 {
            f64::exp(3.91202300543 * volume) / 50.0
        } else {
            volume * 0.295751527165
        }
    };
    result as f32
}

#[cfg(test)]
mod tests {
    use cpal::traits::HostTrait;

    fn make_test_device() -> (Arc<cpal::Host>, Arc<cpal::Device>) {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or_else(|| AudioError::DeviceNotFound("default".to_string()))
            .unwrap();

        (Arc::new(host), Arc::new(device))
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
        let selected_device = SelectedOutputDevice { host: h, device: d };
        let output = CpalOutputStream::new(make_test_buffer(), make_test_config(), selected_device);
        assert!(output.is_ok(), "Should create default output");
    }

    #[test]
    fn test_pause_resume() {
        let (h, d) = make_test_device();
        let selected_device = SelectedOutputDevice { host: h, device: d };
        let output =
            CpalOutputStream::new(make_test_buffer(), make_test_config(), selected_device).unwrap();
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
        let selected_device = SelectedOutputDevice { host: h, device: d };
        let output =
            CpalOutputStream::new(make_test_buffer(), make_test_config(), selected_device).unwrap();
        output.pause();
        thread::sleep(std::time::Duration::from_millis(10));

        output.write(&make_test_batch(vec![0.5, -0.5]));

        let buf = &output.buffer;
        assert!(buf.is_empty(), "Should not write when paused");
    }

    #[test]
    fn test_clear() {
        let (h, d) = make_test_device();
        let selected_device = SelectedOutputDevice { host: h, device: d };
        let output =
            CpalOutputStream::new(make_test_buffer(), make_test_config(), selected_device).unwrap();
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
