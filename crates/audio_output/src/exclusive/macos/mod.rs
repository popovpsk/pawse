pub(super) mod cf;
pub(super) mod format;
pub(super) mod hog;
pub(super) mod ioproc;
pub(super) mod listeners;
pub(super) mod sample_rate;
pub(super) mod sleep;

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use atomic_float::AtomicF32;
use audio_common::{AudioBatch, AudioError};
use objc2_core_audio::AudioDeviceIOProcID;

use crate::cpal_stream::{OutputConfig, PlaybackState};
use crate::ring_buffer::AudioRingBuffer;

use super::{Backend, DeviceSnapshot, ExclusiveEvent};
use format::{apply_format, read_device_format, set_and_wait_sample_rate};
use hog::{acquire_hog_mode, get_device_id_by_uid, get_hogging_pid, release_hog_mode};
use ioproc::{
    IoprocCtx, STATE_IDLE, STATE_PLAYING, create_ioproc, destroy_ioproc, start_ioproc, stop_ioproc,
};
use listeners::{
    register_format_listener, register_is_alive_listener, register_mute_listener,
    register_volume_listener, unregister_format_listener, unregister_is_alive_listener,
    unregister_mute_listener, unregister_volume_listener,
};
use sample_rate::get_available_samplerates;
use sleep::SleepPreventer;

// ----- Inner device state (protected by inner Mutex) -------------------------

struct MacosInner {
    device_id: u32,
    proc_id: AudioDeviceIOProcID,
    /// The "leaked" Arc<IoprocCtx> refcount held inside the IOProc registration.
    ioproc_ctx_raw: usize,
    original_rate: f64,
    hog_acquired: bool,
    playback_state: PlaybackState,
    /// The "leaked" Arc<MacosShared> refcount held inside the format listener registration.
    /// 0 means not registered.
    fmt_listener_raw: usize,
    /// Same for the device-is-alive listener.
    is_alive_listener_raw: usize,
    /// Leaked Arc refcounts for the volume scalar listener (one per registered element).
    vol_listener_raws: Vec<usize>,
    /// Leaked Arc refcounts for the mute listener (one per registered element).
    mute_listener_raws: Vec<usize>,
    sleep: SleepPreventer,
    /// Suppresses rate restoration + hog release during format-change reinit
    /// on the same device (avoids racing another app for hog mode mid-swap).
    suppress_cleanup: bool,
}

// ----- Shared state (accessible from listener callbacks via Arc) --------------

/// Soft cap on the events queue. If the UI never drains (e.g. window minimised
/// for a long time while the device generates events) we drop the oldest event
/// to keep memory bounded. Sized so disconnect+reconnect cycles fit easily.
const MAX_EVENTS: usize = 32;

pub(super) struct MacosShared {
    pub(super) events: Mutex<VecDeque<ExclusiveEvent>>,
    pub(super) alive: AtomicBool,
    /// Hardware output volume scalar in [0.0, 1.0]. 1.0 if device has no control.
    pub(super) hw_volume: AtomicF32,
    /// Hardware output mute state. false if device has no mute control.
    pub(super) hw_muted: AtomicBool,
    /// Device sample rate as integer Hz. Updated by the format-change listener.
    pub(super) device_sample_rate: AtomicU32,
    /// Channel count from the output config; used for per-channel volume writes.
    pub(super) channels: u8,
    inner: Mutex<MacosInner>,
    iopc_ctx: Arc<IoprocCtx>,
}

impl MacosShared {
    /// Pushes an event, dropping the oldest if the queue is over `MAX_EVENTS`.
    /// Used by the property-listener callbacks; the queue is drained by the
    /// main thread via `take_event`.
    pub(super) fn push_event(&self, evt: ExclusiveEvent) {
        let Ok(mut q) = self.events.lock() else {
            return;
        };
        if q.len() >= MAX_EVENTS {
            q.pop_front();
        }
        q.push_back(evt);
    }
}

// ----- Tear-down helper -------------------------------------------------------

fn tear_down_inner(inner: &mut MacosInner, shared: &MacosShared) {
    if inner.device_id == 0 {
        return;
    }

    stop_ioproc(inner.device_id, inner.proc_id);
    shared.iopc_ctx.playing.store(STATE_IDLE, Ordering::SeqCst);
    inner.sleep.allow();

    if inner.fmt_listener_raw != 0 {
        unregister_format_listener(inner.device_id, inner.fmt_listener_raw);
        inner.fmt_listener_raw = 0;
    }
    if inner.is_alive_listener_raw != 0 {
        unregister_is_alive_listener(inner.device_id, inner.is_alive_listener_raw);
        inner.is_alive_listener_raw = 0;
    }
    for raw in inner.vol_listener_raws.drain(..) {
        unregister_volume_listener(inner.device_id, raw);
    }
    for raw in inner.mute_listener_raws.drain(..) {
        unregister_mute_listener(inner.device_id, raw);
    }

    destroy_ioproc(inner.device_id, inner.proc_id, inner.ioproc_ctx_raw);

    if !inner.suppress_cleanup {
        if let Ok(asbd) = read_device_format(inner.device_id)
            && (asbd.mSampleRate - inner.original_rate).abs() > 0.5
        {
            let _ = set_and_wait_sample_rate(inner.device_id, inner.original_rate);
        }
        // Check the actual hogging PID rather than relying on hog_acquired.
        // After recreate_exclusive the new instance has hog_acquired=false because
        // acquire_hog_mode saw our PID already held hog (the old instance skipped
        // release via suppress_cleanup). The flag is wrong in that path, so we
        // verify ownership directly.
        let self_pid = std::process::id() as i32;
        if get_hogging_pid(inner.device_id).ok() == Some(self_pid) {
            release_hog_mode(inner.device_id);
        }
        inner.hog_acquired = false;
    }

    inner.device_id = 0;
    inner.proc_id = None;
    inner.ioproc_ctx_raw = 0;
    inner.playback_state = PlaybackState::Idle;
}

// ----- Init helper (no listeners) ---------------------------------------------

fn init_inner_no_listener(
    device_id: u32,
    config: OutputConfig,
    original_rate: Option<f64>,
    iopc_ctx: &Arc<IoprocCtx>,
) -> Result<MacosInner, AudioError> {
    let orig_rate = match original_rate {
        Some(r) => r,
        None => read_device_format(device_id)
            .map(|f| f.mSampleRate)
            .unwrap_or(44100.0),
    };

    let hog_acquired = acquire_hog_mode(device_id)?;

    let available_rates = get_available_samplerates(device_id).unwrap_or_default();

    if let Err(e) = apply_format(device_id, &config, &available_rates) {
        if hog_acquired {
            release_hog_mode(device_id);
        }
        return Err(e);
    }

    let (proc_id, ioproc_ctx_raw) = match create_ioproc(device_id, iopc_ctx.clone()) {
        Ok(pair) => pair,
        Err(e) => {
            if hog_acquired {
                release_hog_mode(device_id);
            }
            return Err(e);
        }
    };

    Ok(MacosInner {
        device_id,
        proc_id,
        ioproc_ctx_raw,
        original_rate: orig_rate,
        hog_acquired,
        playback_state: PlaybackState::Idle,
        fmt_listener_raw: 0,
        is_alive_listener_raw: 0,
        vol_listener_raws: Vec::new(),
        mute_listener_raws: Vec::new(),
        sleep: SleepPreventer::new(),
        suppress_cleanup: false,
    })
}

// ----- MacosBackend -----------------------------------------------------------

pub(crate) struct MacosBackend {
    shared: Arc<MacosShared>,
}

impl MacosBackend {
    pub(crate) fn new(
        buffer: Arc<AudioRingBuffer>,
        config: OutputConfig,
        device_uid: &str,
        original_rate: Option<f64>,
    ) -> Result<Self, AudioError> {
        let device_id = get_device_id_by_uid(device_uid)?;

        let iopc_ctx = Arc::new(IoprocCtx {
            buffer,
            volume: AtomicF32::new(1.0),
            playing: AtomicU8::new(STATE_IDLE),
        });

        // Step 1: init device state (without listeners yet)
        let inner = init_inner_no_listener(device_id, config, original_rate, &iopc_ctx)?;

        // Read initial device sample rate (best-effort; 0 if unavailable)
        let init_device_rate = read_device_format(device_id)
            .map(|f| f.mSampleRate as u32)
            .unwrap_or(0);

        // Step 2: wrap in Arc<MacosShared>
        let shared = Arc::new(MacosShared {
            events: Mutex::new(VecDeque::new()),
            alive: AtomicBool::new(true),
            hw_volume: AtomicF32::new(1.0),
            hw_muted: AtomicBool::new(false),
            device_sample_rate: AtomicU32::new(init_device_rate),
            channels: config.channels,
            inner: Mutex::new(inner),
            iopc_ctx,
        });

        // Step 3: register listeners using Arc clones
        {
            let mut locked = shared.inner.lock().unwrap();
            let dev = locked.device_id;
            locked.fmt_listener_raw = register_format_listener(dev, shared.clone());
            locked.is_alive_listener_raw = register_is_alive_listener(dev, shared.clone());
            locked.vol_listener_raws =
                register_volume_listener(dev, config.channels, shared.clone());
            locked.mute_listener_raws =
                register_mute_listener(dev, config.channels, shared.clone());
        }

        Ok(MacosBackend { shared })
    }
}

impl Drop for MacosBackend {
    fn drop(&mut self) {
        if let Ok(mut inner) = self.shared.inner.lock() {
            tear_down_inner(&mut inner, &self.shared);
        }
    }
}

impl Backend for MacosBackend {
    fn write(&self, batch: &AudioBatch) -> usize {
        if !self.shared.alive.load(Ordering::Relaxed) {
            return 0;
        }
        if self.shared.iopc_ctx.playing.load(Ordering::Relaxed) != STATE_PLAYING {
            return 0;
        }
        let f32_samples = batch.data.to_f32();
        self.shared
            .iopc_ctx
            .buffer
            .write_slice_blocking(&f32_samples)
    }

    fn clear(&self) {
        self.shared.iopc_ctx.buffer.clear();
    }

    fn pause(&self) {
        if let Ok(mut inner) = self.shared.inner.lock() {
            if inner.playback_state != PlaybackState::Playing {
                return;
            }
            stop_ioproc(inner.device_id, inner.proc_id);
            inner.sleep.allow();
            inner.playback_state = PlaybackState::Paused;
        }
        self.shared
            .iopc_ctx
            .playing
            .store(STATE_IDLE, Ordering::SeqCst);
    }

    fn resume(&self) {
        let started = {
            let mut inner = match self.shared.inner.lock() {
                Ok(g) => g,
                Err(_) => return,
            };
            if inner.playback_state == PlaybackState::Playing {
                return;
            }
            match start_ioproc(inner.device_id, inner.proc_id) {
                Ok(()) => {
                    inner.playback_state = PlaybackState::Playing;
                    inner.sleep.prevent();
                    true
                }
                Err(e) => {
                    eprintln!("coreaudio: resume failed: {}", e);
                    false
                }
            }
        };
        if started {
            self.shared
                .iopc_ctx
                .playing
                .store(STATE_PLAYING, Ordering::SeqCst);
        }
    }

    fn is_playing(&self) -> bool {
        self.shared
            .inner
            .lock()
            .map(|g| g.playback_state == PlaybackState::Playing)
            .unwrap_or(false)
    }

    fn set_volume(&self, volume: f32) {
        self.shared.iopc_ctx.volume.store(volume, Ordering::Relaxed);
    }

    fn set_hw_volume(&self, volume: f32) {
        let device_id = match self.shared.inner.lock() {
            Ok(g) => g.device_id,
            Err(_) => return,
        };
        listeners::set_hw_volume(device_id, self.shared.channels, volume);
    }

    fn is_alive(&self) -> bool {
        self.shared.alive.load(Ordering::SeqCst)
    }

    fn take_event(&self) -> Option<ExclusiveEvent> {
        self.shared.events.lock().ok()?.pop_front()
    }

    fn original_rate(&self) -> f64 {
        self.shared
            .inner
            .lock()
            .map(|g| g.original_rate)
            .unwrap_or(0.0)
    }

    fn suppress_cleanup(&self) {
        if let Ok(mut g) = self.shared.inner.lock() {
            g.suppress_cleanup = true;
        }
    }

    fn allow_cleanup(&self) {
        if let Ok(mut g) = self.shared.inner.lock() {
            g.suppress_cleanup = false;
        }
    }

    fn device_snapshot(&self) -> DeviceSnapshot {
        DeviceSnapshot {
            hw_volume: self.shared.hw_volume.load(Ordering::Relaxed),
            hw_muted: self.shared.hw_muted.load(Ordering::Relaxed),
            device_sample_rate: self.shared.device_sample_rate.load(Ordering::Relaxed),
            app_volume: self.shared.iopc_ctx.volume.load(Ordering::Relaxed),
        }
    }
}

/// Releases hog mode and restores the device sample rate.
///
/// Called when `suppress_cleanup` was set on the old exclusive output before dropping
/// it and then the new exclusive setup fails. In that case the old Drop skipped hog
/// release and rate restoration, so we do it here.
pub(super) fn restore_device_state(uid: &str, orig_rate: f64) {
    if let Ok(device_id) = hog::get_device_id_by_uid(uid) {
        let _ = format::set_nominal_sample_rate(device_id, orig_rate);
        hog::release_hog_mode(device_id);
    }
}
