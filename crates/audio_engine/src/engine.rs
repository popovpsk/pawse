use audio_common::{AudioError, AudioSource, StreamParams};
use audio_decoder::Decoder;
use audio_output::{default_device, AudioPlayback};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Mutex;
use std::thread::JoinHandle;
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaybackState {
    Stopped,
    Playing,
    Paused,
}

#[derive(Debug, Clone)]
pub enum EngineEvent {
    TrackEnded,
    PositionChanged(Duration),
}

pub struct TrackInfo {
    pub params: StreamParams,
    pub duration: Duration,
}

pub struct AudioEngine {
    decoder: Option<Box<dyn AudioSource>>,
    output: Option<Box<dyn AudioPlayback>>,
    state: PlaybackState,
    duration: Duration,
    position_samples: u64,
    running: std::sync::Arc<AtomicBool>,
    worker_handle: Option<JoinHandle<()>>,
    #[allow(dead_code)]
    event_sender: Sender<EngineEvent>,
    event_receiver: std::sync::Arc<Mutex<Receiver<EngineEvent>>>,
    last_second: u64,
    sample_rate: u32,
    channels: u8,
}

impl AudioEngine {
    pub fn new() -> Self {
        let (event_sender, event_receiver) = mpsc::channel();

        Self {
            decoder: None,
            output: None,
            state: PlaybackState::Stopped,
            duration: Duration::ZERO,
            position_samples: 0,
            running: std::sync::Arc::new(AtomicBool::new(false)),
            worker_handle: None,
            event_sender,
            event_receiver: std::sync::Arc::new(Mutex::new(event_receiver)),
            last_second: 0,
            sample_rate: 44100,
            channels: 2,
        }
    }

    pub fn open(&mut self, path: &Path) -> Result<TrackInfo, AudioError> {
        self.stop_worker();

        let path_owned = path.to_path_buf();

        let (decoder_result, params, duration) = std::thread::spawn(move || {
            let decoder = Decoder::open(&path_owned);
            decoder.map(|d| {
                let params = d.params();
                let duration = d.duration().unwrap_or(Duration::ZERO);
                (d, params, duration)
            })
        })
        .join()
        .map_err(|_| AudioError::Decoder("Thread panicked".to_string()))??;

        if self.output.is_none() {
            let output = default_device()
                .ok()
                .and_then(|device| device.open_playback(params).ok());
            self.output = output;
        }

        self.decoder = Some(Box::new(decoder_result));
        self.duration = duration;
        self.sample_rate = params.sample_rate;
        self.channels = params.channels_count();
        self.state = PlaybackState::Stopped;
        self.position_samples = 0;
        self.last_second = 0;

        Ok(TrackInfo { params, duration })
    }

    pub fn play(&mut self) -> Result<(), AudioError> {
        if self.decoder.is_none() {
            return Err(AudioError::InvalidState("No track loaded".to_string()));
        }

        if let Some(output) = self.output.as_ref() {
            let _ = output.resume();
        }

        self.state = PlaybackState::Playing;

        let mut decoder = self.decoder.take().expect("Decoder must exist");
        let output = self.output.take();
        let _sample_rate = self.sample_rate;
        let channels = self.channels;

        let mut _position_samples: u64 = 0u64;

        loop {
            match decoder.next_buffer() {
                Ok(Some(buffer)) => {
                    if let Some(ref out) = output {
                        if let Err(e) = out.write(&buffer) {
                            eprintln!("Error writing to output: {}", e);
                            break;
                        }
                    }

                    let samples_written = (buffer.len() / channels as usize) as u64;
                    _position_samples += samples_written;
                }
                Ok(None) => {
                    break;
                }
                Err(e) => {
                    eprintln!("Error decoding: {}", e);
                    break;
                }
            }
        }

        self.decoder = Some(decoder);
        self.output = output;
        self.state = PlaybackState::Stopped;

        Ok(())
    }

    pub fn pause(&mut self) -> Result<(), AudioError> {
        if self.state != PlaybackState::Playing {
            return Ok(());
        }

        if let Some(output) = self.output.as_ref() {
            output.pause()?;
        }

        self.state = PlaybackState::Paused;
        Ok(())
    }

    pub fn stop(&mut self) -> Result<(), AudioError> {
        self.seek(0.0)?;
        self.pause()?;
        self.state = PlaybackState::Stopped;
        Ok(())
    }

    pub fn seek(&mut self, position: f32) -> Result<(), AudioError> {
        let decoder = self
            .decoder
            .as_mut()
            .ok_or_else(|| AudioError::InvalidState("No track loaded".to_string()))?;

        if !(0.0..=1.0).contains(&position) {
            return Err(AudioError::InvalidState(
                "Position must be between 0.0 and 1.0".to_string(),
            ));
        }

        let target = self.duration.mul_f32(position);
        decoder.seek(target)?;

        self.position_samples = (target.as_secs_f64() * self.sample_rate as f64) as u64;
        self.last_second = self.position_samples / self.sample_rate as u64;

        Ok(())
    }

    pub fn position(&self) -> Duration {
        let secs = self.position_samples as f64 / self.sample_rate as f64;
        Duration::from_secs_f64(secs)
    }

    pub fn duration(&self) -> Duration {
        self.duration
    }

    pub fn state(&self) -> PlaybackState {
        self.state
    }

    pub fn events(&self) -> std::sync::Arc<Mutex<Receiver<EngineEvent>>> {
        std::sync::Arc::clone(&self.event_receiver)
    }

    #[allow(dead_code)]
    fn spawn_worker(&mut self) {
        // Not used - kept for reference
        // On macOS cpal callback only works in the same thread that created the stream
    }

    fn stop_worker(&mut self) {
        self.running.store(false, Ordering::SeqCst);

        if let Some(handle) = self.worker_handle.take() {
            let _ = handle.join();
        }

        if let (Some(decoder), Some(output)) = (self.decoder.take(), self.output.take()) {
            let _ = output.pause();
            self.decoder = Some(decoder);
            self.output = Some(output);
        }
    }
}

impl Default for AudioEngine {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn fixture_path(filename: &str) -> PathBuf {
        let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        path.push("..");
        path.push("..");
        path.push("fixtures");
        path.push(filename);
        path
    }

    #[test]
    fn test_new_engine_is_stopped() {
        let engine = AudioEngine::new();
        assert_eq!(engine.state(), PlaybackState::Stopped);
    }

    #[test]
    #[ignore = "Requires audio device"]
    fn test_open_track() {
        let mut engine = AudioEngine::new();
        let path = fixture_path("sine_440_16_44_mono.wav");

        let result = engine.open(&path);
        assert!(result.is_ok(), "Should open track: {:?}", result.err());

        let info = result.unwrap();
        assert!(info.duration > Duration::ZERO);
    }

    #[test]
    fn test_position_before_open() {
        let engine = AudioEngine::new();
        assert_eq!(engine.position(), Duration::ZERO);
    }

    #[test]
    fn test_duration_before_open() {
        let engine = AudioEngine::new();
        assert_eq!(engine.duration(), Duration::ZERO);
    }

    #[test]
    fn test_seek_invalid_state() {
        let mut engine = AudioEngine::new();

        let result = engine.seek(0.5);
        assert!(result.is_err());
    }

    #[test]
    fn test_play_invalid_state() {
        let mut engine = AudioEngine::new();

        let result = engine.play();
        assert!(result.is_err());
    }

    #[test]
    fn test_pause_without_playing() {
        let mut engine = AudioEngine::new();

        let result = engine.pause();
        assert!(result.is_ok());
    }

    #[test]
    #[ignore = "Requires audio device"]
    fn test_open_and_seek() {
        let mut engine = AudioEngine::new();
        let path = fixture_path("sine_440_16_44_mono.wav");

        engine.open(&path).expect("Should open track");
        engine.seek(0.5).expect("Should seek to 50%");

        let pos = engine.position();
        let dur = engine.duration();

        assert!(pos > Duration::ZERO);
        assert!(pos < dur || pos == dur);
    }

    #[test]
    #[ignore = "Requires audio device"]
    fn test_stop() {
        let mut engine = AudioEngine::new();
        let path = fixture_path("sine_440_16_44_mono.wav");

        engine.open(&path).expect("Should open track");
        engine.seek(0.5).expect("Should seek");
        engine.stop().expect("Should stop");

        assert_eq!(engine.position(), Duration::ZERO);
        assert_eq!(engine.state(), PlaybackState::Paused);
    }
}
