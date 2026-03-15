use audio_common::AudioError;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{OutputCallbackInfo, Stream, StreamConfig};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;

const BUFFER_CAPACITY: usize = 192_000 * 2 * 5;

const STATE_PLAYING: u8 = 1;
const STATE_PAUSED: u8 = 2;

/// Трейт для аудио вывода.
/// Реализуется Output и OutputHandle.
pub trait AudioOutput: Send + Sync {
    /// Записать интерливированные f32 сэмплы (-1.0 до 1.0) в буфер
    fn write(&self, samples: &[f32]);
    /// Очистить буфер от всех сэмплов
    fn clear(&self);
    /// Получить доступ к буферу (для прямого чтения)
    fn buffer(&self) -> Arc<Mutex<Vec<f32>>>;
    /// Приостановить воспроизведение (cpal stream продолжает работать)
    fn pause(&self);
    /// Возобновить воспроизведение
    fn resume(&self);
    /// Проверить, воспроизводится ли сейчас
    fn is_playing(&self) -> bool;
}

/// Основной аудио output: владеет cpal stream, хранит буфер и состояние.
/// Создаётся один раз в главном потоке, управляет воспроизведением.
pub struct Output {
    buffer: Arc<Mutex<Vec<f32>>>,
    state: Arc<AtomicU8>,
    #[allow(dead_code)]
    command_tx: mpsc::Sender<OutputCommand>,
    #[allow(dead_code)]
    _stream: Stream,
}

/// Лёгкая копия Output для передачи в AudioEngine.
/// Реализует Clone, может передаваться между потоками.
#[derive(Clone)]
pub struct OutputHandle {
    buffer: Arc<Mutex<Vec<f32>>>,
    state: Arc<AtomicU8>,
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
    /// Создать Output для устройства по умолчанию: инициализирует cpal stream.
    /// Должен вызываться в главном потоке приложения.
    pub fn default_output() -> Result<Self, AudioError> {
        let buffer = Arc::new(Mutex::new(Vec::with_capacity(BUFFER_CAPACITY * 2)));
        let state = Arc::new(AtomicU8::new(STATE_PLAYING));

        // Отдельный поток для обработки команд pause/resume
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

        // Callback cpal: вызывается потоком устройства, читает из буфера
        let buffer_for_callback = Arc::clone(&buffer);
        let state_for_callback = Arc::clone(&state);

        let callback = move |data: &mut [f32], _: &OutputCallbackInfo| {
            let playing = state_for_callback.load(Ordering::SeqCst);

            // Если на паузе — заполняем тишиной
            if playing != STATE_PLAYING {
                for sample in data.iter_mut() {
                    *sample = 0.0;
                }
                return;
            }

            // Берём сэмплы из буфера, если не хватает — дополняем тишиной
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

        // Создаём stream для устройства по умолчанию
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or_else(|| AudioError::DeviceNotFound("default".to_string()))?;

        let stream_config = StreamConfig {
            channels: 2,
            sample_rate: cpal::SampleRate::from(44100u32),
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
            command_tx: cmd_tx,
            _stream,
        })
    }

    /// Получить клонируемый handle для использования в AudioEngine
    pub fn handle(&self) -> OutputHandle {
        OutputHandle {
            buffer: Arc::clone(&self.buffer),
            state: Arc::clone(&self.state),
            command_tx: self.command_tx.clone(),
        }
    }
}

impl AudioOutput for Output {
    fn write(&self, samples: &[f32]) {
        if self.state.load(Ordering::SeqCst) != STATE_PLAYING {
            return;
        }
        if let Ok(mut buf) = self.buffer.lock() {
            if buf.capacity() < buf.len() + samples.len() {
                buf.reserve(samples.len());
            }
            buf.extend_from_slice(samples);
        }
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
}

impl AudioOutput for OutputHandle {
    fn write(&self, samples: &[f32]) {
        if self.state.load(Ordering::SeqCst) != STATE_PLAYING {
            return;
        }
        if let Ok(mut buf) = self.buffer.lock() {
            if buf.capacity() < buf.len() + samples.len() {
                buf.reserve(samples.len());
            }
            buf.extend_from_slice(samples);
        }
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
}

#[cfg(test)]
mod tests {
    use super::*;

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

        // Оба handle должны работать
        handle1.write(&[0.5, -0.5]);
        handle2.write(&[0.3, -0.3]);

        let buf = output.buffer();
        let samples = buf.lock().unwrap();
        assert_eq!(samples.len(), 4);
    }

    #[test]
    fn test_pause_resume() {
        let output = Output::default_output().unwrap();

        assert!(output.is_playing());

        output.pause();
        // Дать время на обработку команды
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

        output.write(&[0.5, -0.5]);

        let buf = output.buffer();
        let samples = buf.lock().unwrap();
        assert!(samples.is_empty(), "Should not write when paused");
    }

    #[test]
    fn test_clear() {
        let output = Output::default_output().unwrap();
        output.write(&[0.5, -0.5, 0.3, -0.3]);

        let buf = output.buffer();
        let samples = buf.lock().unwrap();
        assert_eq!(samples.len(), 4);

        drop(samples);
        output.clear();

        let samples = buf.lock().unwrap();
        assert!(samples.is_empty());
    }
}
