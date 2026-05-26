mod device;
mod format;
mod sleep;
mod volume;

use std::collections::VecDeque;
use std::slice;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use atomic_float::AtomicF32;
use audio_common::{AudioBatch, AudioError};
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::Media::Audio::{
    AUDCLNT_E_DEVICE_INVALIDATED, IAudioClient, IAudioRenderClient,
};
use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize};
use windows::Win32::System::Threading::{CreateEventW, SetEvent, WaitForSingleObject};
use windows::core::PCWSTR;

use super::render::{RenderCtx, STATE_IDLE, STATE_PLAYING, fill};
use super::{Backend, DeviceSnapshot, ExclusiveEvent};
use crate::cpal_stream::OutputConfig;
use crate::ring_buffer::AudioRingBuffer;
use format::SampleFmt;
use sleep::SleepPreventer;

const MAX_EVENTS: usize = 32;
/// How often (in render iterations) the thread re-reads the endpoint volume/mute
/// into the snapshot atomics. At ~10 ms periods this is roughly twice a second.
const VOLUME_REFRESH_EVERY: u32 = 50;

// ----- Send-safe wrapper for the wake event handle ---------------------------

struct EventHandle(HANDLE);
// The HANDLE is a plain kernel event object (not COM); sharing it across threads
// for SetEvent / WaitForSingleObject is safe.
unsafe impl Send for EventHandle {}
unsafe impl Sync for EventHandle {}

impl Drop for EventHandle {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

// ----- Shared state -----------------------------------------------------------

struct WasapiInner {
    thread: Option<JoinHandle<()>>,
}

struct WasapiShared {
    events: Mutex<VecDeque<ExclusiveEvent>>,
    alive: AtomicBool,
    hw_volume: AtomicF32,
    hw_muted: AtomicBool,
    device_sample_rate: AtomicU32,
    channels: u8,
    ctx: Arc<RenderCtx>,
    /// Desired playback state, reconciled to IAudioClient Start/Stop by the thread.
    want_play: AtomicBool,
    /// Pending hardware-volume write (NaN = none); applied by the render thread.
    pending_hw_volume: AtomicF32,
    running: AtomicBool,
    event: EventHandle,
    inner: Mutex<WasapiInner>,
}

impl WasapiShared {
    fn push_event(&self, evt: ExclusiveEvent) {
        let Ok(mut q) = self.events.lock() else {
            return;
        };
        if q.len() >= MAX_EVENTS {
            q.pop_front();
        }
        q.push_back(evt);
    }

    fn wake(&self) {
        unsafe {
            let _ = SetEvent(self.event.0);
        }
    }
}

// ----- COM objects owned solely by the render thread --------------------------

struct ThreadObjects {
    client: IAudioClient,
    render: IAudioRenderClient,
    endpoint: Option<windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume>,
    buffer_frames: u32,
    sample_fmt: SampleFmt,
}

// ----- f32 -> device-format conversion ---------------------------------------

#[inline]
fn f32_to_i32(x: f32) -> i32 {
    (x.clamp(-1.0, 1.0) as f64 * i32::MAX as f64).round() as i32
}

#[inline]
fn f32_to_i16(x: f32) -> i16 {
    (x.clamp(-1.0, 1.0) * i16::MAX as f32).round() as i16
}

/// Fills `ptr` (the WASAPI render buffer for `frame_samples` interleaved
/// samples) with the next audio block, converting from internal f32 to the
/// device's negotiated sample format. `scratch` is reused across calls to hold
/// the f32 block for the integer paths.
fn fill_convert(
    ctx: &RenderCtx,
    fmt: SampleFmt,
    ptr: *mut u8,
    frame_samples: usize,
    scratch: &mut [f32],
) {
    match fmt {
        SampleFmt::F32 => {
            let out = unsafe { slice::from_raw_parts_mut(ptr as *mut f32, frame_samples) };
            fill(ctx, out);
        }
        SampleFmt::S32 => {
            fill(ctx, scratch);
            let out = unsafe { slice::from_raw_parts_mut(ptr as *mut i32, frame_samples) };
            for (o, &s) in out.iter_mut().zip(scratch.iter()) {
                *o = f32_to_i32(s);
            }
        }
        SampleFmt::S24In32 => {
            // 24 valid bits left-justified in the 32-bit container (low byte 0).
            fill(ctx, scratch);
            let out = unsafe { slice::from_raw_parts_mut(ptr as *mut i32, frame_samples) };
            for (o, &s) in out.iter_mut().zip(scratch.iter()) {
                *o = f32_to_i32(s) & !0xFF;
            }
        }
        SampleFmt::S16 => {
            fill(ctx, scratch);
            let out = unsafe { slice::from_raw_parts_mut(ptr as *mut i16, frame_samples) };
            for (o, &s) in out.iter_mut().zip(scratch.iter()) {
                *o = f32_to_i16(s);
            }
        }
    }
}

/// Resolves the device, negotiates the exclusive f32 format, and wires the event
/// handle. Runs entirely on the render thread (MTA), so the COM objects never
/// cross apartment boundaries.
fn setup(
    shared: &WasapiShared,
    uid: &str,
    config: &OutputConfig,
) -> Result<ThreadObjects, AudioError> {
    let enumerator = device::create_enumerator()?;
    let dev = device::resolve_device(&enumerator, uid)?;
    let init = format::negotiate_and_init(&dev, config)?;

    let render: IAudioRenderClient = unsafe { init.client.GetService() }
        .map_err(|e| AudioError::Output(format!("GetService(IAudioRenderClient): {}", e)))?;
    unsafe { init.client.SetEventHandle(shared.event.0) }
        .map_err(|e| AudioError::Output(format!("SetEventHandle: {}", e)))?;

    let endpoint = volume::activate(&dev);
    if let Some(ep) = &endpoint {
        shared
            .hw_volume
            .store(volume::read_volume(ep), Ordering::Relaxed);
        shared
            .hw_muted
            .store(volume::read_muted(ep), Ordering::Relaxed);
    }
    shared
        .device_sample_rate
        .store(init.sample_rate, Ordering::Relaxed);

    Ok(ThreadObjects {
        client: init.client,
        render,
        endpoint,
        buffer_frames: init.buffer_frames,
        sample_fmt: init.sample_fmt,
    })
}

fn render_loop(shared: &WasapiShared, objs: ThreadObjects) {
    let ThreadObjects {
        client,
        render,
        endpoint,
        buffer_frames,
        sample_fmt,
    } = objs;
    let channels = shared.channels as usize;
    let frame_samples = buffer_frames as usize * channels;
    let mut scratch = vec![0f32; frame_samples];
    let mut sleep = SleepPreventer::new();
    let mut started = false;
    let mut tick: u32 = 0;

    while shared.running.load(Ordering::Relaxed) {
        let want = shared.want_play.load(Ordering::Relaxed);

        if want && !started {
            // Prime one buffer before starting so the first event has data.
            if let Ok(ptr) = unsafe { render.GetBuffer(buffer_frames) } {
                fill_convert(&shared.ctx, sample_fmt, ptr, frame_samples, &mut scratch);
                let _ = unsafe { render.ReleaseBuffer(buffer_frames, 0) };
            }
            if unsafe { client.Start() }.is_ok() {
                started = true;
                sleep.prevent();
            }
        } else if !want && started {
            let _ = unsafe { client.Stop() };
            // Flush queued frames so resume re-primes fresh data (and a clear()
            // during pause isn't undone by stale buffered audio replaying).
            let _ = unsafe { client.Reset() };
            started = false;
            sleep.allow();
        }

        let pending = shared.pending_hw_volume.swap(f32::NAN, Ordering::Relaxed);
        if !pending.is_nan()
            && let Some(ep) = &endpoint
        {
            volume::set_volume(ep, pending);
        }

        if !started {
            unsafe { WaitForSingleObject(shared.event.0, 100) };
            continue;
        }

        unsafe { WaitForSingleObject(shared.event.0, 200) };
        if !shared.running.load(Ordering::Relaxed) || !shared.want_play.load(Ordering::Relaxed) {
            continue;
        }

        match unsafe { render.GetBuffer(buffer_frames) } {
            Ok(ptr) => {
                fill_convert(&shared.ctx, sample_fmt, ptr, frame_samples, &mut scratch);
                let _ = unsafe { render.ReleaseBuffer(buffer_frames, 0) };
            }
            Err(e) if e.code() == AUDCLNT_E_DEVICE_INVALIDATED => {
                shared.alive.store(false, Ordering::SeqCst);
                shared.push_event(ExclusiveEvent::DeviceDisconnected);
                break;
            }
            Err(_) => {} // transient — try again next period
        }

        tick = tick.wrapping_add(1);
        if tick.is_multiple_of(VOLUME_REFRESH_EVERY)
            && let Some(ep) = &endpoint
        {
            shared
                .hw_volume
                .store(volume::read_volume(ep), Ordering::Relaxed);
            shared
                .hw_muted
                .store(volume::read_muted(ep), Ordering::Relaxed);
        }
    }

    if started {
        let _ = unsafe { client.Stop() };
    }
}

// ----- Backend ----------------------------------------------------------------

pub(crate) struct WasapiBackend {
    shared: Arc<WasapiShared>,
}

impl WasapiBackend {
    pub(crate) fn new(
        buffer: Arc<AudioRingBuffer>,
        config: OutputConfig,
        device_uid: &str,
        _original_rate: Option<f64>,
    ) -> Result<Self, AudioError> {
        let event = unsafe { CreateEventW(None, false, false, PCWSTR::null()) }
            .map_err(|e| AudioError::Output(format!("CreateEventW: {}", e)))?;

        let ctx = Arc::new(RenderCtx {
            buffer,
            volume: AtomicF32::new(1.0),
            playing: std::sync::atomic::AtomicU8::new(STATE_IDLE),
            fade: crate::cpal_stream::FadeState::new(),
            sample_rate: config.sample_rate,
            channels: config.channels,
        });

        let shared = Arc::new(WasapiShared {
            events: Mutex::new(VecDeque::new()),
            alive: AtomicBool::new(true),
            hw_volume: AtomicF32::new(1.0),
            hw_muted: AtomicBool::new(false),
            device_sample_rate: AtomicU32::new(0),
            channels: config.channels,
            ctx,
            want_play: AtomicBool::new(false),
            pending_hw_volume: AtomicF32::new(f32::NAN),
            running: AtomicBool::new(true),
            event: EventHandle(event),
            inner: Mutex::new(WasapiInner { thread: None }),
        });

        // Spawn the render thread; it performs all COM work (MTA) and reports the
        // setup result back so `new` can fail synchronously for shared fallback.
        let (tx, rx) = mpsc::channel::<Result<(), AudioError>>();
        let thread_shared = shared.clone();
        let uid = device_uid.to_string();
        let handle = std::thread::Builder::new()
            .name("wasapi-exclusive".into())
            .spawn(move || {
                let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
                match setup(&thread_shared, &uid, &config) {
                    Ok(objs) => {
                        let _ = tx.send(Ok(()));
                        render_loop(&thread_shared, objs);
                    }
                    Err(e) => {
                        let _ = tx.send(Err(e));
                    }
                }
                unsafe { CoUninitialize() };
            })
            .map_err(|e| AudioError::Output(format!("spawn wasapi thread: {}", e)))?;

        match rx.recv() {
            Ok(Ok(())) => {
                shared.inner.lock().unwrap().thread = Some(handle);
                Ok(WasapiBackend { shared })
            }
            Ok(Err(e)) => {
                let _ = handle.join();
                Err(e)
            }
            Err(_) => {
                let _ = handle.join();
                Err(AudioError::Output(
                    "wasapi setup thread exited unexpectedly".to_string(),
                ))
            }
        }
    }
}

impl Drop for WasapiBackend {
    fn drop(&mut self) {
        self.shared.running.store(false, Ordering::SeqCst);
        self.shared.want_play.store(false, Ordering::SeqCst);
        self.shared.wake();
        let handle = self
            .shared
            .inner
            .lock()
            .ok()
            .and_then(|mut g| g.thread.take());
        if let Some(h) = handle {
            let _ = h.join();
        }
    }
}

impl Backend for WasapiBackend {
    fn write(&self, batch: &AudioBatch) -> usize {
        if !self.shared.alive.load(Ordering::Relaxed) {
            return 0;
        }
        if self.shared.ctx.playing.load(Ordering::Relaxed) != STATE_PLAYING {
            return 0;
        }
        let f32_samples = batch.data.to_f32();
        self.shared.ctx.buffer.write_slice_blocking(&f32_samples)
    }

    fn clear(&self) {
        self.shared.ctx.buffer.clear();
    }

    fn pause(&self) {
        self.shared.want_play.store(false, Ordering::SeqCst);
        self.shared.ctx.playing.store(STATE_IDLE, Ordering::SeqCst);
        self.shared.wake();
    }

    fn resume(&self) {
        self.shared
            .ctx
            .playing
            .store(STATE_PLAYING, Ordering::SeqCst);
        self.shared.want_play.store(true, Ordering::SeqCst);
        self.shared.wake();
    }

    fn is_playing(&self) -> bool {
        self.shared.want_play.load(Ordering::Relaxed)
    }

    fn set_volume(&self, volume: f32) {
        self.shared.ctx.volume.store(volume, Ordering::Relaxed);
    }

    fn begin_fade(&self, start: Option<f32>, target: f32, duration_ms: u32) {
        let ctx = &self.shared.ctx;
        ctx.fade.begin(ctx.sample_rate, start, target, duration_ms);
    }

    fn take_fade_event(&self) -> Option<crate::FadeEvent> {
        self.shared.ctx.fade.take_event()
    }

    fn reset_fade(&self) {
        self.shared.ctx.fade.reset();
    }

    fn set_hw_volume(&self, volume: f32) {
        self.shared
            .pending_hw_volume
            .store(volume, Ordering::Relaxed);
        self.shared.wake();
    }

    fn is_alive(&self) -> bool {
        self.shared.alive.load(Ordering::SeqCst)
    }

    fn take_event(&self) -> Option<ExclusiveEvent> {
        self.shared.events.lock().ok()?.pop_front()
    }

    fn original_rate(&self) -> f64 {
        // WASAPI exclusive does not persistently change device config.
        0.0
    }

    fn suppress_cleanup(&self) {}
    fn allow_cleanup(&self) {}

    fn device_snapshot(&self) -> DeviceSnapshot {
        DeviceSnapshot {
            hw_volume: self.shared.hw_volume.load(Ordering::Relaxed),
            hw_muted: self.shared.hw_muted.load(Ordering::Relaxed),
            device_sample_rate: self.shared.device_sample_rate.load(Ordering::Relaxed),
            app_volume: self.shared.ctx.volume.load(Ordering::Relaxed),
        }
    }
}
