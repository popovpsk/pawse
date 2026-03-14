use crate::output::Output;
use crate::types::{Command, EngineEvent, TrackInfo};
use audio_common::{AudioError, AudioSource};
use audio_decoder::Decoder;
use cpal::default_host;
use cpal::traits::HostTrait;
use std::path::Path;
use std::sync::atomic::{AtomicU64, AtomicU8, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

const STATE_STOPPED: u8 = 0;
const STATE_PLAYING: u8 = 1;
const STATE_PAUSED: u8 = 2;

pub struct AudioEngine {
    command_sender: mpsc::Sender<Command>,
    #[allow(dead_code)]
    state: Arc<AtomicU8>,
    position: Arc<AtomicU64>,
    event_receiver: mpsc::Receiver<EngineEvent>,
    track_info: Arc<Mutex<Option<TrackInfo>>>,
    sample_rate: Arc<AtomicU64>,
    #[allow(dead_code)]
    channels: Arc<AtomicU64>,
}

impl AudioEngine {
    pub fn new() -> Result<Self, AudioError> {
        let (event_tx, event_rx) = mpsc::channel();

        let state = Arc::new(AtomicU8::new(STATE_STOPPED));
        let position = Arc::new(AtomicU64::new(0));
        let sample_rate = Arc::new(AtomicU64::new(44100));
        let channels = Arc::new(AtomicU64::new(2));
        let track_info = Arc::new(Mutex::new(None));

        let state_clone = Arc::clone(&state);
        let position_clone = Arc::clone(&position);
        let track_info_clone = Arc::clone(&track_info);
        let sample_rate_clone = Arc::clone(&sample_rate);
        let channels_clone = Arc::clone(&channels);

        let (cmd_tx, cmd_rx) = mpsc::channel();
        let cmd_tx_clone = cmd_tx.clone();

        let ctx = EngineContext {
            state: state_clone,
            position: position_clone,
            track_info: track_info_clone,
            sample_rate: sample_rate_clone,
            channels: channels_clone,
        };

        thread::spawn(move || {
            run_engine_loop(cmd_rx, event_tx, ctx);
        });

        Ok(Self {
            command_sender: cmd_tx_clone,
            state,
            position,
            event_receiver: event_rx,
            track_info,
            sample_rate,
            channels,
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

impl Default for AudioEngine {
    fn default() -> Self {
        Self::new().expect("Failed to create AudioEngine")
    }
}

struct EngineContext {
    state: Arc<AtomicU8>,
    position: Arc<AtomicU64>,
    track_info: Arc<Mutex<Option<TrackInfo>>>,
    sample_rate: Arc<AtomicU64>,
    channels: Arc<AtomicU64>,
}

fn run_engine_loop(
    command_rx: Receiver<Command>,
    event_tx: Sender<EngineEvent>,
    ctx: EngineContext,
) {
    let host = default_host();
    let device = match host.default_output_device() {
        Some(d) => d,
        None => {
            let _ = event_tx.send(EngineEvent::Error("No audio device".to_string()));
            return;
        }
    };

    let output = match Output::new(&device) {
        Ok(o) => o,
        Err(e) => {
            let _ = event_tx.send(EngineEvent::Error(e.to_string()));
            return;
        }
    };

    let mut decoder: Option<Box<dyn AudioSource>> = None;
    let mut duration = Duration::ZERO;

    let mut current_position: u64 = 0;
    let mut is_playing = false;
    let mut has_started = false;
    let mut last_position_update = Instant::now();

    loop {
        while let Ok(cmd) = command_rx.try_recv() {
            match cmd {
                Command::Load(path) => {
                    if decoder.is_some() {
                        decoder = None;
                    }
                    has_started = false;

                    match Decoder::open(&path) {
                        Ok(dec) => {
                            let params = dec.params();
                            duration = dec.duration().unwrap_or(Duration::ZERO);

                            ctx.sample_rate
                                .store(params.sample_rate as u64, Ordering::SeqCst);
                            ctx.channels
                                .store(params.channels_count() as u64, Ordering::SeqCst);

                            if let Ok(mut guard) = ctx.track_info.lock() {
                                *guard = Some(TrackInfo { params, duration });
                            }

                            let _ = event_tx.send(EngineEvent::Loaded { params, duration });

                            decoder = Some(Box::new(dec));
                            current_position = 0;
                            ctx.position.store(0, Ordering::SeqCst);
                            ctx.state.store(STATE_STOPPED, Ordering::SeqCst);

                            output.clear();
                            output.resume();
                        }
                        Err(e) => {
                            let _ = event_tx.send(EngineEvent::Error(e.to_string()));
                        }
                    }
                }
                Command::Play => {
                    if decoder.is_some() {
                        // Pre-buffer only on first play, not on resume from pause
                        if !has_started {
                            for _ in 0..5 {
                                if let Some(ref mut dec) = decoder {
                                    match dec.next_buffer() {
                                        Ok(Some(buffer)) => {
                                            let samples = buffer.as_slice();
                                            let channels =
                                                ctx.channels.load(Ordering::SeqCst) as usize;
                                            let samples_to_write: Vec<f32> = if channels == 1 {
                                                let mut stereo =
                                                    Vec::with_capacity(samples.len() * 2);
                                                for &s in samples {
                                                    stereo.push(s);
                                                    stereo.push(s);
                                                }
                                                stereo
                                            } else {
                                                samples.to_vec()
                                            };
                                            output.write(&samples_to_write);
                                            current_position +=
                                                samples.len() as u64 / channels as u64;
                                        }
                                        Ok(None) => break,
                                        Err(_) => break,
                                    }
                                }
                            }
                            ctx.position.store(current_position, Ordering::SeqCst);
                            has_started = true;
                        }

                        is_playing = true;
                        ctx.state.store(STATE_PLAYING, Ordering::SeqCst);
                        output.resume();
                        let _ = event_tx.send(EngineEvent::Playing);
                    }
                }
                Command::Pause => {
                    is_playing = false;
                    ctx.state.store(STATE_PAUSED, Ordering::SeqCst);
                    output.pause();
                    let _ = event_tx.send(EngineEvent::Paused);
                }
                Command::Stop => {
                    if let Some(ref mut dec) = decoder {
                        let _ = dec.seek(Duration::ZERO);
                    }
                    output.clear();
                    current_position = 0;
                    ctx.position.store(0, Ordering::SeqCst);
                    is_playing = false;
                    has_started = false;
                    ctx.state.store(STATE_STOPPED, Ordering::SeqCst);
                    output.pause();
                    let _ = event_tx.send(EngineEvent::Stopped);
                }
                Command::Seek(pos) => {
                    if let Some(ref mut dec) = decoder {
                        let target = duration.mul_f32(pos);
                        if dec.seek(target).is_ok() {
                            output.clear();
                            current_position = (target.as_secs_f64()
                                * ctx.sample_rate.load(Ordering::SeqCst) as f64)
                                as u64;
                            ctx.position.store(current_position, Ordering::SeqCst);
                        }
                    }
                }
            }
        }

        if is_playing {
            if let Some(ref mut dec) = decoder {
                // Check buffer level - only decode if buffer is not too full
                let buffer_arc = output.buffer();
                let buffer_level = {
                    let buf = buffer_arc.lock().unwrap();
                    buf.len()
                };

                // Only decode if buffer has room (backpressure)
                const MAX_BUFFER_SAMPLES: usize = 44100 * 2; // ~0.5 sec stereo
                if buffer_level < MAX_BUFFER_SAMPLES {
                    match dec.next_buffer() {
                        Ok(Some(buffer)) => {
                            let samples = buffer.as_slice();
                            let channels = ctx.channels.load(Ordering::SeqCst) as usize;

                            // Convert to stereo if mono
                            let samples_to_write: Vec<f32> = if channels == 1 {
                                let mut stereo = Vec::with_capacity(samples.len() * 2);
                                for &s in samples {
                                    stereo.push(s);
                                    stereo.push(s);
                                }
                                stereo
                            } else {
                                samples.to_vec()
                            };

                            output.write(&samples_to_write);
                            let written = samples.len();
                            if written > 0 {
                                current_position += written as u64 / channels as u64;
                                ctx.position.store(current_position, Ordering::SeqCst);
                            }
                        }
                        Ok(None) => {
                            is_playing = false;
                            ctx.state.store(STATE_STOPPED, Ordering::SeqCst);
                            let _ = event_tx.send(EngineEvent::TrackEnded);
                            output.pause();
                        }
                        Err(e) => {
                            let _ = event_tx.send(EngineEvent::Error(e.to_string()));
                            is_playing = false;
                        }
                    }
                }
            }
        }

        if last_position_update.elapsed() >= Duration::from_secs(1) {
            let pos = Duration::from_secs_f64(
                current_position as f64 / ctx.sample_rate.load(Ordering::SeqCst) as f64,
            );
            let _ = event_tx.send(EngineEvent::PositionChanged(pos));
            last_position_update = Instant::now();
        }

        // Sleep longer when not playing to reduce CPU usage
        let sleep_time = if is_playing { 5 } else { 50 };
        thread::sleep(Duration::from_millis(sleep_time));
    }
}
