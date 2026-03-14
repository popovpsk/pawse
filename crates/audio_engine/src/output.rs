use audio_common::AudioError;
use cpal::traits::{DeviceTrait, StreamTrait};
use cpal::{Device, OutputCallbackInfo, Stream, StreamConfig};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

pub const BUFFER_CAPACITY: usize = 192_000 * 2 * 5;

pub struct Output {
    command_tx: mpsc::Sender<OutputCommand>,
    buffer: Arc<Mutex<Vec<f32>>>,
    is_playing: Arc<AtomicBool>,
    #[allow(dead_code)]
    stream: Stream,
}

#[derive(Debug, Clone, Copy)]
pub enum OutputCommand {
    Pause,
    Resume,
    Shutdown,
}

impl Output {
    pub fn new(device: &Device) -> Result<Self, AudioError> {
        let buffer = Arc::new(Mutex::new(Vec::with_capacity(BUFFER_CAPACITY * 2)));
        let buffer_clone = Arc::clone(&buffer);

        let is_playing = Arc::new(AtomicBool::new(true));
        let is_playing_clone = Arc::clone(&is_playing);

        let (cmd_tx, cmd_rx) = mpsc::channel();

        let stream_config = StreamConfig {
            channels: 2,
            sample_rate: 44100u32,
            buffer_size: cpal::BufferSize::Default,
        };

        let callback = move |data: &mut [f32], _: &OutputCallbackInfo| {
            if let Ok(mut buf) = buffer_clone.try_lock() {
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

        let stream = device
            .build_output_stream(
                &stream_config,
                callback,
                |err| eprintln!("Audio stream error: {}", err),
                None,
            )
            .map_err(|e| AudioError::Output(e.to_string()))?;

        stream
            .play()
            .map_err(|e| AudioError::Output(e.to_string()))?;

        thread::spawn(move || loop {
            if let Ok(cmd) = cmd_rx.recv_timeout(Duration::from_millis(10)) {
                match cmd {
                    OutputCommand::Pause => {
                        is_playing_clone.store(false, Ordering::SeqCst);
                    }
                    OutputCommand::Resume => {
                        is_playing_clone.store(true, Ordering::SeqCst);
                    }
                    OutputCommand::Shutdown => {
                        break;
                    }
                }
            }
            thread::sleep(Duration::from_millis(5));
        });

        Ok(Self {
            command_tx: cmd_tx,
            buffer,
            is_playing,
            stream,
        })
    }

    pub fn pause(&self) {
        let _ = self.command_tx.send(OutputCommand::Pause);
    }

    pub fn resume(&self) {
        let _ = self.command_tx.send(OutputCommand::Resume);
    }

    pub fn shutdown(&self) {
        let _ = self.command_tx.send(OutputCommand::Shutdown);
    }

    pub fn write(&self, samples: &[f32]) {
        if !self.is_playing.load(Ordering::SeqCst) {
            return;
        }
        if let Ok(mut buf) = self.buffer.lock() {
            if buf.capacity() < buf.len() + samples.len() {
                buf.reserve(samples.len());
            }
            buf.extend_from_slice(samples);
        }
    }

    pub fn clear(&self) {
        if let Ok(mut buf) = self.buffer.lock() {
            buf.clear();
        }
    }
}
