#[cfg(target_os = "macos")]
mod macos;

#[cfg(not(target_os = "macos"))]
mod unsupported;

use std::sync::Arc;

use audio_common::{AudioBatch, AudioError};

use crate::cpal_stream::{AudioOutput, OutputConfig};
use crate::ring_buffer::AudioRingBuffer;

// ----- Event type ------------------------------------------------------------

/// Asynchronous events from the platform backend. Currently the only event
/// that needs caller action is `DeviceDisconnected` — everything else (e.g.
/// device sample-rate changes) is reflected in `DeviceSnapshot` atomics
/// directly and consumed via `bit_perfect_status()` without a queued event.
#[derive(Debug, Clone)]
pub enum ExclusiveEvent {
    DeviceDisconnected,
}

// ----- Backend trait (platform-specific implementations) ---------------------

#[derive(Debug, Clone, Copy)]
pub(crate) struct DeviceSnapshot {
    /// Hardware output volume scalar; 1.0 if no hardware volume control.
    pub hw_volume: f32,
    /// Hardware output mute state; false if no hardware mute control.
    pub hw_muted: bool,
    /// Device nominal sample rate in Hz; 0 if unknown.
    pub device_sample_rate: u32,
    /// App-level digital volume scalar.
    pub app_volume: f32,
}

pub(crate) trait Backend: Send + Sync {
    fn write(&self, batch: &AudioBatch) -> usize;
    fn clear(&self);
    fn pause(&self);
    fn resume(&self);
    fn is_playing(&self) -> bool;
    fn set_volume(&self, volume: f32);
    fn is_alive(&self) -> bool;
    fn take_event(&self) -> Option<ExclusiveEvent>;
    fn original_rate(&self) -> f64;
    fn suppress_cleanup(&self);
    fn allow_cleanup(&self);
    fn device_snapshot(&self) -> DeviceSnapshot;
}

// ----- Public facade ---------------------------------------------------------

pub struct ExclusiveOutput {
    backend: Box<dyn Backend>,
    /// The output config this instance was created with.
    pub config: OutputConfig,
    /// Cached original rate (before exclusive mode changed the device).
    /// Exposed so Output can save it for recreate_stream calls.
    pub original_rate: f64,
}

impl ExclusiveOutput {
    /// Creates a new exclusive output.
    ///
    /// `original_rate`: if `Some`, use this rate for final restoration (the "true" pre-exclusive
    /// rate). If `None`, read the device's current rate (first-time activation).
    pub fn new(
        buffer: Arc<AudioRingBuffer>,
        config: OutputConfig,
        device_uid: &str,
        original_rate: Option<f64>,
    ) -> Result<Self, AudioError> {
        #[cfg(target_os = "macos")]
        {
            let backend = macos::MacosBackend::new(buffer, config, device_uid, original_rate)?;
            let rate = backend.original_rate();
            Ok(ExclusiveOutput {
                backend: Box::new(backend),
                config,
                original_rate: rate,
            })
        }

        #[cfg(not(target_os = "macos"))]
        {
            let _ = (buffer, config, device_uid, original_rate);
            return Err(AudioError::UnsupportedFormat(
                "Exclusive mode is not supported on this platform".to_string(),
            ));
        }
    }

    /// Whether the underlying device is still accessible.
    pub fn is_alive(&self) -> bool {
        self.backend.is_alive()
    }

    /// Polls the event queue. Called from `Output::write` / UI render tick.
    pub fn take_event(&self) -> Option<ExclusiveEvent> {
        self.backend.take_event()
    }

    /// Called before creating a replacement `ExclusiveOutput` to prevent
    /// hog-mode release and rate restoration when this instance drops.
    pub fn suppress_cleanup(&self) {
        self.backend.suppress_cleanup();
    }

    /// Reverts `suppress_cleanup` (used in error paths).
    pub fn allow_cleanup(&self) {
        self.backend.allow_cleanup();
    }

    /// Snapshot of device + app state for bit-perfect computation. Lock-free.
    pub(crate) fn device_snapshot(&self) -> DeviceSnapshot {
        self.backend.device_snapshot()
    }
}

/// Releases hog mode and restores the device sample rate after a failed exclusive
/// recreate where `suppress_cleanup` was set on the old output before it was dropped.
pub fn restore_device_state(device_uid: &str, original_rate: f64) {
    #[cfg(target_os = "macos")]
    macos::restore_device_state(device_uid, original_rate);
    #[cfg(not(target_os = "macos"))]
    let _ = (device_uid, original_rate);
}

impl AudioOutput for ExclusiveOutput {
    fn write(&self, batch: &AudioBatch) -> usize {
        self.backend.write(batch)
    }

    fn clear(&self) {
        self.backend.clear();
    }

    fn pause(&self) {
        self.backend.pause();
    }

    fn resume(&self) {
        self.backend.resume();
    }

    fn is_playing(&self) -> bool {
        self.backend.is_playing()
    }

    fn set_volume(&self, volume: f32) {
        self.backend.set_volume(volume);
    }
}
