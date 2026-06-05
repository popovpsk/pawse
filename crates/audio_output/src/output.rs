pub mod bit_perfect;
pub mod cpal_stream;
pub mod device;
pub mod exclusive;
pub mod ring_buffer;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, Ordering};

use atomic_float::AtomicF32;
use audio_common::{AudioBatch, AudioError, AudioSamples, Metadata};
pub use bit_perfect::{BitPerfectIssue, BitPerfectStatus, UNITY_VOLUME_TOLERANCE};
use cpal::traits::HostTrait;
pub use cpal_stream::{
    AudioOutput, CpalOutputStream, OutputConfig, PlaybackState, SelectedOutputDevice,
};
use device::DeviceManager;
use parking_lot::{Mutex, RwLock};

use crate::ring_buffer::AudioRingBuffer;

enum OutputMode {
    Shared(CpalOutputStream),
    Exclusive(exclusive::ExclusiveOutput),
}

/// Signal posted by the real-time callback when a fade ramp reaches its target.
/// Drained by the engine via `Output::take_fade_event`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FadeEvent {
    FadedIn,
    FadedOut,
}

/// User-visible audio events. UI consumes via `drain_events`. Each variant is a
/// single human-readable message; callers should display these as notifications.
#[derive(Debug, Clone)]
pub enum OutputEvent {
    /// Recoverable failure on the audio pipeline. The output has already been
    /// transitioned into a working state (shared on system default); no user
    /// action is required, but they should know.
    Recovered { message: String },
    /// The output is currently broken and we couldn't transition to anything
    /// working. Severe.
    Failure { message: String },
}

pub struct Output {
    host: Arc<cpal::Host>,
    device_manager: RwLock<DeviceManager>,
    /// `None` only during transitions while the old mode has been dropped and
    /// the new one has not yet been installed. All accessors must handle this.
    current: RwLock<Option<OutputMode>>,
    events: Mutex<Vec<OutputEvent>>,
    // Source state — updated from write(); read by bit_perfect_status().
    source_sample_rate: AtomicU32,
    source_bit_depth: AtomicU8,
    source_present: AtomicBool,
    // App-level digital volume — mirrored here so bit_perfect_status() can read it.
    app_volume: AtomicF32,
}

fn calc_buffer_size(cfg: &OutputConfig) -> usize {
    const BUFFER_DURATION_MS: u32 = 128;
    cfg.channels as usize * (cfg.sample_rate as usize * BUFFER_DURATION_MS as usize / 1000)
}

const DEFAULT_CONFIG: OutputConfig = OutputConfig {
    sample_rate: 44100,
    channels: 2,
    bit_depth: 16,
};

impl Output {
    pub fn new() -> Self {
        let host = Arc::new(cpal::default_host());
        let buffer = Arc::new(AudioRingBuffer::new(calc_buffer_size(&DEFAULT_CONFIG)));

        let mut device_manager =
            DeviceManager::from_host(&host).unwrap_or_else(|_| DeviceManager::headless(&host));

        let (current, events) = match device_manager.resolve_device().and_then(|device| {
            let selected = SelectedOutputDevice {
                host: host.clone(),
                device,
            };
            CpalOutputStream::new(buffer, DEFAULT_CONFIG, selected)
        }) {
            Ok(stream) => (Some(OutputMode::Shared(stream)), Vec::new()),
            Err(e) => {
                let message = format!("No audio output device available: {e}");
                log::error!("audio output: {message}");
                (None, vec![OutputEvent::Failure { message }])
            }
        };

        Self {
            host,
            device_manager: RwLock::new(device_manager),
            current: RwLock::new(current),
            events: Mutex::new(events),
            source_sample_rate: AtomicU32::new(0),
            source_bit_depth: AtomicU8::new(0),
            source_present: AtomicBool::new(false),
            app_volume: AtomicF32::new(1.0),
        }
    }

    fn current_config(&self) -> OutputConfig {
        match self.current.read().as_ref() {
            Some(OutputMode::Shared(s)) => s.config,
            Some(OutputMode::Exclusive(e)) => e.config,
            None => DEFAULT_CONFIG,
        }
    }

    /// Atomically take the current mode out (replacing with `None`) so that
    /// dropping it (which releases the device) happens before we try to open
    /// a new stream on the same device.
    fn take_current(&self) -> Option<OutputMode> {
        self.current.write().take()
    }

    fn push_event(&self, evt: OutputEvent) {
        match &evt {
            OutputEvent::Recovered { message } => log::warn!("audio output recovered: {message}"),
            OutputEvent::Failure { message } => log::error!("audio output failure: {message}"),
        }
        self.events.lock().push(evt);
    }

    /// Drain all pending events. UI consumes these on its render tick.
    pub fn drain_events(&self) -> Vec<OutputEvent> {
        std::mem::take(&mut *self.events.lock())
    }

    fn recreate_stream(&self, metadata: Metadata) {
        let was_playing = self.is_playing();
        let new_config = OutputConfig {
            sample_rate: metadata.sample_rate,
            channels: metadata.channels.to_u8(),
            bit_depth: metadata.bit_depth,
        };

        let is_exclusive = matches!(*self.current.read(), Some(OutputMode::Exclusive(_)));

        if is_exclusive {
            self.recreate_exclusive(new_config, was_playing);
        } else {
            self.recreate_shared(new_config, was_playing);
        }
    }

    fn recreate_exclusive(&self, new_config: OutputConfig, was_playing: bool) {
        // Snapshot everything we need from the old instance AND mark it to skip
        // cleanup inside a single read-borrow, so a concurrent `set_exclusive`
        // (UI thread) can't swap `current` between our reads. Resolve the UID
        // outside the guard — DeviceManager has its own lock and we don't want
        // to hold the current-mode read lock across CoreAudio FFI calls.
        let orig_rate = {
            let current = self.current.read();
            let Some(OutputMode::Exclusive(old)) = current.as_ref() else {
                return;
            };
            old.suppress_cleanup();
            old.original_rate
        };

        let device_uid = match self.device_manager.write().resolve_uid() {
            Ok(u) => u,
            Err(e) => {
                // We marked the old instance with suppress_cleanup but never
                // got far enough to install a replacement — flip cleanup back
                // on so the eventual Drop restores the device properly.
                if let Some(OutputMode::Exclusive(old)) = self.current.read().as_ref() {
                    old.allow_cleanup();
                }
                self.push_event(OutputEvent::Failure {
                    message: format!("Cannot resolve device UID: {}", e),
                });
                return;
            }
        };

        // Drop the old exclusive output FIRST so its IOProc is destroyed before
        // we register a new one on the same device.
        let _ = self.take_current();

        let buffer = Arc::new(AudioRingBuffer::new(calc_buffer_size(&new_config)));
        match exclusive::ExclusiveOutput::new(buffer, new_config, &device_uid, Some(orig_rate)) {
            Ok(excl) => {
                if was_playing {
                    excl.resume();
                }
                *self.current.write() = Some(OutputMode::Exclusive(excl));
                self.apply_current_volume();
            }
            Err(e) => {
                // The old output was dropped with suppress_cleanup set, so hog
                // was NOT released and the device rate was NOT restored. Do it
                // now before opening a shared stream on the same device.
                exclusive::restore_device_state(&device_uid, orig_rate);
                self.push_event(OutputEvent::Recovered {
                    message: format!(
                        "Couldn't switch exclusive format ({}); falling back to shared.",
                        e
                    ),
                });
                self.install_shared_fallback(&new_config, was_playing);
            }
        }
    }

    fn recreate_shared(&self, new_config: OutputConfig, was_playing: bool) {
        let _ = self.take_current();
        let device = match self.device_manager.write().resolve_device() {
            Ok(d) => d,
            Err(e) => {
                self.push_event(OutputEvent::Failure {
                    message: format!("Cannot open output device: {}", e),
                });
                return;
            }
        };
        let selected = SelectedOutputDevice {
            host: self.host.clone(),
            device,
        };
        let buffer = Arc::new(AudioRingBuffer::new(calc_buffer_size(&new_config)));
        match CpalOutputStream::new(buffer, new_config, selected) {
            Ok(stream) => {
                if was_playing {
                    stream.resume();
                }
                *self.current.write() = Some(OutputMode::Shared(stream));
                self.apply_current_volume();
            }
            Err(e) => {
                // Selected device may have disappeared; try system default.
                self.device_manager.write().select_default();
                if !self.try_install_shared_on_default(&new_config, was_playing) {
                    self.push_event(OutputEvent::Failure {
                        message: format!("Failed to recreate shared stream: {}", e),
                    });
                } else {
                    self.push_event(OutputEvent::Recovered {
                        message: format!(
                            "Selected device unavailable ({}); switched to system default.",
                            e
                        ),
                    });
                }
            }
        }
    }

    /// Creates a fresh shared stream on the currently selected device. Used as
    /// a recovery path when an exclusive setup fails and we need *something*.
    /// On failure of the selected device, retries on system default; only
    /// returns false if even that doesn't work.
    fn install_shared_fallback(&self, config: &OutputConfig, resume_after: bool) -> bool {
        let device = match self.device_manager.write().resolve_device() {
            Ok(d) => d,
            Err(_) => return self.try_install_shared_on_default(config, resume_after),
        };
        let selected = SelectedOutputDevice {
            host: self.host.clone(),
            device,
        };
        let buffer = Arc::new(AudioRingBuffer::new(calc_buffer_size(config)));
        match CpalOutputStream::new(buffer, *config, selected) {
            Ok(stream) => {
                if resume_after {
                    stream.resume();
                }
                *self.current.write() = Some(OutputMode::Shared(stream));
                self.apply_current_volume();
                true
            }
            Err(_) => {
                self.device_manager.write().select_default();
                self.try_install_shared_on_default(config, resume_after)
            }
        }
    }

    fn try_install_shared_on_default(&self, config: &OutputConfig, resume_after: bool) -> bool {
        // This path opens the host default device directly (bypassing
        // `resolve_device`), so target the current default sink explicitly —
        // the last-resort fallback must reach the system default device and not
        // be silently rerouted by PipeWire's stream-restore.
        #[cfg(target_os = "linux")]
        crate::device::set_pipewire_node(crate::device::pulse_default_sink().as_deref());

        let Some(device) = self.host.default_output_device() else {
            return false;
        };
        let selected = SelectedOutputDevice {
            host: self.host.clone(),
            device: Arc::new(device),
        };
        let buffer = Arc::new(AudioRingBuffer::new(calc_buffer_size(config)));
        match CpalOutputStream::new(buffer, *config, selected) {
            Ok(stream) => {
                if resume_after {
                    stream.resume();
                }
                *self.current.write() = Some(OutputMode::Shared(stream));
                self.apply_current_volume();
                true
            }
            Err(_) => false,
        }
    }

    pub fn set_exclusive(&self, exclusive: bool) -> Result<(), AudioError> {
        let was_playing = self.is_playing();
        let config = self.current_config();

        if exclusive {
            let device_uid = {
                let mut dm = self.device_manager.write();
                let uid = dm.resolve_uid()?;
                // Pin the UID so leave_exclusive restores the shared stream on
                // this exact device. Without pinning, macOS may have changed
                // the system default while hog mode was active, so
                // install_shared_fallback would open on the wrong device.
                dm.set_selected_uid(uid.clone());
                uid
            };
            let buffer = Arc::new(AudioRingBuffer::new(calc_buffer_size(&config)));

            // Drop the existing shared stream BEFORE acquiring hog mode on
            // the same device. Two clients (cpal HAL + our IOProc) on the
            // same physical device during init is unsafe at best.
            let old = self.take_current();
            drop(old);

            match exclusive::ExclusiveOutput::new(buffer, config, &device_uid, None) {
                Ok(excl) => {
                    if was_playing {
                        excl.resume();
                    }
                    *self.current.write() = Some(OutputMode::Exclusive(excl));
                    self.apply_current_volume();
                    Ok(())
                }
                Err(e) => {
                    // Exclusive failed — make sure we always have a working
                    // shared stream so the app doesn't break. Also push a
                    // Recovered event so the UI can tell the user *why* the
                    // switch didn't flip (the Err message is the same as in
                    // the toast; the event acknowledges the state landed in
                    // a working place, not a half-broken one).
                    self.install_shared_fallback(&config, was_playing);
                    self.push_event(OutputEvent::Recovered {
                        message: format!(
                            "Couldn't acquire exclusive mode ({}); staying on shared.",
                            e
                        ),
                    });
                    Err(e)
                }
            }
        } else {
            // Leaving exclusive mode is a "I want out" command. We promise to
            // land in a working state regardless of what happens to the device
            // along the way (it may have been unplugged while exclusive was on).
            self.leave_exclusive(config, was_playing);
            Ok(())
        }
    }

    /// Infallibly transitions to shared mode. If the currently selected device
    /// can't be opened, falls back to system default. Only surfaces failures via
    /// the event queue — never returns Err.
    fn leave_exclusive(&self, config: OutputConfig, resume_after: bool) {
        // Drop the exclusive output first so it releases hog mode and restores
        // the device rate (or doesn't, if the device is dead) before cpal tries
        // to open anything.
        let _ = self.take_current();

        // First try the user-selected device.
        if self.install_shared_fallback(&config, resume_after) {
            return;
        }

        // Last resort: shared on system default with a known-good config.
        if self.try_install_shared_on_default(&DEFAULT_CONFIG, resume_after) {
            self.push_event(OutputEvent::Recovered {
                message: "Switched to system default device.".to_string(),
            });
        } else {
            self.push_event(OutputEvent::Failure {
                message: "No working audio device available.".to_string(),
            });
        }
    }

    /// Starts a fade ramp on the active output. `target` 0.0 fades out, 1.0
    /// fades in; `start` (if given) seeds the gain first. No-op while the
    /// output is mid-transition (`current` is `None`).
    pub fn begin_fade(&self, start: Option<f32>, target: f32, duration_ms: u32) {
        match self.current.read().as_ref() {
            Some(OutputMode::Shared(s)) => s.begin_fade(start, target, duration_ms),
            Some(OutputMode::Exclusive(e)) => e.begin_fade(start, target, duration_ms),
            None => {}
        }
    }

    /// Returns and clears a pending fade-completion signal from the callback.
    pub fn take_fade_event(&self) -> Option<FadeEvent> {
        match self.current.read().as_ref() {
            Some(OutputMode::Shared(s)) => s.take_fade_event(),
            Some(OutputMode::Exclusive(e)) => e.take_fade_event(),
            None => None,
        }
    }

    /// Cancels any active fade and restores full gain on the active output.
    pub fn reset_fade(&self) {
        match self.current.read().as_ref() {
            Some(OutputMode::Shared(s)) => s.reset_fade(),
            Some(OutputMode::Exclusive(e)) => e.reset_fade(),
            None => {}
        }
    }

    pub fn volume(&self) -> f32 {
        self.app_volume.load(Ordering::Relaxed)
    }

    fn apply_current_volume(&self) {
        let v = self.app_volume.load(Ordering::Relaxed);
        match self.current.read().as_ref() {
            Some(OutputMode::Shared(s)) => s.set_volume(v),
            Some(OutputMode::Exclusive(e)) => e.set_volume(v),
            None => {}
        }
    }

    /// Sets the hardware output volume to `volume` (0.0–1.0) via a CoreAudio
    /// property write. Only valid in exclusive mode; silently ignored otherwise.
    pub fn set_hw_volume(&self, volume: f32) {
        if let Some(OutputMode::Exclusive(e)) = self.current.read().as_ref() {
            e.set_hw_volume(volume);
        }
    }

    pub fn is_exclusive(&self) -> bool {
        matches!(*self.current.read(), Some(OutputMode::Exclusive(_)))
    }

    pub fn source_format(&self) -> Option<(u32, u8)> {
        if !self.source_present.load(Ordering::Relaxed) {
            return None;
        }
        Some((
            self.source_sample_rate.load(Ordering::Relaxed),
            self.source_bit_depth.load(Ordering::Relaxed),
        ))
    }

    pub fn selected_device_name(&self) -> String {
        self.device_manager
            .read()
            .selected_device_name()
            .to_string()
    }

    /// Index of the selected device in the current enumeration, or `None` if
    /// the selection is "follow default" or no longer present.
    pub fn selected_device_index(&self) -> Option<usize> {
        self.device_manager.read().selected_device_index()
    }

    /// The pinned device UID (`None` = follow system default). Lets the UI mark
    /// the selected row from an already-fetched `devices()` list without a second
    /// enumeration (which, on Linux, would re-spawn `pactl`).
    pub fn selected_device_uid(&self) -> Option<String> {
        self.device_manager.read().selected_uid().map(String::from)
    }

    pub fn devices(&self) -> Vec<device::OutputDeviceInfo> {
        self.device_manager
            .read()
            .output_devices()
            .unwrap_or_default()
    }

    pub fn select_device(&self, index: usize) -> Result<(), AudioError> {
        let was_exclusive = self.is_exclusive();
        let was_playing = self.is_playing();

        self.device_manager.write().select_device(index)?;

        // Switching to a different physical device — fully tear down the
        // existing output (release hog + restore rate on the OLD device)
        // before opening anything on the new device.
        let old = self.take_current();
        drop(old);

        let config = self.current_config();

        if was_exclusive {
            let device_uid = self.device_manager.write().resolve_uid()?;
            let buffer = Arc::new(AudioRingBuffer::new(calc_buffer_size(&config)));
            match exclusive::ExclusiveOutput::new(buffer, config, &device_uid, None) {
                Ok(excl) => {
                    if was_playing {
                        excl.resume();
                    }
                    *self.current.write() = Some(OutputMode::Exclusive(excl));
                    self.apply_current_volume();
                    Ok(())
                }
                Err(e) => {
                    self.install_shared_fallback(&config, was_playing);
                    Err(e)
                }
            }
        } else {
            let device = self.device_manager.write().resolve_device()?;
            let selected = SelectedOutputDevice {
                host: self.host.clone(),
                device,
            };
            let buffer = Arc::new(AudioRingBuffer::new(calc_buffer_size(&config)));
            match CpalOutputStream::new(buffer, config, selected) {
                Ok(stream) => {
                    if was_playing {
                        stream.resume();
                    }
                    *self.current.write() = Some(OutputMode::Shared(stream));
                    self.apply_current_volume();
                    Ok(())
                }
                // The chosen device couldn't open (e.g. busy, or an unsupported
                // format). The old stream is already torn down, so install a
                // fallback rather than leaving the output dead, and surface the
                // error so the UI can notify the user.
                Err(e) => {
                    self.install_shared_fallback(&config, was_playing);
                    Err(e)
                }
            }
        }
    }

    /// Quiesces playback and parks the output in a safe idle state.
    ///
    /// This is NOT a destructor: the `Output` instance keeps running so that
    /// subsequent UI-driven calls (`pause`, `resume`, `set_volume`, etc.) don't
    /// have to handle a torn-down state. The current track is dropped and any
    /// exclusive hold is released, but a minimal shared stream stays open on
    /// the system default device so the audio path remains valid.
    ///
    /// Called when the user clicks "stop" or closes the playback view — *not*
    /// on app exit (which is handled by `Drop`).
    pub fn shutdown(&self) {
        self.source_present.store(false, Ordering::Relaxed);
        self.pause();
        // Tear down the existing output first so the device is fully released
        // before we open a fresh fallback stream on it.
        let _ = self.take_current();
        if !self.install_shared_fallback(&DEFAULT_CONFIG, false)
            && !self.try_install_shared_on_default(&DEFAULT_CONFIG, false)
        {
            self.push_event(OutputEvent::Failure {
                message: "shutdown: failed to install fallback stream".to_string(),
            });
        }
    }

    /// Reacts to a `DeviceDisconnected` event by tearing down the dead exclusive
    /// output, clearing the selection (so future actions don't try the same
    /// ghost device), and installing a shared stream on the system default.
    ///
    /// This is the single source of recovery — UI must NOT separately call
    /// `set_exclusive(false)` after a disconnect.
    fn handle_device_disconnect(&self) {
        let was_playing = self.is_playing();
        let config = self.current_config();
        let _ = self.take_current();
        self.device_manager.write().select_default();

        if self.try_install_shared_on_default(&config, was_playing)
            || self.try_install_shared_on_default(&DEFAULT_CONFIG, was_playing)
        {
            self.push_event(OutputEvent::Recovered {
                message: "Output device disconnected; switched to system default.".to_string(),
            });
        } else {
            self.push_event(OutputEvent::Failure {
                message: "Output device disconnected and no fallback device is available."
                    .to_string(),
            });
        }
    }
}

fn is_config_match(config: &OutputConfig, metadata: &Metadata) -> bool {
    config.bit_depth == metadata.bit_depth
        && config.sample_rate == metadata.sample_rate
        && config.channels == metadata.channels.to_u8()
}

impl Default for Output {
    fn default() -> Self {
        Self::new()
    }
}

impl Output {
    /// Returns the current bit-perfect status. No syscalls — reads only atomics.
    pub fn bit_perfect_status(&self) -> BitPerfectStatus {
        if !self.source_present.load(Ordering::Relaxed) {
            return BitPerfectStatus {
                issues: vec![BitPerfectIssue::NoSource],
            };
        }

        let source_rate = self.source_sample_rate.load(Ordering::Relaxed);
        let source_bits = self.source_bit_depth.load(Ordering::Relaxed);
        let mut issues = Vec::new();

        match self.current.read().as_ref() {
            Some(OutputMode::Shared(_)) | None => {
                issues.push(BitPerfectIssue::NotExclusive);
            }
            Some(OutputMode::Exclusive(excl)) => {
                let snap = excl.device_snapshot();
                if snap.hw_volume < 1.0 - UNITY_VOLUME_TOLERANCE {
                    issues.push(BitPerfectIssue::SystemVolumeNotUnity {
                        current: snap.hw_volume,
                    });
                }
                if snap.hw_muted {
                    issues.push(BitPerfectIssue::SystemMuted);
                }
                if snap.app_volume < 1.0 - UNITY_VOLUME_TOLERANCE {
                    issues.push(BitPerfectIssue::AppVolumeNotUnity {
                        current: snap.app_volume,
                    });
                }
                if snap.device_sample_rate != 0 && snap.device_sample_rate != source_rate {
                    issues.push(BitPerfectIssue::SampleRateMismatch {
                        source: source_rate,
                        device: snap.device_sample_rate,
                    });
                }
            }
        }

        if source_bits > 24 {
            issues.push(BitPerfectIssue::BitDepthExceedsContainer {
                source: source_bits,
            });
        }

        BitPerfectStatus { issues }
    }
}

impl AudioOutput for Output {
    fn write(&self, batch: &AudioBatch) -> usize {
        // Update source-state atomics for bit-perfect tracking. We report the
        // claimed source bit depth from metadata — but if the decoder upcast
        // an integer source into F32 along the way, we lose precision: f32 has
        // only 24 mantissa bits, so anything > 24 *would* have exceeded that
        // container. Keep the claim so the indicator reflects the source.
        self.source_sample_rate
            .store(batch.metadata.sample_rate, Ordering::Relaxed);
        let effective_bit_depth = match &batch.data {
            // F32 sources are inherently float — they "fit" their container.
            // Use a sentinel value (24) so the >24 check doesn't fire for
            // legitimately-float source material like DSD-decoded-to-float.
            AudioSamples::F32(_) if batch.metadata.bit_depth <= 24 => batch.metadata.bit_depth,
            // Decoder-introduced F32: source claimed >24 bits, decoder lost precision.
            // Report the source's bit depth so the indicator flags it.
            _ => batch.metadata.bit_depth,
        };
        self.source_bit_depth
            .store(effective_bit_depth, Ordering::Relaxed);
        self.source_present.store(true, Ordering::Relaxed);

        let needs_recreate = {
            let current = self.current.read();
            match current.as_ref() {
                Some(OutputMode::Shared(s)) => !is_config_match(&s.config, &batch.metadata),
                Some(OutputMode::Exclusive(e)) => !is_config_match(&e.config, &batch.metadata),
                None => false,
            }
        };

        if needs_recreate {
            self.recreate_stream(batch.metadata.clone());
        }

        // Drain any events from the exclusive backend. Disconnect needs a full
        // recovery cycle (tear down + switch to default device), and we have to
        // do that OUTSIDE the read lock — otherwise take_current would deadlock.
        let mut disconnect = false;
        {
            let current = self.current.read();
            if let Some(OutputMode::Exclusive(excl)) = current.as_ref() {
                while let Some(evt) = excl.take_event() {
                    match evt {
                        exclusive::ExclusiveEvent::DeviceDisconnected => disconnect = true,
                    }
                }
            }
        }
        if disconnect {
            self.handle_device_disconnect();
        }

        match self.current.read().as_ref() {
            Some(OutputMode::Shared(s)) => s.write(batch),
            Some(OutputMode::Exclusive(e)) => e.write(batch),
            None => 0,
        }
    }

    fn clear(&self) {
        match self.current.read().as_ref() {
            Some(OutputMode::Shared(s)) => s.clear(),
            Some(OutputMode::Exclusive(e)) => e.clear(),
            None => {}
        }
    }

    fn pause(&self) {
        match self.current.read().as_ref() {
            Some(OutputMode::Shared(s)) => s.pause(),
            Some(OutputMode::Exclusive(e)) => e.pause(),
            None => {}
        }
    }

    fn resume(&self) {
        match self.current.read().as_ref() {
            Some(OutputMode::Shared(s)) => s.resume(),
            Some(OutputMode::Exclusive(e)) => e.resume(),
            None => {}
        }
    }

    fn is_playing(&self) -> bool {
        match self.current.read().as_ref() {
            Some(OutputMode::Shared(s)) => s.is_playing(),
            Some(OutputMode::Exclusive(e)) => e.is_playing(),
            None => false,
        }
    }

    fn set_volume(&self, volume: f32) {
        self.app_volume.store(volume, Ordering::Relaxed);
        self.apply_current_volume();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // These tests open a real audio stream on the system default output device.
    // They assert the contracts of the Output state machine (recovery is
    // automatic, set_exclusive(false) is infallible, etc.) on a host that
    // actually has an audio device — fine for macOS dev/CI, will fail-fast on
    // a headless box, which is what we want.

    #[test]
    fn new_lands_in_shared_mode_with_a_named_device() {
        let out = Output::new();
        assert!(
            !out.is_exclusive(),
            "fresh Output must start in shared mode"
        );
        assert!(
            !out.selected_device_name().is_empty(),
            "selected device must have a non-empty name"
        );
    }

    #[test]
    fn new_drains_no_events_and_has_no_source_status() {
        let out = Output::new();
        assert!(
            out.drain_events().is_empty(),
            "no events queued before any I/O"
        );
        let s = out.bit_perfect_status();
        assert!(
            s.issues.contains(&BitPerfectIssue::NoSource),
            "no track loaded → NoSource issue, got {:?}",
            s
        );
    }

    #[test]
    fn set_exclusive_false_is_infallible() {
        let out = Output::new();
        // Already in shared — turning exclusive off must succeed without producing
        // a hard error and must remain in shared mode.
        let res = out.set_exclusive(false);
        assert!(
            res.is_ok(),
            "set_exclusive(false) is contractually infallible"
        );
        assert!(!out.is_exclusive());
    }

    #[test]
    fn exclusive_toggle_returns_to_shared_cleanly() {
        let out = Output::new();
        // Best-effort: if exclusive can't be acquired (another app holds hog),
        // we still expect to remain in a working state and we expect
        // set_exclusive(false) to bring us back to shared without error.
        let _ = out.set_exclusive(true);
        let res = out.set_exclusive(false);
        assert!(res.is_ok());
        assert!(
            !out.is_exclusive(),
            "after exclusive-off we must be in shared mode"
        );
    }

    #[test]
    fn handle_device_disconnect_drops_to_shared_and_emits_one_event() {
        let out = Output::new();
        // Simulate the disconnect signal (we can't actually yank the audio
        // hardware in a test). The recovery code path is what matters.
        out.handle_device_disconnect();
        assert!(!out.is_exclusive());
        let events = out.drain_events();
        assert_eq!(
            events.len(),
            1,
            "exactly one user-visible event per disconnect, got {:?}",
            events
        );
        match &events[0] {
            OutputEvent::Recovered { .. } | OutputEvent::Failure { .. } => {}
        }
    }

    #[test]
    fn selected_device_index_is_none_or_a_valid_index() {
        let out = Output::new();
        // On a fresh Output we follow system default, so the index is None.
        // After explicit selection it would be Some — but we can't pick an
        // index without knowing the host enumeration, so just test the None case.
        assert_eq!(out.selected_device_index(), None);
        let devs = out.devices();
        assert!(
            !devs.is_empty(),
            "dev host should expose at least one device"
        );
    }

    #[test]
    fn pause_resume_on_fresh_output_does_not_panic() {
        let out = Output::new();
        out.pause();
        out.resume();
        out.pause();
        // Round-trip should not change exclusive state.
        assert!(!out.is_exclusive());
    }
}
