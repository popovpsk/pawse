# Exclusive Audio Output Mode (Bit-Perfect / Direct DAC)

## Goal

Add a user-selectable exclusive audio output mode that bypasses the macOS system mixer.
When enabled, Audio MIDI Setup settings (volume, sample rate conversion) no longer affect the sound.
The output is bit-perfect: the device receives audio at the exact sample rate and bit depth
of the source material, with no resampling, no system volume mixing, no format conversion
by the mixer.

In exclusive mode, software volume control is disabled — volume is always 1.0.
The user controls volume on their external DAC/amp.

## Background

macOS CoreAudio provides **hog mode** (`kAudioDevicePropertyHogMode`) — exclusive access
to an audio device. When a process takes hog mode:

- System mixer is bypassed: no volume attenuation from Audio MIDI Setup
- No other application can use the device
- Device sample rate can be set directly (not locked to the system default)
- Audio arrives at the DAC in its native format, bit-perfect

Other applications using hog mode: mpv (`ao_coreaudio_exclusive`), Spotify, Tidal, Audirvana.

### What cpal does (current state)

cpal's CoreAudio backend always uses an `AudioUnit` in shared mode. The AudioUnit feeds
through the system mixer, meaning:

- System volume affects output
- System default sample rate forces resampling of all audio
- Format conversion happens inside AudioUnit

cpal v0.17.3 has **zero support** for exclusive mode on macOS.

### Why coreaudio-rs

The `coreaudio-rs` crate already provides all necessary helper functions wrapped in safe Rust:

| Function | Purpose |
|---|---|
| `get_audio_device_ids_for_scope(output)` | List output devices |
| `get_device_name(device_id)` | Human-readable device name |
| `toggle_hog_mode(device_id)` | Acquire/release exclusive access |
| `get_hogging_pid(device_id)` | Check who owns hog mode |
| `set_device_sample_rate(device_id, rate)` | Change device sample rate directly |
| `get_supported_physical_stream_formats(device_id)` | List physical formats a device supports |

`coreaudio-rs` is already a transitive dependency of `cpal`, so no version conflicts.

---

## Architecture

### Design Principles

1. **No `#[cfg]` in business logic.** All conditional compilation is isolated to the
   `exclusive/` subdirectory and one `mod` declaration in `output.rs`.
   `cpal_stream.rs` has zero `#[cfg]` — it is pure cross-platform cpal code.

2. **`AudioOutput` trait is the contract.** `CpalOutputStream` and `ExclusiveOutput` both
   implement `AudioOutput`. The `Output` manager holds one of them and delegates.

3. **`Output` is a manager, not a stream.** It decides which implementation to use
   based on user preference (shared vs exclusive). It handles device selection, mode
   switching, and stream recreation.

4. **Exclusive mode = no software volume.** In exclusive mode, `set_volume()` is a no-op.
   Volume is always 1.0 — the user controls volume on their DAC/amp.

5. **Device change = full recreation.** Whether switching devices or switching modes,
   the entire output subsystem is recreated from scratch. This matches the existing
   pattern where even a bit depth change triggers full stream recreation.

### Directory Structure After Refactor

```
crates/audio_output/src/
├── output.rs              — Output manager: delegates to CpalOutputStream or ExclusiveOutput
├── cpal_stream.rs         — CpalOutputStream: pure cpal, zero #[cfg], RENAMED from output_stream.rs
├── ring_buffer.rs         — AudioRingBuffer (unchanged)
├── device.rs              — DeviceManager: cpal device listing + macOS AudioDeviceID lookup
└── exclusive/
    ├── mod.rs              — pub re-exports platform module (macos or unsupported)
    ├── macos.rs            — ExclusiveOutput via coreaudio-rs AudioUnit + hog mode
    └── unsupported.rs      — Compile-time stub: ExclusiveOutput::new always returns Err
```

### Component Diagram

```
┌─────────────────────────────────────────────────────────────────┐
│                         Output (manager)                         │
│                                                                 │
│  mode: Shared ──────────┐                                       │
│                          ▼                                       │
│                   CpalOutputStream     (cpal_stream.rs)          │
│                   - AudioUnit via cpal                             │
│                   - Software volume curve                         │
│                   - Recreates on format change                    │
│                          ▲                                       │
│  mode: Exclusive ────────┘                                       │
│                          ▼                                       │
│                   ExclusiveOutput       (exclusive/macos.rs)      │
│                   - AudioUnit via coreaudio-rs directly           │
│                   - Hog mode acquired on device                   │
│                   - Device sample rate set to match source        │
│                   - Volume = 1.0 always                          │
│                   - Recreates on format change (rate change too) │
│                                                                 │
│  DeviceManager ───────────────────────────────────────────────    │
│  - Lists output devices via cpal                                 │
│  - Maps cpal Device → macOS AudioDeviceID (index + name)         │
│  - Tracks selected device                                        │
│  - #[cfg(macos)] stores AudioDeviceID for exclusive mode        │
│                                                                 │
└─────────────────────────────────────────────────────────────────┘
```

### Data Flow

```
AudioEngineLoop
    │
    ▼
Output.write(batch)        ← delegates to current implementation
    │
    ├── Shared mode → CpalOutputStream.write(batch)
    │       │
    │       ▼
    │   buffer.write_slice_blocking(f32_samples)
    │       │
    │       ▼
    │   cpal callback: buffer.pop_slice(data) → apply_volume → DAC
    │
    └── Exclusive mode → ExclusiveOutput.write(batch)
            │
            ▼
        buffer.write_slice_blocking(f32_samples)
            │
            ▼
        coreaudio-rs render callback: buffer.pop_slice(data) → DAC (no volume, bit-perfect)
```

---

## Implementation Plan

### Step 1: Rename `output_stream.rs` → `cpal_stream.rs`

Rename the file. Update all `pub use` and `use` statements.

**Before:**
```
crates/audio_output/src/output_stream.rs
```
**After:**
```
crates/audio_output/src/cpal_stream.rs
```

The struct inside renames from `OutputStream` to `CpalOutputStream`.
The public API of `audio_output` crate keeps `CpalOutputStream` exported,
but `OutputStream` is no longer the name — it becomes the manager type.

**File changes:**
- Rename file `output_stream.rs` → `cpal_stream.rs`
- In `cpal_stream.rs`: rename struct `OutputStream` → `CpalOutputStream`
- In `output.rs` (crate root): change `pub mod output_stream` → `pub mod cpal_stream`
- In `output.rs`: change `pub use output_stream::OutputStream` → `pub use cpal_stream::CpalOutputStream`
- In `audio_engine/src/engine.rs`: change `use audio_output::Output` (unchanged, `Output` is the manager)
- In `pawse/src/services.rs`: change `use audio_output::Output` (unchanged)
- In `audio_engine/src/test/main.rs`: change `use audio_output::Output` (unchanged)

### Step 2: Create `device.rs` — DeviceManager

This module handles device enumeration and selection using cpal (cross-platform).
On macOS, it also provides `AudioDeviceID` lookup for the exclusive mode module.

```rust
// crates/audio_output/src/device.rs

use std::sync::Arc;
use audio_common::AudioError;

pub struct OutputDeviceInfo {
    pub name: String,
    pub is_default: bool,
}

pub struct DeviceManager {
    host: Arc<cpal::Host>,
    selected_device: Arc<cpal::Device>,
    selected_device_name: String,
}

impl DeviceManager {
    pub fn new() -> Result<Self, AudioError> { ... }

    /// List all available output devices with metadata.
    pub fn output_devices(&self) -> Result<Vec<OutputDeviceInfo>, AudioError> { ... }

    /// Get the currently selected device.
    pub fn selected_device(&self) -> &Arc<cpal::Device> { ... }

    /// Select a device by index from the output_devices() list.
    /// Returns the new device. The caller is responsible for recreating the stream.
    pub fn select_device(&mut self, index: usize) -> Result<Arc<cpal::Device>, AudioError> { ... }

    /// Get the default output device.
    pub fn default_device(&self) -> Result<Arc<cpal::Device>, AudioError> { ... }
}
```

On macOS, `DeviceManager` also provides a method to resolve the `AudioDeviceID`
for the currently selected device. This is `#[cfg(target_os = "macos")]`:

```rust
#[cfg(target_os = "macos")]
impl DeviceManager {
    /// Resolve the CoreAudio AudioDeviceID for the selected device.
    ///
    /// Strategy:
    /// 1. Call cpal to enumerate output devices → Vec<Device>
    /// 2. Call coreaudio-rs to enumerate output device IDs + names
    /// 3. Match by index: the n-th device in cpal's list should be the
    ///    n-th device in coreaudio-rs's list (same CoreAudio backend)
    /// 4. Cross-validate by comparing names
    /// 5. On name mismatch at all indices, fall back to first name match
    /// 6. On complete failure, return AudioError
    pub fn audio_device_id(&self) -> Result<u32, AudioError> { ... }
}
```

**Why index-based matching is reliable:** Both cpal and `coreaudio-rs` enumerate devices
by querying the same CoreAudio system object (`kAudioObjectSystemObject` /
`kAudioHardwarePropertyDevices`). They return devices in the same stable order.
The name cross-check is a safety net.

### Step 3: Create `exclusive/mod.rs`

```rust
// crates/audio_output/src/exclusive/mod.rs

#[cfg(target_os = "macos")]
mod macos;

#[cfg(not(target_os = "macos"))]
mod unsupported;

#[cfg(target_os = "macos")]
pub use macos::ExclusiveOutput;

#[cfg(not(target_os = "macos"))]
pub use unsupported::ExclusiveOutput;
```

This is the ONLY place outside `exclusive/` where `#[cfg]` appears for platform selection.
Consumers just use `ExclusiveOutput` — the right implementation is selected at compile time.

### Step 4: Create `exclusive/unsupported.rs`

Stub for platforms without exclusive mode support.

```rust
// crates/audio_output/src/exclusive/unsupported.rs

use audio_common::AudioError;
use crate::cpal_stream::OutputConfig;
use crate::ring_buffer::AudioRingBuffer;

pub struct ExclusiveOutput;

impl ExclusiveOutput {
    pub fn new(
        _buffer: std::sync::Arc<AudioRingBuffer>,
        _config: OutputConfig,
        _audio_device_id: u32,
    ) -> Result<Self, AudioError> {
        Err(AudioError::UnsupportedFormat(
            "Exclusive mode is not supported on this platform".to_string(),
        ))
    }
}

impl crate::cpal_stream::AudioOutput for ExclusiveOutput {
    fn write(&self, _samples: &audio_common::AudioBatch) -> usize { 0 }
    fn clear(&self) {}
    fn pause(&self) {}
    fn resume(&self) {}
    fn is_playing(&self) -> bool { false }
    fn set_volume(&self, _volume: f32) {}
}
```

### Step 5: Create `exclusive/macos.rs`

This is the core exclusive mode implementation using `coreaudio-rs`.

#### 5.1 Structure

```rust
// crates/audio_output/src/exclusive/macos.rs

use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};
use parking_lot::RwLock;
use audio_common::{AudioBatch, AudioError};
use coreaudio::audio_unit::macos_helpers;

use crate::cpal_stream::{AudioOutput, OutputConfig, PlaybackState};
use crate::ring_buffer::AudioRingBuffer;
```

#### 5.2 Hog mode functions (private helpers)

```rust
/// Acquire hog mode on the device.
///
/// Returns Ok(()) if we acquired exclusive access.
/// Returns Err(AudioError) if another process already owns hog mode
///   or if the hog mode request failed.
fn acquire_hog_mode(device_id: u32) -> Result<(), AudioError> {
    // 1. Call macos_helpers::get_hogging_pid(device_id)
    //    If pid != -1 and pid != our process id → Err(DeviceBusy)
    // 2. Call macos_helpers::toggle_hog_mode(device_id)
    //    This acquires hog mode if nobody owns it, or releases if we own it
    // 3. Verify we now own it: get_hogging_pid should return our PID
    //    If not → Err (shouldn't happen, but handle gracefully)
}

/// Release hog mode on the device.
fn release_hog_mode(device_id: u32) -> Result<(), AudioError> {
    // Call macos_helpers::toggle_hog_mode(device_id)
    // This releases if we currently own hog mode.
    // On error, log but don't panic — macOS will release on process exit.
}
```

#### 5.3 Sample rate management (private helpers)

```rust
/// Get the current sample rate of an audio device.
fn get_device_sample_rate(device_id: u32) -> Result<f64, AudioError> {
    // Use AudioObjectGetPropertyData with kAudioDevicePropertyNominalSampleRate
    // via coreaudio-rs or direct FFI
    // OR use macos_helpers::get_device_sample_rate if available
}

/// Set the sample rate of an audio device.
fn set_device_sample_rate(device_id: u32, rate: f64) -> Result<(), AudioError> {
    // Use AudioObjectSetPropertyData with kAudioDevicePropertyNominalSampleRate
    // This is an async operation — must also set up a property listener
    // to wait for the rate change to take effect before proceeding.
    // macos_helpers::set_device_sample_rate exists but may not wait for the callback.
    // We need to:
    //   1. Add a property listener for kAudioDevicePropertyNominalSampleRate
    //   2. Set the rate
    //   3. Wait for the listener callback (with timeout)
    //   4. Remove the listener
}
```

**IMPORTANT: Sample rate change is asynchronous on macOS.** The pattern from cpal's
own CoreAudio backend is:

1. Add a property listener for `kAudioDevicePropertyNominalSampleRate`
2. Call `AudioObjectSetPropertyData` to set the new rate
3. Wait for the listener callback confirming the rate has changed
4. Remove the listener

The `macos_helpers::set_device_sample_rate` function in `coreaudio-rs` only does step 2
but does NOT wait for confirmation. We must implement the wait ourselves.
See cpal's own implementation in `src/host/coreaudio/macos/device.rs` for reference
(their `set_device_sample_rate` uses a `Condvar` to wait).

#### 5.4 Device ID matching

```rust
/// Find the CoreAudio AudioDeviceID corresponding to a cpal Device.
///
/// Enumerates output devices from both cpal and coreaudio-rs, matches by
/// index (same enumeration order), cross-validates by name.
pub fn find_audio_device_id(cpal_device: &cpal::Device) -> Result<u32, AudioError> {
    // 1. Get list of all output device IDs via macos_helpers
    // 2. Get list of all cpal output devices via host.output_devices()
    // 3. Find the index of cpal_device in the cpal list
    // 4. Use that same index to get the AudioDeviceID from the coreaudio-rs list
    // 5. Cross-validate: get_device_name(audio_device_id) should match cpal_device.name()
    // 6. On name mismatch, iterate coreaudio-rs list looking for matching name
    // 7. On no match at all → Err(DeviceNotFound)
}
```

#### 5.5 ExclusiveOutput struct

```rust
pub struct ExclusiveOutput {
    inner: RwLock<ExclusiveOutputInner>,
    buffer: Arc<AudioRingBuffer>,
    config: OutputConfig,
    audio_device_id: u32,
    original_sample_rate: f64,  // Device rate before we changed it, for restoration
}

struct ExclusiveOutputInner {
    state: PlaybackState,
    audio_unit: AudioUnit,  // coreaudio-rs AudioUnit
}

impl ExclusiveOutput {
    pub fn new(
        buffer: Arc<AudioRingBuffer>,
        config: OutputConfig,
        audio_device_id: u32,  // from DeviceManager::audio_device_id()
    ) -> Result<Self, AudioError> {
        // 1. Save original device sample rate
        //    original_sample_rate = get_device_sample_rate(audio_device_id)?

        // 2. Acquire hog mode
        //    acquire_hog_mode(audio_device_id)?

        // 3. Set device sample rate to match source
        //    let target_rate = config.sample_rate as f64;
        //    if target_rate != original_sample_rate {
        //        set_device_sample_rate(audio_device_id, target_rate)?;
        //    }

        // 4. Create AudioUnit via coreaudio-rs
        //    audio_unit = macos_helpers::audio_unit_from_device_id(audio_device_id, false)
        //    (false = output/playback, not input)
        //    Set the stream format on the AudioUnit to match config
        //    Set render callback that reads from our ring buffer

        // 5. Return Self { inner, buffer, config, audio_device_id, original_sample_rate }
    }
}
```

#### 5.6 AudioUnit render callback

The render callback is the heart of `ExclusiveOutput`. It reads from the ring buffer
and fills the AudioUnit's output buffer.

```rust
// The render callback:
fn render_callback(
    buffer: &Arc<AudioRingBuffer>,
    data: &mut [f32],
) -> OSStatus {
    let samples_read = buffer.pop_slice(data);
    // Fill remaining buffer with silence (no data available)
    for sample in &mut data[samples_read..] {
        *sample = 0.0;
    }
    // NO volume applied — bit-perfect output
    0  // noErr
}
```

#### 5.7 AudioOutput trait implementation

```rust
impl AudioOutput for ExclusiveOutput {
    fn write(&self, batch: &AudioBatch) -> usize {
        if self.inner.read().state != PlaybackState::Playing {
            return 0;
        }
        let f32_samples = batch.data.to_f32();
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
        inner.audio_unit.stop().unwrap();  // or appropriate pause method
        inner.state = PlaybackState::Paused;
    }

    fn resume(&self) {
        let mut inner = self.inner.write();
        if inner.state == PlaybackState::Playing {
            return;
        }
        inner.audio_unit.start().unwrap();  // or appropriate resume method
        inner.state = PlaybackState::Playing;
    }

    fn is_playing(&self) -> bool {
        self.inner.read().state == PlaybackState::Playing
    }

    fn set_volume(&self, _volume: f32) {
        // NO-OP in exclusive mode. Volume is always 1.0 (bit-perfect).
        // The user controls volume on their DAC/amp.
    }
}
```

#### 5.8 Drop implementation — cleanup

```rust
impl Drop for ExclusiveOutput {
    fn drop(&mut self) {
        // 1. Stop the audio unit
        //    self.inner.read().audio_unit.stop()  — best effort, ignore errors

        // 2. Release hog mode (best effort)
        //    let _ = release_hog_mode(self.audio_device_id);

        // 3. Restore original sample rate (best effort)
        //    if self.original_sample_rate != current_sample_rate {
        //        let _ = set_device_sample_rate(self.audio_device_id, self.original_sample_rate);
        //    }
    }
}
```

**NOTE:** Drop is a fallback cleanup. The primary cleanup path is through
`Output::shutdown()` which explicitly releases resources. Drop handles the
crash/panic case. macOS also auto-releases hog mode when the process dies.

### Step 6: Refactor `Output` into a manager

The `Output` struct changes from owning a `CpalOutputStream` directly to owning
an `OutputInner` enum that can be either shared or exclusive.

```rust
// crates/audio_output/src/output.rs

pub mod cpal_stream;
pub mod ring_buffer;
pub mod device;
#[cfg(target_os = "macos")]
pub mod exclusive;
#[cfg(not(target_os = "macos"))]
pub mod exclusive;

use std::sync::Arc;
use audio_common::{AudioBatch, AudioError, Metadata};
use cpal::traits::HostTrait;
use parking_lot::RwLock;

use crate::cpal_stream::{AudioOutput, CpalOutputStream, OutputConfig, SelectedOutputDevice};
use crate::device::DeviceManager;
use crate::ring_buffer::AudioRingBuffer;

enum OutputMode {
    Shared(CpalOutputStream),
    Exclusive(exclusive::ExclusiveOutput),
}

pub struct Output {
    host: Arc<cpal::Host>,
    device_manager: RwLock<DeviceManager>,
    current: RwLock<OutputMode>,
}

fn calc_buffer_size(cfg: &OutputConfig) -> usize {
    (cfg.bit_depth / 8) as usize
        * cfg.channels as usize
        * (cfg.sample_rate / 8) as usize
}
```

#### 6.1 Output::new()

```rust
impl Output {
    pub fn new() -> Self {
        let host = Arc::new(cpal::default_host());
        let device_manager = DeviceManager::from_host(&host)
            .expect("Failed to initialize device manager");
        let device = device_manager.selected_device().clone();

        let output_config = OutputConfig::default();
        let selected = SelectedOutputDevice { host: host.clone(), device };
        let buffer = Arc::new(AudioRingBuffer::new(calc_buffer_size(&output_config)));

        let stream = CpalOutputStream::new(buffer, output_config, selected)
            .expect("Failed to create audio output stream");

        Self {
            host,
            device_manager: RwLock::new(device_manager),
            current: RwLock::new(OutputMode::Shared(stream)),
        }
    }
}
```

#### 6.2 set_exclusive()

```rust
impl Output {
    /// Switch between shared and exclusive mode.
    /// This recreates the entire output stream.
    /// On error, falls back to the previous mode.
    pub fn set_exclusive(&self, exclusive: bool) -> Result<(), AudioError> {
        let was_playing = self.is_playing();
        let config = self.current_config();

        if exclusive {
            #[cfg(target_os = "macos")]
            {
                let audio_device_id = self.device_manager.read().audio_device_id()?;
                let buffer = Arc::new(AudioRingBuffer::new(calc_buffer_size(&config)));
                match exclusive::ExclusiveOutput::new(buffer, config, audio_device_id) {
                    Ok(exclusive_output) => {
                        let mut current = self.current.write();
                        drop_current(&*current);  // release shared stream
                        *current = OutputMode::Exclusive(exclusive_output);
                        if was_playing {
                            self.resume();
                        }
                        Ok(())
                    }
                    Err(e) => Err(e),
                }
            }
            #[cfg(not(target_os = "macos"))]
            {
                Err(AudioError::UnsupportedFormat(
                    "Exclusive mode is not supported on this platform".to_string(),
                ))
            }
        } else {
            // Switch to shared mode
            let device = self.device_manager.read().selected_device().clone();
            let selected = SelectedOutputDevice { host: self.host.clone(), device };
            let buffer = Arc::new(AudioRingBuffer::new(calc_buffer_size(&config)));
            let stream = CpalOutputStream::new(buffer, config, selected)?;

            let mut current = self.current.write();
            drop_current(&*current);
            *current = OutputMode::Shared(stream);
            if was_playing {
                self.resume();
            }
            Ok(())
        }
    }

    pub fn is_exclusive(&self) -> bool {
        matches!(*self.current.read(), OutputMode::Exclusive(_))
    }
}
```

#### 6.3 AudioOutput delegation

```rust
impl AudioOutput for Output {
    fn write(&self, batch: &AudioBatch) -> usize {
        match &*self.current.read() {
            OutputMode::Shared(s) => s.write(batch),
            OutputMode::Exclusive(e) => e.write(batch),
        }
    }

    fn clear(&self) {
        match &*self.current.read() {
            OutputMode::Shared(s) => s.clear(),
            OutputMode::Exclusive(e) => e.clear(),
        }
    }

    fn pause(&self) {
        match &*self.current.read() {
            OutputMode::Shared(s) => s.pause(),
            OutputMode::Exclusive(e) => e.pause(),
        }
    }

    fn resume(&self) {
        match &*self.current.read() {
            OutputMode::Shared(s) => s.resume(),
            OutputMode::Exclusive(e) => e.resume(),
        }
    }

    fn is_playing(&self) -> bool {
        match &*self.current.read() {
            OutputMode::Shared(s) => s.is_playing(),
            OutputMode::Exclusive(e) => e.is_playing(),
        }
    }

    fn set_volume(&self, volume: f32) {
        match &*self.current.read() {
            OutputMode::Shared(s) => s.set_volume(volume),
            OutputMode::Exclusive(_) => {
                // No-op in exclusive mode. Volume is always 1.0.
            }
        }
    }
}
```

#### 6.4 Stream recreation on format change

The existing logic in `Output::write()` checks if stream config matches metadata
and recreates if needed. This must work for both modes.

```rust
impl Output {
    fn current_config(&self) -> OutputConfig {
        match &*self.current.read() {
            OutputMode::Shared(s) => s.config,
            OutputMode::Exclusive(e) => e.config,
        }
    }
}

fn is_config_match(config: &OutputConfig, metadata: &Metadata) -> bool {
    config.bit_depth == metadata.bit_depth
        && config.sample_rate == metadata.sample_rate
        && config.channels == metadata.channels.to_u8()
}

impl AudioOutput for Output {
    fn write(&self, batch: &AudioBatch) -> usize {
        let needs_recreate = {
            let current = self.current.read();
            let config = match &*current {
                OutputMode::Shared(s) => &s.config,
                OutputMode::Exclusive(e) => &e.config,
            };
            !is_config_match(config, &batch.metadata)
        };

        if needs_recreate {
            self.recreate_stream(batch.metadata.clone());
        }

        match &*self.current.read() {
            OutputMode::Shared(s) => s.write(batch),
            OutputMode::Exclusive(e) => e.write(batch),
        }
    }
}
```

`recreate_stream` must also update the device sample rate in exclusive mode:

```rust
impl Output {
    fn recreate_stream(&self, metadata: Metadata) {
        let was_playing = self.is_playing();
        let new_config = OutputConfig {
            sample_rate: metadata.sample_rate,
            channels: metadata.channels.to_u8(),
            bit_depth: metadata.bit_depth,
        };

        match &*self.current.read() {
            OutputMode::Shared(_) => {
                let device = self.device_manager.read().selected_device().clone();
                let selected = SelectedOutputDevice { host: self.host.clone(), device };
                let buffer = Arc::new(AudioRingBuffer::new(calc_buffer_size(&new_config)));
                let stream = CpalOutputStream::new(buffer, new_config, selected)
                    .expect("Failed to create new audio output stream");
                if was_playing { stream.resume(); }
                let mut current = self.current.write();
                *current = OutputMode::Shared(stream);
            }
            OutputMode::Exclusive(_) => {
                // In exclusive mode, also set device sample rate
                #[cfg(target_os = "macos")]
                {
                    if let Ok(device_id) = self.device_manager.read().audio_device_id() {
                        let _ = exclusive::set_device_sample_rate(
                            device_id, new_config.sample_rate as f64
                        );
                    }
                }
                let buffer = Arc::new(AudioRingBuffer::new(calc_buffer_size(&new_config)));
                // Recreate the ExclusiveOutput
                #[cfg(target_os = "macos")]
                {
                    if let Ok(device_id) = self.device_manager.read().audio_device_id() {
                        if let Ok(excl) = exclusive::ExclusiveOutput::new(
                            buffer, new_config, device_id
                        ) {
                            if was_playing { excl.resume(); }
                            let mut current = self.current.write();
                            *current = OutputMode::Exclusive(excl);
                        }
                    }
                }
            }
        }
    }
}
```

### Step 7: Device selection API

Add methods to `Output` for device switching:

```rust
impl Output {
    /// List available output devices.
    pub fn devices(&self) -> Vec<OutputDeviceInfo> {
        self.device_manager.read().output_devices()
            .unwrap_or_default()
    }

    /// Switch to a different output device by index.
    /// This recreates the output stream for the new device.
    /// Returns Err if the index is out of bounds.
    pub fn select_device(&self, index: usize) -> Result<(), AudioError> {
        let is_exclusive = self.is_exclusive();
        let was_playing = self.is_playing();

        // Update device manager
        let new_device = self.device_manager.write().select_device(index)?;

        // Recreate stream for new device
        if is_exclusive {
            #[cfg(target_os = "macos")]
            {
                let device_id = self.device_manager.read().audio_device_id()?;
                let config = self.current_config();
                let buffer = Arc::new(AudioRingBuffer::new(calc_buffer_size(&config)));
                let excl = exclusive::ExclusiveOutput::new(buffer, config, device_id)?;
                let mut current = self.current.write();
                *current = OutputMode::Exclusive(excl);
            }
        } else {
            let selected = SelectedOutputDevice {
                host: self.host.clone(),
                device: new_device,
            };
            let config = self.current_config();
            let buffer = Arc::new(AudioRingBuffer::new(calc_buffer_size(&config)));
            let stream = CpalOutputStream::new(buffer, config, selected)?;
            let mut current = self.current.write();
            *current = OutputMode::Shared(stream);
        }

        if was_playing {
            self.resume();
        }

        Ok(())
    }
}
```

### Step 8: Shutdown

```rust
impl Output {
    /// Shut down the output, releasing exclusive mode if active.
    /// Call this before app exit to properly release hog mode and
    /// restore the device sample rate.
    pub fn shutdown(&self) {
        self.pause();
        // Drop the current stream — ExclusiveOutput's Drop impl
        // will release hog mode and restore sample rate.
        // For shared mode, Drop is a no-op beyond stopping the stream.
        let mut current = self.current.write();
        // Replacing with a dummy state; the old value's Drop runs
        // In practice, after shutdown the app exits, so this is
        // just for cleanliness.
    }
}
```

### Step 9: Update `Services::shutdown()`

In `crates/pawse/src/services.rs`:

```rust
impl Services {
    pub fn shutdown(&self) {
        self.output.shutdown();
        self.engine_manager.shutdown();
    }
}
```

### Step 10: Volume UI in exclusive mode

In `crates/pawse/src/volume.rs`, the `Volume` component should be visually disabled
or hidden when exclusive mode is active:

- `Output::is_exclusive()` returns `true` → show a lock icon or "Direct DAC" label
  instead of the volume slider
- `set_volume()` calls are no-ops on `ExclusiveOutput`, so existing code won't break

This can be a simple `is_exclusive` check on `Output` propagated through `Services`.

---

## Error Handling

| Scenario | Behavior |
|---|---|
| Device not found by index+name match | Return `AudioError::DeviceNotFound`, don't switch to exclusive |
| Another process owns hog mode | Return `AudioError::DeviceNotFound("Device is in use by another application")`, don't switch |
| Device doesn't support requested sample rate | Return `AudioError::UnsupportedFormat`, or try closest supported rate |
| Stream creation fails in exclusive mode | Fall back to shared mode, log error |
| Format change while exclusive | Recreate ExclusiveOutput; update device sample rate; on failure → log, continue |
| Sample rate change times out (async) | Return error, don't switch to exclusive |
| App crash | macOS auto-releases hog mode when the process exits (PID-based cleanup) |

## Hog Mode Lifecycle

| Event | Hog Mode Action |
|---|---|
| User enables exclusive mode | `acquire_hog_mode()` + `set_device_sample_rate()` to match source |
| User disables exclusive mode | Drop ExclusiveOutput → release hog mode + restore original sample rate |
| Track changes (different sample rate) | Recreate ExclusiveOutput → update device sample rate |
| Device change (user selects different DAC) | Release hog mode on old device → recreate everything on new device → acquire hog mode |
| App shutdown (clean) | Drop ExclusiveOutput → release hog mode + restore sample rate |
| App crash | macOS auto-releases hog mode (property is per-PID) |

## Non-macOS Platforms

All exclusive-mode code is behind `#[cfg(target_os = "macos")]` in `exclusive/macos.rs`.
The `exclusive/unsupported.rs` stub returns `AudioError::UnsupportedFormat` for all
operations. On Linux/Windows:

- `Output::set_exclusive(true)` returns `Err`
- `Output::is_exclusive()` always returns `false`
- `OutputMode::Exclusive` variant is never constructed
- `CpalOutputStream` is used exclusively (no pun intended)

## Files to Create or Modify

| File | Action | Key Changes |
|---|---|---|
| `crates/audio_output/src/output_stream.rs` | **Rename** → `cpal_stream.rs` | Rename struct `OutputStream` → `CpalOutputStream`, make `PlaybackState` pub |
| `crates/audio_output/src/cpal_stream.rs` | Rename result | Same content, struct rename, `PlaybackState` pub |
| `crates/audio_output/src/output.rs` | **Major refactor** | Manager pattern with `OutputMode` enum, `DeviceManager`, `set_exclusive()`, `select_device()`, `shutdown()`, `devices()` |
| `crates/audio_output/src/device.rs` | **New** | DeviceManager: cpal device listing, macOS AudioDeviceID resolution |
| `crates/audio_output/src/exclusive/mod.rs` | **New** | Platform module selection |
| `crates/audio_output/src/exclusive/macos.rs` | **New** | ExclusiveOutput: coreaudio-rs AudioUnit, hog mode, sample rate management |
| `crates/audio_output/src/exclusive/unsupported.rs` | **New** | Stub that always returns errors |
| `crates/audio_output/Cargo.toml` | **Modify** | Add `coreaudio-rs = "0.14"` as macOS dep |
| `crates/pawse/src/services.rs` | **Modify** | Add `output.shutdown()` to `Services::shutdown()` |
| `crates/pawse/src/volume.rs` | **Modify** | Check `is_exclusive()` to disable/label volume UI |
| `crates/audio_engine/src/engine.rs` | **Modify** | Update import: `OutputStream` → `CpalOutputStream` (if referenced directly) |

## Implementation Order

1. **Rename** `output_stream.rs` → `cpal_stream.rs`, rename `OutputStream` → `CpalOutputStream`, make `PlaybackState` pub. Update all imports across workspace.
2. **Create** `device.rs` — DeviceManager with cpal device listing and macOS AudioDeviceID lookup.
3. **Create** `exclusive/mod.rs`, `exclusive/unsupported.rs` — stub implementation.
4. **Create** `exclusive/macos.rs` — hog mode helpers, AudioUnit render callback, ExclusiveOutput struct.
5. **Refactor** `output.rs` — OutputMode enum, delegation, set_exclusive(), select_device(), shutdown().
6. **Update** `Cargo.toml` — add coreaudio-rs dependency.
7. **Update** `services.rs` — call `output.shutdown()`.
8. **Update** `volume.rs` — check is_exclusive() for UI state.
9. **Build, test, clippy** — `cargo build && cargo clippy && cargo test`.

## Testing

### Unit Tests (no audio hardware required)

- `CpalOutputStream` tests (existing) continue to work unchanged.
- `DeviceManager` tests: enumerate devices, verify default device is valid.
- `Output` manager tests: create in shared mode, verify `is_exclusive() == false`.

### Integration Tests (require audio hardware, macOS)

- Create `ExclusiveOutput` → verify hog mode acquired.
- Drop `ExclusiveOutput` → verify hog mode released.
- Set exclusive → verify device sample rate changed.
- Release exclusive → verify device sample rate restored.
- All tests MUST release hog mode in cleanup to avoid leaving the device locked.

### Manual Testing Checklist

1. Play a 44.1kHz FLAC in shared mode → verify sound via system mixer.
2. Switch to exclusive mode → verify sound continues, system volume no longer affects output.
3. Change track to 96kHz file → verify stream recreation, continuation of playback.
4. Switch back to shared mode → verify system volume works again.
5. Open Audio MIDI Setup → verify sample rate restored to original.
6. Try exclusive mode when another app holds the device → verify graceful error.
7. Kill the app while in exclusive mode → verify device is released (macOS cleanup).