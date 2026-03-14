use audio_common::{AudioError, AudioSource, StreamParams};
use audio_decoder::Decoder;
use audio_output::{list_output_devices, AudioSink, CpalOutput, DeviceInfo};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaybackState {
    Stopped,
    Playing,
    Paused,
}

pub struct Player {
    state: PlaybackState,
    #[allow(dead_code)]
    current_file: Option<PathBuf>,
    position_samples: u64,
    volume: f32,
    bit_perfect: bool,
}

impl Player {
    pub fn new() -> Self {
        Self {
            state: PlaybackState::Stopped,
            current_file: None,
            position_samples: 0,
            volume: 1.0,
            bit_perfect: false,
        }
    }

    pub fn state(&self) -> PlaybackState {
        self.state
    }

    pub fn set_volume(&mut self, volume: f32) {
        self.volume = volume.clamp(0.0, 1.0);
    }

    pub fn volume(&self) -> f32 {
        self.volume
    }

    pub fn set_bit_perfect(&mut self, bit_perfect: bool) {
        self.bit_perfect = bit_perfect;
    }

    pub fn is_bit_perfect(&self) -> bool {
        self.bit_perfect
    }

    pub fn position_samples(&self) -> u64 {
        self.position_samples
    }
}

impl Default for Player {
    fn default() -> Self {
        Self::new()
    }
}

pub struct AudioEngine {
    decoder: Option<Box<dyn AudioSource>>,
    output: Option<CpalOutput>,
    player: Arc<Mutex<Player>>,
    running: Arc<AtomicBool>,
    worker_handle: Option<thread::JoinHandle<()>>,
}

impl AudioEngine {
    pub fn new() -> Self {
        Self {
            decoder: None,
            output: None,
            player: Arc::new(Mutex::new(Player::new())),
            running: Arc::new(AtomicBool::new(false)),
            worker_handle: None,
        }
    }

    pub fn list_devices() -> Vec<DeviceInfo> {
        list_output_devices()
    }

    pub fn open_file<P: AsRef<std::path::Path>>(
        &mut self,
        path: P,
    ) -> Result<StreamParams, AudioError> {
        let decoder = Box::new(Decoder::open(path)?);
        let params = decoder.params();

        self.decoder = Some(decoder);
        Ok(params)
    }

    pub fn open_output(
        &mut self,
        params: StreamParams,
        device: Option<&str>,
    ) -> Result<(), AudioError> {
        let output = CpalOutput::new(params, device)?;
        self.output = Some(output);
        Ok(())
    }

    pub fn play(&mut self) -> Result<(), AudioError> {
        if self.decoder.is_none() {
            return Err(AudioError::InvalidState("No file loaded".to_string()));
        }

        if let Some(ref mut output) = self.output {
            output.start()?;
        }

        self.running.store(true, Ordering::SeqCst);

        let player = Arc::clone(&self.player);
        let running = Arc::clone(&self.running);
        let decoder = self.decoder.take().expect("decoder should exist");
        let output = std::mem::replace(&mut self.output, None);

        let (volume, bypass) = {
            let p = player.lock().unwrap();
            (p.volume(), p.is_bit_perfect())
        };

        self.worker_handle = Some(thread::spawn(move || {
            let output = output;
            let mut decoder = decoder;

            while running.load(Ordering::SeqCst) {
                match decoder.next_buffer() {
                    Ok(Some(mut buffer)) => {
                        if !bypass && (volume - 1.0).abs() > f32::EPSILON {
                            let samples = buffer.as_slice_mut();
                            for sample in samples.iter_mut() {
                                *sample *= volume;
                            }
                        }

                        if let Some(ref out) = output {
                            if let Err(e) = out.write(&buffer) {
                                eprintln!("Output error: {}", e);
                            }
                        }

                        let samples = buffer.len() as u64 / 2;
                        if let Ok(mut p) = player.lock() {
                            p.position_samples += samples;
                        }
                    }
                    Ok(None) => {
                        break;
                    }
                    Err(e) => {
                        eprintln!("Decode error: {}", e);
                        break;
                    }
                }
            }
        }));

        if let Ok(mut p) = self.player.lock() {
            p.state = PlaybackState::Playing;
        }

        Ok(())
    }

    pub fn pause(&mut self) -> Result<(), AudioError> {
        self.running.store(false, Ordering::SeqCst);

        if let Some(ref mut output) = self.output {
            output.stop()?;
        }

        if let Some(handle) = self.worker_handle.take() {
            let _ = handle.join();
        }

        if let Ok(mut p) = self.player.lock() {
            p.state = PlaybackState::Paused;
        }

        Ok(())
    }

    pub fn stop(&mut self) -> Result<(), AudioError> {
        self.running.store(false, Ordering::SeqCst);

        if let Some(ref mut output) = self.output {
            output.stop()?;
        }

        if let Some(handle) = self.worker_handle.take() {
            let _ = handle.join();
        }

        self.decoder = None;

        if let Ok(mut p) = self.player.lock() {
            p.state = PlaybackState::Stopped;
            p.position_samples = 0;
        }

        Ok(())
    }

    pub fn set_volume(&mut self, volume: f32) {
        if let Ok(mut p) = self.player.lock() {
            p.set_volume(volume);
        }
    }

    pub fn set_bit_perfect(&mut self, enabled: bool) -> Result<(), AudioError> {
        if let Ok(mut p) = self.player.lock() {
            p.set_bit_perfect(enabled);
        }

        Ok(())
    }

    pub fn player_state(&self) -> Option<PlaybackState> {
        self.player.lock().ok().map(|p| p.state())
    }

    pub fn position_samples(&self) -> u64 {
        self.player
            .lock()
            .ok()
            .map(|p| p.position_samples())
            .unwrap_or(0)
    }
}

impl Default for AudioEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for AudioEngine {
    fn drop(&mut self) {
        self.running.store(false, Ordering::SeqCst);
        if let Some(handle) = self.worker_handle.take() {
            let _ = handle.join();
        }
    }
}
