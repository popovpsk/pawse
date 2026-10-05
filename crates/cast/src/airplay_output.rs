use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use atomic_float::AtomicF32;
use audio_common::{AudioBatch, AudioSamples};
use audio_output::{
    AudioOutput, EngineOutput, FadeEvent, FadeState, apply_fade_gain, calculate_volume_scaled,
};
use rubato::{FftFixedIn, Resampler};

use airplay::{CHANNELS, LATENCY_FRAMES, NowPlaying, RemoteCommand, SAMPLE_RATE};

use crate::media::{Cover, TrackInfo};

const QUEUE_SECONDS: f32 = 0.5;
const WRITE_WAIT: Duration = Duration::from_millis(200);
const RESAMPLE_CHUNK: usize = 1024;

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

fn queue_capacity() -> usize {
    (SAMPLE_RATE as f32 * QUEUE_SECONDS) as usize * CHANNELS
}

fn history_capacity() -> usize {
    (LATENCY_FRAMES as usize + 4 * airplay::FRAMES_PER_PACKET) * CHANNELS
}

struct Buffer {
    queue: Mutex<VecDeque<f32>>,
    history: Mutex<VecDeque<f32>>,
    space: Condvar,
    fade: FadeState,
    gain: AtomicF32,
}

struct Renderer {
    buffer: Arc<Buffer>,
    scratch: Vec<f32>,
    silent_tail: usize,
}

impl airplay::Render for Renderer {
    fn render(&mut self, out: &mut [i16]) {
        if self.buffer.fade.is_frozen() {
            out.fill(0);
            self.silent_tail += out.len() / CHANNELS;
            return;
        }
        self.scratch.clear();
        {
            let mut queue = lock(&self.buffer.queue);
            let take = out.len().min(queue.len()) / CHANNELS * CHANNELS;
            self.scratch.extend(queue.drain(..take));
        }
        self.buffer.space.notify_all();
        {
            let mut history = lock(&self.buffer.history);
            history.extend(self.scratch.iter().copied());
            let excess = history.len().saturating_sub(history_capacity());
            history.drain(..excess);
        }
        apply_fade_gain(
            &self.buffer.fade,
            self.buffer.gain.load(Ordering::Relaxed),
            CHANNELS,
            &mut self.scratch,
        );
        for (slot, sample) in out.iter_mut().zip(self.scratch.iter()) {
            *slot = (sample.clamp(-1.0, 1.0) * f32::from(i16::MAX)).round() as i16;
        }
        out[self.scratch.len()..].fill(0);
        let silent = (out.len() - self.scratch.len()) / CHANNELS;
        if self.scratch.is_empty() {
            self.silent_tail += silent;
        } else {
            self.silent_tail = silent;
        }
    }

    fn rewind(&mut self, frames: usize) {
        let audible = frames.saturating_sub(std::mem::take(&mut self.silent_tail));
        let tail: Vec<f32> = {
            let mut history = lock(&self.buffer.history);
            let take = (audible * CHANNELS).min(history.len());
            let start = history.len() - take;
            history.drain(start..).collect()
        };
        let mut queue = lock(&self.buffer.queue);
        for sample in tail.into_iter().rev() {
            queue.push_front(sample);
        }
    }
}

struct Converter {
    rate: u32,
    resampler: Option<FftFixedIn<f32>>,
    input: [Vec<f32>; 2],
    output: Vec<Vec<f32>>,
}

impl Converter {
    fn new() -> Self {
        Self {
            rate: SAMPLE_RATE,
            resampler: None,
            input: [Vec::new(), Vec::new()],
            output: Vec::new(),
        }
    }

    fn reset(&mut self) {
        if let Some(resampler) = &mut self.resampler {
            resampler.reset();
        }
        self.input[0].clear();
        self.input[1].clear();
    }

    fn convert(&mut self, rate: u32, stereo: &[f32], out: &mut Vec<f32>) {
        if rate != self.rate {
            self.rate = rate;
            self.input[0].clear();
            self.input[1].clear();
            self.resampler = if rate == SAMPLE_RATE {
                None
            } else {
                match FftFixedIn::new(
                    rate as usize,
                    SAMPLE_RATE as usize,
                    RESAMPLE_CHUNK,
                    2,
                    CHANNELS,
                ) {
                    Ok(resampler) => {
                        self.output = resampler.output_buffer_allocate(true);
                        Some(resampler)
                    }
                    Err(e) => {
                        log::error!("AirPlay: no resampler from {rate} Hz: {e}");
                        None
                    }
                }
            };
        }
        let Some(resampler) = &mut self.resampler else {
            out.extend_from_slice(stereo);
            return;
        };
        for [left, right] in stereo.as_chunks::<CHANNELS>().0 {
            self.input[0].push(*left);
            self.input[1].push(*right);
        }
        loop {
            let needed = resampler.input_frames_next();
            if self.input[0].len() < needed {
                return;
            }
            let chunk = [&self.input[0][..needed], &self.input[1][..needed]];
            match resampler.process_into_buffer(&chunk, &mut self.output, None) {
                Ok((_, produced)) => {
                    for i in 0..produced {
                        out.push(self.output[0][i]);
                        out.push(self.output[1][i]);
                    }
                }
                Err(e) => log::error!("AirPlay: resampling failed: {e}"),
            }
            self.input[0].drain(..needed);
            self.input[1].drain(..needed);
        }
    }
}

fn to_stereo(batch: &AudioBatch) -> Vec<f32> {
    let channels = usize::from(batch.metadata.channels.to_u8()).max(1);
    let samples = match &batch.data {
        AudioSamples::F32(data) if channels == CHANNELS => return data.clone(),
        other => other.to_f32(),
    };
    match channels {
        1 => samples.iter().flat_map(|s| [*s, *s]).collect(),
        2 => samples,
        _ => samples
            .chunks_exact(channels)
            .flat_map(|frame| {
                let center = frame.get(2).copied().unwrap_or(0.0) * 0.707;
                let left_surround = frame.get(4).copied().unwrap_or(0.0) * 0.707;
                let right_surround = frame.get(5).copied().unwrap_or(0.0) * 0.707;
                [
                    (frame[0] + center + left_surround) * 0.5,
                    (frame[1] + center + right_surround) * 0.5,
                ]
            })
            .collect(),
    }
}

pub struct AirPlayOutput {
    device: airplay::Device,
    stream: Mutex<Option<airplay::Stream>>,
    buffer: Arc<Buffer>,
    converter: Mutex<Converter>,
    intent: AtomicBool,
    closed: AtomicBool,
    volume: Mutex<Option<f32>>,
    now_playing: Mutex<Option<NowPlaying>>,
    progress: Mutex<Option<(i64, u64)>>,
    route: Mutex<Option<crate::dacp::Route>>,
    lost: flume::Sender<String>,
    lost_events: flume::Receiver<String>,
    remote: flume::Sender<RemoteCommand>,
    remote_commands: flume::Receiver<RemoteCommand>,
}

impl AirPlayOutput {
    pub fn connect(
        device: airplay::Device,
        volume: Option<f32>,
        gain: f32,
    ) -> Result<Arc<Self>, String> {
        let (lost, lost_events) = flume::unbounded();
        let (remote, remote_commands) = flume::unbounded();
        let output = Arc::new(Self {
            device,
            stream: Mutex::new(None),
            buffer: Arc::new(Buffer {
                queue: Mutex::new(VecDeque::with_capacity(queue_capacity() * 2)),
                history: Mutex::new(VecDeque::with_capacity(history_capacity())),
                space: Condvar::new(),
                fade: FadeState::new(),
                gain: AtomicF32::new(calculate_volume_scaled(gain)),
            }),
            converter: Mutex::new(Converter::new()),
            intent: AtomicBool::new(false),
            closed: AtomicBool::new(false),
            volume: Mutex::new(volume),
            now_playing: Mutex::new(None),
            progress: Mutex::new(None),
            route: Mutex::new(None),
            lost,
            lost_events,
            remote,
            remote_commands,
        });
        output.ensure_stream()?;
        Ok(output)
    }

    pub fn name(&self) -> &str {
        &self.device.name
    }

    pub fn lost(&self) -> flume::Receiver<String> {
        self.lost_events.clone()
    }

    pub fn commands(&self) -> flume::Receiver<RemoteCommand> {
        self.remote_commands.clone()
    }

    pub fn set_now_playing(&self, info: TrackInfo, cover: Option<Cover>) {
        let now = NowPlaying {
            title: info.title,
            artist: info.artist,
            album: info.album,
            cover: cover.map(|cover| airplay::Cover {
                mime: cover.mime,
                bytes: cover.bytes,
            }),
        };
        *lock(&self.now_playing) = Some(now.clone());
        if let Some(stream) = lock(&self.stream).as_ref() {
            stream.set_now_playing(now);
        }
    }

    pub fn set_progress(&self, heard_ms: i64, duration_ms: u64) {
        *lock(&self.progress) = Some((heard_ms, duration_ms));
        if let Some(stream) = lock(&self.stream).as_ref() {
            stream.set_progress(heard_ms, duration_ms);
        }
    }

    pub fn pending(&self) -> Duration {
        let queued = lock(&self.buffer.queue).len() / CHANNELS;
        let unheard = lock(&self.stream)
            .as_ref()
            .map(|stream| stream.unheard_frames())
            .unwrap_or(0);
        Duration::from_secs_f64((queued as u64 + unheard) as f64 / f64::from(SAMPLE_RATE))
    }

    pub fn set_device_volume(&self, volume: f32) {
        *lock(&self.volume) = Some(volume);
        if let Some(stream) = lock(&self.stream).as_ref() {
            stream.set_volume(volume);
        }
    }

    pub fn leave_device_volume(&self) {
        *lock(&self.volume) = None;
    }

    pub fn set_gain(&self, volume: f32) {
        self.buffer.gain.store(
            calculate_volume_scaled(volume.clamp(0.0, 1.0)),
            Ordering::Relaxed,
        );
    }

    fn ensure_stream(&self) -> Result<(), String> {
        if self.closed.load(Ordering::Acquire) {
            return Err("the AirPlay output is closed".into());
        }
        let stale = {
            let mut slot = lock(&self.stream);
            if slot.as_ref().is_some_and(airplay::Stream::is_alive) {
                return Ok(());
            }
            slot.take()
        };
        drop(stale);
        let renderer = Renderer {
            buffer: self.buffer.clone(),
            scratch: Vec::with_capacity(airplay::FRAMES_PER_PACKET * CHANNELS),
            silent_tail: 0,
        };
        let volume = *lock(&self.volume);
        let stream =
            airplay::Stream::start(&self.device, volume, Box::new(renderer), crate::net::PORTS)
                .map_err(|e| e.to_string())?;
        let events = stream.events();
        let (lost, remote) = (self.lost.clone(), self.remote.clone());
        let _ = std::thread::Builder::new()
            .name("airplay-forward".into())
            .spawn(move || {
                while let Ok(event) = events.recv() {
                    match event {
                        airplay::StreamEvent::Lost(reason) => {
                            let _ = lost.send(reason);
                        }
                        airplay::StreamEvent::Remote(command) => {
                            let _ = remote.send(command);
                        }
                    }
                }
            });
        let replaced = lock(&self.stream).replace(stream);
        drop(replaced);
        if self.closed.load(Ordering::Acquire) {
            let stream = lock(&self.stream).take();
            drop(stream);
            return Err("the AirPlay output is closed".into());
        }
        if let Some(stream) = lock(&self.stream).as_ref() {
            let old = lock(&self.route).take();
            drop(old);
            *lock(&self.route) = crate::dacp::route(stream.active_remote(), self.remote.clone());
            if let Some(now) = lock(&self.now_playing).clone() {
                stream.set_now_playing(now);
            }
            if let Some((heard_ms, duration_ms)) = *lock(&self.progress) {
                stream.set_progress(heard_ms, duration_ms);
            }
        }
        Ok(())
    }

    pub fn close(&self) {
        self.closed.store(true, Ordering::Release);
        self.intent.store(false, Ordering::SeqCst);
        lock(&self.buffer.queue).clear();
        self.buffer.space.notify_all();
        let stream = lock(&self.stream).take();
        drop(stream);
        lock(&self.route).take();
    }
}

impl AudioOutput for AirPlayOutput {
    fn write(&self, batch: &AudioBatch) -> usize {
        if batch.data.is_empty() {
            return 0;
        }
        let channels = usize::from(batch.metadata.channels.to_u8()).max(1);
        let frames = batch.data.len() / channels;
        let rate = batch.metadata.sample_rate.max(1);
        let incoming = (frames as u64 * u64::from(SAMPLE_RATE) / u64::from(rate)) as usize
            * CHANNELS
            + RESAMPLE_CHUNK * CHANNELS * 2;
        {
            let queue = lock(&self.buffer.queue);
            let (queue, _) = self
                .buffer
                .space
                .wait_timeout_while(queue, WRITE_WAIT, |queue| {
                    queue.len() + incoming > queue_capacity().max(incoming)
                })
                .unwrap_or_else(PoisonError::into_inner);
            if queue.len() + incoming > queue_capacity().max(incoming) {
                return 0;
            }
        }
        let stereo = to_stereo(batch);
        let mut converted = Vec::with_capacity(incoming);
        lock(&self.converter).convert(rate, &stereo, &mut converted);
        lock(&self.buffer.queue).extend(converted);
        batch.data.len()
    }

    fn clear(&self) {
        lock(&self.buffer.queue).clear();
        lock(&self.buffer.history).clear();
        lock(&self.converter).reset();
        self.buffer.space.notify_all();
        if let Some(stream) = lock(&self.stream).as_ref() {
            stream.flush();
        }
    }

    fn pause(&self) {
        self.intent.store(false, Ordering::SeqCst);
        let flush = self.buffer.fade.is_frozen();
        if let Some(stream) = lock(&self.stream).as_ref() {
            stream.pause(flush);
        }
    }

    fn resume(&self) {
        self.intent.store(true, Ordering::SeqCst);
        if let Err(e) = self.ensure_stream() {
            log::warn!("AirPlay: reconnecting to {} failed: {e}", self.device.name);
            let _ = self.lost.send(e);
            return;
        }
        if let Some(stream) = lock(&self.stream).as_ref() {
            stream.play();
        }
    }

    fn is_playing(&self) -> bool {
        self.intent.load(Ordering::SeqCst)
    }

    fn set_volume(&self, volume: f32) {
        self.set_gain(volume);
    }
}

impl EngineOutput for AirPlayOutput {
    fn begin_fade(&self, start: Option<f32>, target: f32, duration_ms: u32) {
        self.buffer
            .fade
            .begin(SAMPLE_RATE, start, target, duration_ms);
    }

    fn take_fade_event(&self) -> Option<FadeEvent> {
        self.buffer.fade.take_event()
    }

    fn reset_fade(&self) {
        self.buffer.fade.reset();
    }

    fn release_paused(&self) {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use airplay::Render;
    use audio_common::{ChannelCount, Metadata};

    fn buffer() -> Arc<Buffer> {
        Arc::new(Buffer {
            queue: Mutex::new(VecDeque::new()),
            history: Mutex::new(VecDeque::new()),
            space: Condvar::new(),
            fade: FadeState::new(),
            gain: AtomicF32::new(1.0),
        })
    }

    #[test]
    fn rewound_frames_are_rendered_again_in_order() {
        let buffer = buffer();
        lock(&buffer.queue).extend((0..8).map(|i| i as f32 / 100.0));
        let mut renderer = Renderer {
            buffer: buffer.clone(),
            scratch: Vec::new(),
            silent_tail: 0,
        };
        let mut out = [0i16; 4];
        renderer.render(&mut out);
        renderer.render(&mut out);
        assert!(lock(&buffer.queue).is_empty());
        renderer.rewind(1);
        let mut again = [0i16; 4];
        renderer.render(&mut again);
        assert_eq!(&again[..2], &out[2..4]);
        assert_eq!(&again[2..], &[0, 0]);
    }

    #[test]
    fn a_finished_fade_out_renders_silence_and_keeps_the_queue() {
        let buffer = buffer();
        lock(&buffer.queue).extend([0.5f32; 8]);
        buffer.fade.begin(SAMPLE_RATE, None, 0.0, 0);
        let mut renderer = Renderer {
            buffer: buffer.clone(),
            scratch: Vec::new(),
            silent_tail: 0,
        };
        let mut out = [1i16; 4];
        renderer.render(&mut out);
        assert!(buffer.fade.is_frozen());
        assert_eq!(buffer.fade.take_event(), Some(FadeEvent::FadedOut));
        renderer.render(&mut out);
        assert_eq!(out, [0; 4]);
        assert_eq!(lock(&buffer.queue).len(), 4);
    }

    #[test]
    fn the_gain_scales_what_is_rendered() {
        let buffer = buffer();
        buffer
            .gain
            .store(calculate_volume_scaled(0.5), Ordering::Relaxed);
        lock(&buffer.queue).extend([0.5f32; 4]);
        let mut renderer = Renderer {
            buffer: buffer.clone(),
            scratch: Vec::new(),
            silent_tail: 0,
        };
        let mut out = [0i16; 4];
        renderer.render(&mut out);
        let expected = (0.5 * calculate_volume_scaled(0.5) * f32::from(i16::MAX)).round() as i16;
        assert_eq!(out, [expected; 4]);
        assert!(expected < i16::MAX / 4);
    }

    #[test]
    fn surround_and_mono_become_stereo() {
        let batch = |channels: u8, data: Vec<f32>| AudioBatch {
            data: AudioSamples::F32(data),
            metadata: Metadata {
                sample_rate: 44_100,
                channels: ChannelCount::from_u8(channels),
                bit_depth: 16,
            },
        };
        assert_eq!(
            to_stereo(&batch(1, vec![0.1, 0.2])),
            vec![0.1, 0.1, 0.2, 0.2]
        );
        let surround = to_stereo(&batch(6, vec![1.0, 0.0, 0.0, 0.0, 0.0, 0.0]));
        assert_eq!(surround, vec![0.5, 0.0]);
    }

    #[test]
    fn resampling_keeps_the_duration() {
        let mut converter = Converter::new();
        let mut out = Vec::new();
        let second: Vec<f32> = (0..48_000 * 2)
            .map(|i| ((i / 2) as f32 * 0.01).sin())
            .collect();
        converter.convert(48_000, &second, &mut out);
        converter.convert(48_000, &second, &mut out);
        let frames = out.len() / 2;
        assert!(frames > 44_100 * 2 - 3 * RESAMPLE_CHUNK && frames <= 44_100 * 2);
        let mut passthrough = Vec::new();
        converter.convert(44_100, &[0.25, -0.25], &mut passthrough);
        assert_eq!(passthrough, vec![0.25, -0.25]);
    }
}
