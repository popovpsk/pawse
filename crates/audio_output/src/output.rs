use audio_common::{AudioBatch, AudioError};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{OutputCallbackInfo, Stream, StreamConfig};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;

const BUFFER_CAPACITY: usize = 192_000 * 2 * 5;

const STATE_PLAYING: u8 = 1;
const STATE_PAUSED: u8 = 2;

#[derive(Clone, Copy, Debug)]
pub struct OutputState {
    pub channels: u8,
    pub sample_rate: u32,
}

impl Default for OutputState {
    fn default() -> Self {
        Self {
            channels: 2,
            sample_rate: 44100,
        }
    }
}

pub trait AudioOutput: Send + Sync {
    fn write(&self, samples: AudioBatch);
    fn clear(&self);
    fn buffer(&self) -> Arc<Mutex<Vec<f32>>>;
    fn pause(&self);
    fn resume(&self);
    fn is_playing(&self) -> bool;
    fn output_state(&self) -> OutputState;
}

pub struct Output {
    buffer: Arc<Mutex<Vec<f32>>>,
    state: Arc<AtomicU8>,
    output_state: OutputState,
    #[allow(dead_code)]
    command_tx: mpsc::Sender<OutputCommand>,
    #[allow(dead_code)]
    _stream: Stream,
}

#[derive(Clone)]
pub struct OutputHandle {
    buffer: Arc<Mutex<Vec<f32>>>,
    state: Arc<AtomicU8>,
    output_state: OutputState,
    command_tx: mpsc::Sender<OutputCommand>,
}

#[derive(Debug, Clone, Copy)]
enum OutputCommand {
    Pause,
    Resume,
    #[allow(dead_code)]
    Shutdown,
}

impl Output {
    pub fn default_output() -> Result<Self, AudioError> {
        let buffer = Arc::new(Mutex::new(Vec::with_capacity(BUFFER_CAPACITY)));
        let state = Arc::new(AtomicU8::new(STATE_PLAYING));

        let (cmd_tx, cmd_rx) = mpsc::channel();

        let state_clone = Arc::clone(&state);

        thread::spawn(move || loop {
            if let Ok(cmd) = cmd_rx.recv() {
                match cmd {
                    OutputCommand::Pause => {
                        state_clone.store(STATE_PAUSED, Ordering::SeqCst);
                    }
                    OutputCommand::Resume => {
                        state_clone.store(STATE_PLAYING, Ordering::SeqCst);
                    }
                    OutputCommand::Shutdown => {
                        break;
                    }
                }
            }
        });

        let buffer_for_callback = Arc::clone(&buffer);
        let state_for_callback = Arc::clone(&state);

        let callback = move |data: &mut [f32], _: &OutputCallbackInfo| {
            let playing = state_for_callback.load(Ordering::SeqCst);

            if playing != STATE_PLAYING {
                for sample in data.iter_mut() {
                    *sample = 0.0;
                }
                return;
            }

            if let Ok(mut buf) = buffer_for_callback.try_lock() {
                let samples_needed = data.len();

                if buf.len() >= samples_needed {
                    data.copy_from_slice(&buf[..samples_needed]);
                    buf.drain(..samples_needed);
                } else if !buf.is_empty() {
                    data[..buf.len()].copy_from_slice(&buf);
                    for sample in &mut data[buf.len()..] {
                        *sample = 0.0;
                    }
                    buf.clear();
                } else {
                    for sample in data.iter_mut() {
                        *sample = 0.0;
                    }
                }
            } else {
                for sample in data.iter_mut() {
                    *sample = 0.0;
                }
            }
        };

        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or_else(|| AudioError::DeviceNotFound("default".to_string()))?;

        let supported_config = device
            .default_output_config()
            .map_err(|e| AudioError::Output(e.to_string()))?;

        let output_state = OutputState {
            channels: supported_config.channels() as u8,
            sample_rate: 44100,
        };

        let stream_config = StreamConfig {
            channels: supported_config.channels(),
            sample_rate: 44100,
            buffer_size: cpal::BufferSize::Default,
        };

        let _stream = device
            .build_output_stream(
                &stream_config,
                callback,
                |err| eprintln!("Audio stream error: {}", err),
                None,
            )
            .map_err(|e| AudioError::Output(e.to_string()))?;

        _stream
            .play()
            .map_err(|e| AudioError::Output(e.to_string()))?;

        Ok(Self {
            buffer,
            state,
            output_state,
            command_tx: cmd_tx,
            _stream,
        })
    }

    pub fn handle(&self) -> OutputHandle {
        OutputHandle {
            buffer: Arc::clone(&self.buffer),
            state: Arc::clone(&self.state),
            output_state: self.output_state,
            command_tx: self.command_tx.clone(),
        }
    }
}

impl OutputHandle {
    fn do_write(&self, batch: AudioBatch) {
        if self.state.load(Ordering::SeqCst) != STATE_PLAYING {
            return;
        }

        let f32_samples = batch.data.to_f32();

        if let Ok(mut buf) = self.buffer.lock() {
            if buf.capacity() < buf.len() + f32_samples.len() {
                buf.reserve(f32_samples.len());
            }
            buf.extend_from_slice(&f32_samples);
        }
    }

    fn do_clear(&self) {
        if let Ok(mut buf) = self.buffer.lock() {
            buf.clear();
        }
    }
}

impl AudioOutput for Output {
    fn write(&self, samples: AudioBatch) {
        OutputHandle {
            buffer: Arc::clone(&self.buffer),
            state: Arc::clone(&self.state),
            output_state: self.output_state,
            command_tx: self.command_tx.clone(),
        }
        .do_write(samples);
    }

    fn clear(&self) {
        if let Ok(mut buf) = self.buffer.lock() {
            buf.clear();
        }
    }

    fn buffer(&self) -> Arc<Mutex<Vec<f32>>> {
        Arc::clone(&self.buffer)
    }

    fn pause(&self) {
        let _ = self.command_tx.send(OutputCommand::Pause);
    }

    fn resume(&self) {
        let _ = self.command_tx.send(OutputCommand::Resume);
    }

    fn is_playing(&self) -> bool {
        self.state.load(Ordering::SeqCst) == STATE_PLAYING
    }

    fn output_state(&self) -> OutputState {
        self.output_state
    }
}

impl AudioOutput for OutputHandle {
    fn write(&self, samples: AudioBatch) {
        self.do_write(samples);
    }

    fn clear(&self) {
        self.do_clear();
    }

    fn buffer(&self) -> Arc<Mutex<Vec<f32>>> {
        Arc::clone(&self.buffer)
    }

    fn pause(&self) {
        let _ = self.command_tx.send(OutputCommand::Pause);
    }

    fn resume(&self) {
        let _ = self.command_tx.send(OutputCommand::Resume);
    }

    fn is_playing(&self) -> bool {
        self.state.load(Ordering::SeqCst) == STATE_PLAYING
    }

    fn output_state(&self) -> OutputState {
        self.output_state
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use audio_common::{AudioSamples, ChannelCount, Metadata};

    fn make_test_batch(samples: Vec<f32>) -> AudioBatch {
        AudioBatch {
            data: AudioSamples::F32(samples),
            metadata: Metadata {
                sample_rate: 44100,
                channels: ChannelCount::Stereo,
                bit_depth: 32,
            },
        }
    }

    #[test]
    fn test_default_output() {
        let output = Output::default_output();
        assert!(output.is_ok(), "Should create default output");
    }

    #[test]
    fn test_output_handle_clone() {
        let output = Output::default_output().unwrap();
        let handle1 = output.handle();
        let handle2 = output.handle();

        handle1.write(make_test_batch(vec![0.5, -0.5]));
        handle2.write(make_test_batch(vec![0.3, -0.3]));

        let buf = output.handle().buffer();
        let samples = buf.lock().unwrap();
        assert_eq!(samples.len(), 4);
    }

    #[test]
    fn test_pause_resume() {
        let output = Output::default_output().unwrap();

        assert!(output.is_playing());

        output.pause();
        thread::sleep(std::time::Duration::from_millis(10));
        assert!(!output.is_playing());

        output.resume();
        thread::sleep(std::time::Duration::from_millis(10));
        assert!(output.is_playing());
    }

    #[test]
    fn test_write_when_paused() {
        let output = Output::default_output().unwrap();
        output.pause();
        thread::sleep(std::time::Duration::from_millis(10));

        output.write(make_test_batch(vec![0.5, -0.5]));

        let buf = output.handle().buffer();
        let samples = buf.lock().unwrap();
        assert!(samples.is_empty(), "Should not write when paused");
    }

    #[test]
    fn test_clear() {
        let output = Output::default_output().unwrap();
        output.write(make_test_batch(vec![0.5, -0.5, 0.3, -0.3]));

        let buf = output.handle().buffer();
        let samples = buf.lock().unwrap();
        assert_eq!(samples.len(), 4);

        drop(samples);
        output.clear();

        let samples = buf.lock().unwrap();
        assert!(samples.is_empty());
    }

    #[test]
    fn test_convert_s16_to_f32() {
        let samples = AudioSamples::S16(vec![0, 32767, -32768, 0]);
        let f32_samples = samples.to_f32();

        assert_eq!(f32_samples.len(), 4);
        assert!((f32_samples[0] - 0.0).abs() < 0.001);
        assert!((f32_samples[1] - 1.0).abs() < 0.001);
        assert!((f32_samples[2] - (-1.0)).abs() < 0.001);
        assert!((f32_samples[3] - 0.0).abs() < 0.001);
    }

    #[test]
    fn test_convert_u8_to_f32() {
        let samples = AudioSamples::U8(vec![0, 128, 255]);
        let f32_samples = samples.to_f32();

        assert_eq!(f32_samples.len(), 3);
        assert!((f32_samples[0] - (-1.0)).abs() < 0.01);
        assert!((f32_samples[1] - 0.0).abs() < 0.01);
        assert!((f32_samples[2] - 1.0).abs() < 0.01);
    }

    #[test]
    fn test_output_state() {
        let output = Output::default_output().unwrap();
        let state = output.output_state();

        assert_eq!(state.channels, 2);
        assert_eq!(state.sample_rate, 44100);
    }
}
