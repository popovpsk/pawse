mod pcm;
mod status;

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use alsa::pcm::PCM;
use atomic_float::AtomicF32;
use audio_common::{AudioBatch, AudioError};

use super::render::{RenderCtx, STATE_IDLE, STATE_PLAYING, fill};
use super::{Backend, DeviceSnapshot, ExclusiveEvent};
use crate::cpal_stream::OutputConfig;
use crate::ring_buffer::AudioRingBuffer;
use pcm::DeviceFormat;
use status::StatusShared;

const MAX_EVENTS: usize = 32;

struct LinuxInner {
    writer: Option<JoinHandle<()>>,
    monitor: Option<JoinHandle<()>>,
}

struct LinuxShared {
    events: Mutex<VecDeque<ExclusiveEvent>>,
    alive: AtomicBool,
    channels: u8,
    ctx: Arc<RenderCtx>,
    want_play: AtomicBool,
    running: AtomicBool,
    status: Arc<StatusShared>,
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

/// Writes a full period, looping over short writes (`writei` can return fewer
/// frames than requested, e.g. when interrupted by a signal). Returns on
/// completion or propagates a hard error for the caller to recover from.
fn write_all(pcm: &PCM, buf: &[f32], channels: usize) -> Result<(), alsa::Error> {
    let io = pcm.io_f32()?;
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

fn render_loop(shared: &LinuxShared, pcm: PCM, fmt: DeviceFormat) {
    let channels = shared.channels as usize;
    let mut buf = vec![0.0f32; fmt.period_frames * channels];
    let mut started = false;

    while shared.running.load(Ordering::Relaxed) {
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

        fill(&shared.ctx, &mut buf);

        if let Err(e) = write_all(&pcm, &buf, channels)
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

pub(crate) struct PipewireBackend {
    shared: Arc<LinuxShared>,
}

impl PipewireBackend {
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
            channels: config.channels,
            ctx,
            want_play: AtomicBool::new(false),
            running: AtomicBool::new(true),
            status: Arc::new(StatusShared::new()),
            inner: Mutex::new(LinuxInner {
                writer: None,
                monitor: None,
            }),
        });

        let (tx, rx) = mpsc::channel::<Result<(), AudioError>>();
        let thread_shared = shared.clone();
        let uid = device_uid.to_string();
        let writer = std::thread::Builder::new()
            .name("pw-exclusive".into())
            .spawn(move || match pcm::open(&uid, &config) {
                Ok((pcm, fmt)) => {
                    let _ = tx.send(Ok(()));
                    render_loop(&thread_shared, pcm, fmt);
                }
                Err(e) => {
                    let _ = tx.send(Err(e));
                }
            })
            .map_err(|e| AudioError::Output(format!("spawn pipewire thread: {}", e)))?;

        match rx.recv() {
            Ok(Ok(())) => {
                let node = device_uid.strip_prefix("pw:").unwrap_or("").to_string();
                let monitor = status::spawn(shared.status.clone(), node, config.sample_rate);
                if let Ok(mut inner) = shared.inner.lock() {
                    inner.writer = Some(writer);
                    inner.monitor = monitor;
                }
                Ok(PipewireBackend { shared })
            }
            Ok(Err(e)) => {
                let _ = writer.join();
                Err(e)
            }
            Err(_) => {
                let _ = writer.join();
                Err(AudioError::Output(
                    "pipewire setup thread exited unexpectedly".to_string(),
                ))
            }
        }
    }
}

impl Drop for PipewireBackend {
    fn drop(&mut self) {
        self.shared.running.store(false, Ordering::SeqCst);
        self.shared.status.running.store(false, Ordering::SeqCst);
        self.shared.want_play.store(false, Ordering::SeqCst);

        let (writer, monitor) = match self.shared.inner.lock() {
            Ok(mut inner) => (inner.writer.take(), inner.monitor.take()),
            Err(_) => (None, None),
        };
        if let Some(h) = writer {
            let _ = h.join();
        }
        if let Some(h) = monitor {
            let _ = h.join();
        }
    }
}

impl Backend for PipewireBackend {
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

    fn is_alive(&self) -> bool {
        self.shared.alive.load(Ordering::SeqCst)
    }

    fn take_event(&self) -> Option<ExclusiveEvent> {
        self.shared.events.lock().ok()?.pop_front()
    }

    fn original_rate(&self) -> f64 {
        // Playing through PipeWire does not persistently change device configuration.
        0.0
    }

    fn suppress_cleanup(&self) {}
    fn allow_cleanup(&self) {}

    fn device_snapshot(&self) -> DeviceSnapshot {
        DeviceSnapshot {
            hw_volume: self.shared.status.hw_volume.load(Ordering::Relaxed),
            hw_muted: self.shared.status.hw_muted.load(Ordering::Relaxed),
            device_sample_rate: self
                .shared
                .status
                .device_sample_rate
                .load(Ordering::Relaxed),
            app_volume: self.shared.ctx.volume.load(Ordering::Relaxed),
        }
    }
}
