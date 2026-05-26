mod device;
mod format;
mod volume;

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use alsa::Direction;
use alsa::mixer::{Mixer, SelemId};
use alsa::pcm::PCM;
use atomic_float::AtomicF32;
use audio_common::{AudioBatch, AudioError};

use super::render::{RenderCtx, STATE_IDLE, STATE_PLAYING, fill};
use super::{Backend, DeviceSnapshot, ExclusiveEvent};
use crate::cpal_stream::OutputConfig;
use crate::ring_buffer::AudioRingBuffer;
use format::{DeviceFormat, FmtKind};

const MAX_EVENTS: usize = 32;
/// Render iterations between mixer volume/mute refreshes (~twice a second at a
/// ~1024-frame period).
const VOLUME_REFRESH_EVERY: u32 = 20;
const I32_SCALE: f32 = 2_147_483_647.0;
const I16_SCALE: f32 = 32_767.0;

struct LinuxInner {
    thread: Option<JoinHandle<()>>,
}

struct LinuxShared {
    events: Mutex<VecDeque<ExclusiveEvent>>,
    alive: AtomicBool,
    hw_volume: AtomicF32,
    hw_muted: AtomicBool,
    device_sample_rate: AtomicU32,
    channels: u8,
    ctx: Arc<RenderCtx>,
    want_play: AtomicBool,
    pending_hw_volume: AtomicF32,
    running: AtomicBool,
    inner: Mutex<LinuxInner>,
}

impl LinuxShared {
    fn push_event(&self, evt: ExclusiveEvent) {
        let Ok(mut q) = self.events.lock() else {
            return;
        };
        if q.len() >= MAX_EVENTS {
            q.pop_front();
        }
        q.push_back(evt);
    }
}

// ----- Render-thread objects (owned solely by the thread) ---------------------

struct ThreadObjects {
    pcm: PCM,
    fmt: DeviceFormat,
    mixer: Option<(Mixer, SelemId)>,
}

fn setup(
    shared: &LinuxShared,
    uid: &str,
    config: &OutputConfig,
) -> Result<ThreadObjects, AudioError> {
    let (pcm_name, ctl_name) = device::resolve_names(uid);

    let pcm = PCM::new(&pcm_name, Direction::Playback, false)
        .map_err(|e| AudioError::DeviceNotFound(format!("open '{}': {}", pcm_name, e)))?;
    let fmt = format::configure(&pcm, config)?;

    let mixer = volume::open(&ctl_name);
    if let Some((m, id)) = &mixer {
        shared
            .hw_volume
            .store(volume::read_volume(m, id), Ordering::Relaxed);
        shared
            .hw_muted
            .store(volume::read_muted(m, id), Ordering::Relaxed);
    }
    shared
        .device_sample_rate
        .store(config.sample_rate, Ordering::Relaxed);

    Ok(ThreadObjects { pcm, fmt, mixer })
}

/// Writes a full period, looping over short writes (`writei` can return fewer
/// frames than requested, e.g. when interrupted by a signal). Returns on
/// completion or propagates a hard error for the caller to recover from.
fn write_all<S: Copy>(
    io: &alsa::pcm::IO<'_, S>,
    buf: &[S],
    channels: usize,
) -> Result<(), alsa::Error> {
    let total = buf.len() / channels;
    let mut done = 0usize;
    while done < total {
        let n = io.writei(&buf[done * channels..])?;
        if n == 0 {
            break; // avoid spinning if the device accepts nothing
        }
        done += n;
    }
    Ok(())
}

fn write_frames(
    pcm: &PCM,
    kind: FmtKind,
    f32buf: &[f32],
    i32buf: &mut [i32],
    i16buf: &mut [i16],
    channels: usize,
) -> Result<(), alsa::Error> {
    match kind {
        FmtKind::F32 => write_all(&pcm.io_f32()?, f32buf, channels),
        FmtKind::S32 => {
            for (d, s) in i32buf.iter_mut().zip(f32buf) {
                *d = (s.clamp(-1.0, 1.0) * I32_SCALE) as i32;
            }
            write_all(&pcm.io_i32()?, i32buf, channels)
        }
        FmtKind::S16 => {
            for (d, s) in i16buf.iter_mut().zip(f32buf) {
                *d = (s.clamp(-1.0, 1.0) * I16_SCALE) as i16;
            }
            write_all(&pcm.io_i16()?, i16buf, channels)
        }
    }
}

fn render_loop(shared: &LinuxShared, objs: ThreadObjects) {
    let ThreadObjects { pcm, fmt, mixer } = objs;
    let channels = shared.channels as usize;
    let n = fmt.period_frames * channels;

    let mut f32buf = vec![0.0f32; n];
    let mut i32buf = if matches!(fmt.kind, FmtKind::S32) {
        vec![0i32; n]
    } else {
        Vec::new()
    };
    let mut i16buf = if matches!(fmt.kind, FmtKind::S16) {
        vec![0i16; n]
    } else {
        Vec::new()
    };

    let mut started = false;
    let mut tick: u32 = 0;

    while shared.running.load(Ordering::Relaxed) {
        // Hardware-volume writes and reads are applied regardless of play state,
        // so adjusting device volume while paused takes effect immediately.
        let pending = shared.pending_hw_volume.swap(f32::NAN, Ordering::Relaxed);
        if !pending.is_nan()
            && let Some((m, id)) = &mixer
        {
            volume::set_volume(m, id, pending);
        }

        tick = tick.wrapping_add(1);
        if tick.is_multiple_of(VOLUME_REFRESH_EVERY)
            && let Some((m, id)) = &mixer
        {
            shared
                .hw_volume
                .store(volume::read_volume(m, id), Ordering::Relaxed);
            shared
                .hw_muted
                .store(volume::read_muted(m, id), Ordering::Relaxed);
        }

        if !shared.want_play.load(Ordering::Relaxed) {
            if started {
                let _ = pcm.drop();
                started = false;
            }
            std::thread::sleep(Duration::from_millis(10));
            continue;
        }

        if !started {
            let _ = pcm.prepare();
            started = true;
        }

        fill(&shared.ctx, &mut f32buf);

        if let Err(e) = write_frames(&pcm, fmt.kind, &f32buf, &mut i32buf, &mut i16buf, channels)
            && pcm.try_recover(e, true).is_err()
        {
            shared.alive.store(false, Ordering::SeqCst);
            shared.push_event(ExclusiveEvent::DeviceDisconnected);
            break;
        }
    }

    let _ = pcm.drop();
}

// ----- Backend ----------------------------------------------------------------

pub(crate) struct AlsaBackend {
    shared: Arc<LinuxShared>,
}

impl AlsaBackend {
    pub(crate) fn new(
        buffer: Arc<AudioRingBuffer>,
        config: OutputConfig,
        device_uid: &str,
        _original_rate: Option<f64>,
    ) -> Result<Self, AudioError> {
        let ctx = Arc::new(RenderCtx {
            buffer,
            volume: AtomicF32::new(1.0),
            playing: AtomicU8::new(STATE_IDLE),
            fade: crate::cpal_stream::FadeState::new(),
            sample_rate: config.sample_rate,
            channels: config.channels,
        });

        let shared = Arc::new(LinuxShared {
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
            inner: Mutex::new(LinuxInner { thread: None }),
        });

        let (tx, rx) = mpsc::channel::<Result<(), AudioError>>();
        let thread_shared = shared.clone();
        let uid = device_uid.to_string();
        let handle = std::thread::Builder::new()
            .name("alsa-exclusive".into())
            .spawn(move || match setup(&thread_shared, &uid, &config) {
                Ok(objs) => {
                    let _ = tx.send(Ok(()));
                    render_loop(&thread_shared, objs);
                }
                Err(e) => {
                    let _ = tx.send(Err(e));
                }
            })
            .map_err(|e| AudioError::Output(format!("spawn alsa thread: {}", e)))?;

        match rx.recv() {
            Ok(Ok(())) => {
                shared.inner.lock().unwrap().thread = Some(handle);
                Ok(AlsaBackend { shared })
            }
            Ok(Err(e)) => {
                let _ = handle.join();
                Err(e)
            }
            Err(_) => {
                let _ = handle.join();
                Err(AudioError::Output(
                    "alsa setup thread exited unexpectedly".to_string(),
                ))
            }
        }
    }
}

impl Drop for AlsaBackend {
    fn drop(&mut self) {
        self.shared.running.store(false, Ordering::SeqCst);
        self.shared.want_play.store(false, Ordering::SeqCst);
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

impl Backend for AlsaBackend {
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
    }

    fn resume(&self) {
        self.shared
            .ctx
            .playing
            .store(STATE_PLAYING, Ordering::SeqCst);
        self.shared.want_play.store(true, Ordering::SeqCst);
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
    }

    fn is_alive(&self) -> bool {
        self.shared.alive.load(Ordering::SeqCst)
    }

    fn take_event(&self) -> Option<ExclusiveEvent> {
        self.shared.events.lock().ok()?.pop_front()
    }

    fn original_rate(&self) -> f64 {
        // Opening a hw: PCM does not persistently change device configuration.
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
