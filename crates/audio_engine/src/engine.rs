use audio_output::AudioOutput;

use crate::types::{Command, EngineEvent, TrackInfo};
use audio_common::{AudioError, AudioSource};
use audio_decoder::Decoder;
use std::path::Path;
use std::sync::atomic::{AtomicU64, AtomicU8, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, PartialEq, Eq)]
enum State {
    Stopped,
    Playing,
    Paused,
}

impl State {
    fn as_u8(self) -> u8 {
        match self {
            State::Stopped => 0,
            State::Playing => 1,
            State::Paused => 2,
        }
    }
}

pub struct AudioEngine {
    command_sender: mpsc::Sender<Command>,
    position: Arc<AtomicU64>,
    event_receiver: mpsc::Receiver<EngineEvent>,
    track_info: Arc<Mutex<Option<TrackInfo>>>,
    sample_rate: Arc<AtomicU64>,
}

impl AudioEngine {
    pub fn new(output: Arc<dyn AudioOutput>) -> Result<Self, AudioError> {
        let (event_tx, event_rx) = mpsc::channel();

        let state = Arc::new(AtomicU8::new(State::Stopped.as_u8()));
        let position = Arc::new(AtomicU64::new(0));
        let sample_rate = Arc::new(AtomicU64::new(44100));
        let channels = Arc::new(AtomicU64::new(2));
        let track_info = Arc::new(Mutex::new(None));

        let (cmd_tx, cmd_rx) = mpsc::channel();

        let ctx = EngineContext {
            state: Arc::clone(&state),
            position: Arc::clone(&position),
            track_info: Arc::clone(&track_info),
            sample_rate: Arc::clone(&sample_rate),
            channels: Arc::clone(&channels),
        };

        let output_clone = Arc::clone(&output);
        thread::spawn(move || {
            run_engine_loop(cmd_rx, event_tx, ctx, output_clone);
        });

        Ok(Self {
            command_sender: cmd_tx,
            position,
            event_receiver: event_rx,
            track_info,
            sample_rate,
        })
    }

    pub fn load(&self, path: &Path) -> Result<(), AudioError> {
        self.command_sender
            .send(Command::Load(path.to_path_buf()))
            .map_err(|_| AudioError::StreamClosed)
    }

    pub fn track_info(&self) -> Option<TrackInfo> {
        self.track_info.lock().ok().and_then(|guard| guard.clone())
    }

    pub fn play(&self) -> Result<(), AudioError> {
        self.command_sender
            .send(Command::Play)
            .map_err(|_| AudioError::StreamClosed)
    }

    pub fn pause(&self) -> Result<(), AudioError> {
        self.command_sender
            .send(Command::Pause)
            .map_err(|_| AudioError::StreamClosed)
    }

    pub fn stop(&self) -> Result<(), AudioError> {
        self.command_sender
            .send(Command::Stop)
            .map_err(|_| AudioError::StreamClosed)
    }

    pub fn seek(&self, position: f32) -> Result<(), AudioError> {
        self.command_sender
            .send(Command::Seek(position))
            .map_err(|_| AudioError::StreamClosed)
    }

    pub fn position_samples(&self) -> u64 {
        self.position.load(Ordering::SeqCst)
    }

    pub fn position(&self) -> Duration {
        let samples = self.position.load(Ordering::SeqCst);
        let rate = self.sample_rate.load(Ordering::SeqCst) as f64;
        Duration::from_secs_f64(samples as f64 / rate)
    }

    pub fn events(&self) -> &mpsc::Receiver<EngineEvent> {
        &self.event_receiver
    }
}

struct EngineContext {
    state: Arc<AtomicU8>,
    position: Arc<AtomicU64>,
    track_info: Arc<Mutex<Option<TrackInfo>>>,
    sample_rate: Arc<AtomicU64>,
    channels: Arc<AtomicU64>,
}

struct PlaybackState<'a> {
    decoder: &'a mut Option<Box<dyn AudioSource>>,
    duration: &'a mut Duration,
    current_position: &'a mut u64,
    has_started: &'a mut bool,
    is_playing: &'a mut bool,
}

fn handle_load(
    path: &Path,
    state: &mut PlaybackState,
    ctx: &EngineContext,
    output: &Arc<dyn AudioOutput>,
    event_tx: &Sender<EngineEvent>,
) {
    if state.decoder.is_some() {
        *state.decoder = None;
    }
    *state.has_started = false;

    match Decoder::open(path) {
        Ok(dec) => {
            let params = dec.params();
            *state.duration = dec.duration().unwrap_or(Duration::ZERO);

            ctx.sample_rate
                .store(params.sample_rate as u64, Ordering::SeqCst);
            ctx.channels
                .store(params.channels_count() as u64, Ordering::SeqCst);

            if let Ok(mut guard) = ctx.track_info.lock() {
                *guard = Some(TrackInfo {
                    params,
                    duration: *state.duration,
                });
            }

            let _ = event_tx.send(EngineEvent::Loaded {
                params,
                duration: *state.duration,
            });

            *state.decoder = Some(Box::new(dec));
            *state.current_position = 0;
            ctx.position.store(0, Ordering::SeqCst);
            ctx.state.store(State::Stopped.as_u8(), Ordering::SeqCst);

            output.clear();
            output.resume();
        }
        Err(e) => {
            let _ = event_tx.send(EngineEvent::Error(e.to_string()));
        }
    }
}

/// Предварительная буферизация перед запуском воспроизведения
fn pre_buffer(
    decoder: &mut Option<Box<dyn AudioSource>>,
    ctx: &EngineContext,
    output: &Arc<dyn AudioOutput>,
    current_position: &mut u64,
) {
    let Some(ref mut dec) = decoder else {
        return;
    };

    for _ in 0..5 {
        match dec.next_buffer() {
            Ok(Some(batch)) => {
                let channels = ctx.channels.load(Ordering::SeqCst) as usize;
                let samples_len = batch.data.len();
                output.write(batch);
                *current_position += samples_len as u64 / channels as u64;
            }
            Ok(None) => break,
            Err(_) => break,
        }
    }
    ctx.position.store(*current_position, Ordering::SeqCst);
}

fn handle_play(
    state: &mut PlaybackState,
    ctx: &EngineContext,
    output: &Arc<dyn AudioOutput>,
    event_tx: &Sender<EngineEvent>,
) {
    if state.decoder.is_some() {
        if !*state.has_started {
            pre_buffer(state.decoder, ctx, output, state.current_position);
            *state.has_started = true;
        }

        *state.is_playing = true;
        ctx.state.store(State::Playing.as_u8(), Ordering::SeqCst);
        output.resume();
        let _ = event_tx.send(EngineEvent::Playing);
    }
}

fn handle_pause(
    is_playing: &mut bool,
    ctx: &EngineContext,
    output: &Arc<dyn AudioOutput>,
    event_tx: &Sender<EngineEvent>,
) {
    *is_playing = false;
    ctx.state.store(State::Paused.as_u8(), Ordering::SeqCst);
    output.pause();
    let _ = event_tx.send(EngineEvent::Paused);
}

fn handle_stop(
    state: &mut PlaybackState,
    ctx: &EngineContext,
    output: &Arc<dyn AudioOutput>,
    event_tx: &Sender<EngineEvent>,
) {
    if let Some(ref mut dec) = state.decoder {
        let _ = dec.seek(Duration::ZERO);
    }
    output.clear();
    *state.current_position = 0;
    ctx.position.store(0, Ordering::SeqCst);
    *state.is_playing = false;
    *state.has_started = false;
    ctx.state.store(State::Stopped.as_u8(), Ordering::SeqCst);
    output.pause();
    let _ = event_tx.send(EngineEvent::Stopped);
}

fn handle_seek(
    pos: f32,
    decoder: &mut Option<Box<dyn AudioSource>>,
    duration: Duration,
    current_position: &mut u64,
    ctx: &EngineContext,
    output: &Arc<dyn AudioOutput>,
) {
    if let Some(ref mut dec) = decoder {
        let target = duration.mul_f32(pos);
        if dec.seek(target).is_ok() {
            output.clear();
            *current_position =
                (target.as_secs_f64() * ctx.sample_rate.load(Ordering::SeqCst) as f64) as u64;
            ctx.position.store(*current_position, Ordering::SeqCst);
        }
    }
}

/// Обработка воспроизведения: декодирование и запись в output
fn process_playback(
    decoder: &mut Option<Box<dyn AudioSource>>,
    is_playing: &mut bool,
    current_position: &mut u64,
    ctx: &EngineContext,
    output: &Arc<dyn AudioOutput>,
    event_tx: &Sender<EngineEvent>,
) {
    let Some(ref mut dec) = decoder else {
        return;
    };

    match dec.next_buffer() {
        Ok(Some(batch)) => {
            let channels = ctx.channels.load(Ordering::SeqCst) as usize;
            let written = batch.data.len();
            output.write(batch);
            if written > 0 {
                *current_position += written as u64 / channels as u64;
                ctx.position.store(*current_position, Ordering::SeqCst);
            }
        }
        Ok(None) => {
            *is_playing = false;
            ctx.state.store(State::Stopped.as_u8(), Ordering::SeqCst);
            let _ = event_tx.send(EngineEvent::TrackEnded);
            output.pause();
        }
        Err(e) => {
            let _ = event_tx.send(EngineEvent::Error(e.to_string()));
            *is_playing = false;
        }
    }
}

/// Главный цикл движка: обработка команд и воспроизведение
fn run_engine_loop(
    command_rx: Receiver<Command>,
    event_tx: Sender<EngineEvent>,
    ctx: EngineContext,
    output: Arc<dyn AudioOutput>,
) {
    let mut decoder: Option<Box<dyn AudioSource>> = None;
    let mut duration = Duration::ZERO;
    let mut current_position: u64 = 0;
    let mut is_playing = false;
    let mut has_started = false;
    let mut last_position_update = Instant::now();

    macro_rules! make_state {
        () => {
            PlaybackState {
                decoder: &mut decoder,
                duration: &mut duration,
                current_position: &mut current_position,
                has_started: &mut has_started,
                is_playing: &mut is_playing,
            }
        };
    }

    loop {
        while let Ok(cmd) = command_rx.try_recv() {
            match cmd {
                Command::Load(path) => {
                    handle_load(&path, &mut make_state!(), &ctx, &output, &event_tx);
                }
                Command::Play => {
                    handle_play(&mut make_state!(), &ctx, &output, &event_tx);
                }
                Command::Pause => handle_pause(&mut is_playing, &ctx, &output, &event_tx),
                Command::Stop => {
                    handle_stop(&mut make_state!(), &ctx, &output, &event_tx);
                }
                Command::Seek(pos) => handle_seek(
                    pos,
                    &mut decoder,
                    duration,
                    &mut current_position,
                    &ctx,
                    &output,
                ),
            }
        }

        if is_playing {
            process_playback(
                &mut decoder,
                &mut is_playing,
                &mut current_position,
                &ctx,
                &output,
                &event_tx,
            );
        }

        if last_position_update.elapsed() >= Duration::from_secs(1) {
            let pos = Duration::from_secs_f64(
                current_position as f64 / ctx.sample_rate.load(Ordering::SeqCst) as f64,
            );
            let _ = event_tx.send(EngineEvent::PositionChanged(pos));
            last_position_update = Instant::now();
        }

        let sleep_time = if is_playing { 5 } else { 50 };
        thread::sleep(Duration::from_millis(sleep_time));
    }
}
