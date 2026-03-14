use audio_common::{AudioBuffer, AudioError, ChannelCount, StreamParams};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Device, OutputCallbackInfo, SampleFormat, Stream, StreamConfig};
use std::ops::RangeInclusive;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

// ============================================================================
// Публичные структуры
// ============================================================================

/// Информация об аудио устройстве (физическое или виртуальное)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceInfo {
    /// Уникальный идентификатор (для cpal - имя устройства)
    pub id: String,
    /// Человекочитаемое имя
    pub name: String,
    /// Является ли устройство системным по умолчанию
    pub is_default: bool,
}

/// Поддерживаемый устройством формат аудио
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupportedFormat {
    /// Доступные частоты дискретизации (напр. 44100..=192000)
    pub sample_rates: RangeInclusive<u32>,
    /// Количество каналов
    pub channels: ChannelCount,
    /// Битовая глубина (16, 24, 32)
    pub bit_depth: u8,
}

/// Режим вывода (для будущей поддержки exclusive mode)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OutputMode {
    #[default]
    Shared, // OS mixer (по умолчанию)
    Exclusive, // Прямой доступ к железу (future)
}

// ============================================================================
// ПубличныеTraits
// ============================================================================

/// Представляет аудио устройство вывода (физическое или виртуальное)
pub trait AudioDevice: Send + Sync {
    /// Информация об устройстве
    fn info(&self) -> &DeviceInfo;
    /// Все форматы, поддерживаемые устройством
    fn supported_formats(&self) -> Vec<SupportedFormat>;
    /// Открыть воспроизведение с указанным форматом
    fn open_playback(&self, format: StreamParams) -> Result<Box<dyn AudioPlayback>, AudioError>;
}

/// Представляет активную сессию воспроизведения аудио
pub trait AudioPlayback: Send {
    /// Записать интерливированные аудио сэмплы (f32, -1.0 до 1.0)
    fn write(&self, buffer: &AudioBuffer) -> Result<(), AudioError>;
    /// Приостановить воспроизведение
    fn pause(&self) -> Result<(), AudioError>;
    /// Возобновить воспроизведение
    fn resume(&self) -> Result<(), AudioError>;
    /// Проверить, воспроизводится ли сейчас
    fn is_playing(&self) -> bool;
    /// Текущий формат воспроизведения (может отличаться от запрошенного)
    fn format(&self) -> StreamParams;
}

// ============================================================================
// Публичные функции API
// ============================================================================

/// Список всех доступных устройств вывода (реальное железо + виртуальные)
pub fn list_devices() -> Vec<DeviceInfo> {
    let host = cpal::default_host();
    let default_output = host.default_output_device();

    host.output_devices()
        .map(|devices| {
            devices
                .filter_map(|dev| {
                    let name = dev.description().ok()?.to_string();
                    let is_default = default_output
                        .as_ref()
                        .and_then(|d| d.description().ok())
                        .map(|n| n.to_string() == name)
                        .unwrap_or(false);

                    Some(DeviceInfo {
                        id: name.clone(),
                        name,
                        is_default,
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Получить устройство по ID (из DeviceInfo), или "default" для системного по умолчанию
pub fn get_device(id: &str) -> Result<Box<dyn AudioDevice>, AudioError> {
    if id == "default" {
        return default_device();
    }

    // Find device by matching description
    let host = cpal::default_host();
    let devices: Vec<_> = host
        .output_devices()
        .map_err(|e| AudioError::Output(e.to_string()))?
        .filter_map(|dev| dev.description().ok().map(|d| (dev, d.to_string())))
        .collect();

    let found = devices.into_iter().find(|(_, name)| *name == id);

    match found {
        Some((device, _)) => Ok(Box::new(CpalDevice::new(device, false))),
        None => Err(AudioError::DeviceNotFound(id.to_string())),
    }
}

/// Получить системное устройство вывода по умолчанию
pub fn default_device() -> Result<Box<dyn AudioDevice>, AudioError> {
    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .ok_or_else(|| AudioError::DeviceNotFound("default".to_string()))?;

    Ok(Box::new(CpalDevice::new(device, true)))
}

// ============================================================================
// cpal реализация
// ============================================================================

struct CpalDevice {
    device: Device,
    info: DeviceInfo,
    formats: Vec<SupportedFormat>,
}

impl CpalDevice {
    fn new(device: Device, is_default: bool) -> Self {
        let name = device
            .description()
            .map(|d| d.to_string())
            .unwrap_or_default();
        let formats = Self::query_formats(&device);

        Self {
            device,
            info: DeviceInfo {
                id: name.clone(),
                name,
                is_default,
            },
            formats,
        }
    }

    /// Запрашивает поддерживаемые форматы у устройства
    fn query_formats(device: &Device) -> Vec<SupportedFormat> {
        device
            .supported_output_configs()
            .map(|configs| {
                configs
                    .filter_map(|config| {
                        let sample_rate = config.min_sample_rate()..=config.max_sample_rate();
                        let channels = ChannelCount::from_u8(config.channels() as u8);

                        let bit_depth = match config.sample_format() {
                            SampleFormat::I16 => 16,
                            SampleFormat::U16 => 16,
                            SampleFormat::F32 => 32,
                            _ => return None,
                        };

                        Some(SupportedFormat {
                            sample_rates: sample_rate,
                            channels,
                            bit_depth,
                        })
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Находит наиболее подходящий формат для запрошенного
    fn find_matching_format(
        formats: &[SupportedFormat],
        requested: StreamParams,
    ) -> Option<SupportedFormat> {
        // Точное совпадение
        for f in formats {
            if f.sample_rates.contains(&requested.sample_rate)
                && f.channels == requested.channels
                && f.bit_depth == requested.bit_depth
            {
                return Some(f.clone());
            }
        }

        // Поиск с тем же количеством каналов, ближайшая частота
        let same_channels: Vec<_> = formats
            .iter()
            .filter(|f| f.channels == requested.channels)
            .collect();

        if !same_channels.is_empty() {
            let mut closest = same_channels[0];
            let mut min_diff = u32::MAX;

            for f in &same_channels {
                let sr = *f.sample_rates.start();
                let diff = (sr as i32 - requested.sample_rate as i32).unsigned_abs() as u32;
                if diff < min_diff {
                    min_diff = diff;
                    closest = f;
                }
            }

            return Some(closest.clone());
        }

        // Берем первый попавшийся
        formats.first().cloned()
    }
}

impl AudioDevice for CpalDevice {
    fn info(&self) -> &DeviceInfo {
        &self.info
    }

    fn supported_formats(&self) -> Vec<SupportedFormat> {
        self.formats.clone()
    }

    fn open_playback(&self, format: StreamParams) -> Result<Box<dyn AudioPlayback>, AudioError> {
        // Найти подходящий формат
        let matched = Self::find_matching_format(&self.formats, format).ok_or_else(|| {
            AudioError::UnsupportedFormat(format!("No suitable format for {:?}", format))
        })?;

        // Создаем конфиг для cpal
        let sample_rate: cpal::SampleRate = (*matched.sample_rates.start()).into();
        let stream_config = StreamConfig {
            channels: matched.channels.to_u8() as u16,
            sample_rate,
            buffer_size: cpal::BufferSize::Default,
        };

        CpalPlayback::new(&self.device, &stream_config, format)
    }
}

// ============================================================================
// Реализация воспроизведения через cpal
// ============================================================================

struct CpalPlayback {
    buffer: Arc<Mutex<Vec<f32>>>,
    is_playing: Arc<AtomicBool>,
    requested_format: StreamParams,
    #[allow(dead_code)]
    actual_format: StreamParams,
    #[allow(dead_code)]
    _stream: Stream,
}

impl CpalPlayback {
    fn new(
        device: &Device,
        stream_config: &StreamConfig,
        requested_format: StreamParams,
    ) -> Result<Box<dyn AudioPlayback>, AudioError> {
        let buffer = Arc::new(Mutex::new(Vec::new()));
        let buffer_clone = Arc::clone(&buffer);
        let is_playing = Arc::new(AtomicBool::new(true));

        // Callback который cpal вызывает когда нужен звук
        let callback = move |data: &mut [f32], _: &OutputCallbackInfo| {
            let mut buf = buffer_clone.lock().unwrap();
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
        };

        // Создаем поток
        let stream = device
            .build_output_stream(
                stream_config,
                callback,
                |err| eprintln!("Audio stream error: {}", err),
                None,
            )
            .map_err(|e| AudioError::Output(e.to_string()))?;

        // Запускаем поток
        stream
            .play()
            .map_err(|e| AudioError::Output(e.to_string()))?;

        let actual_format = StreamParams::new(
            stream_config.sample_rate.into(),
            ChannelCount::from_u8(stream_config.channels as u8),
            32,
        );

        let playback = Self {
            buffer,
            is_playing,
            requested_format,
            actual_format,
            _stream: stream,
        };

        Ok(Box::new(playback))
    }
}

impl AudioPlayback for CpalPlayback {
    fn write(&self, buffer: &AudioBuffer) -> Result<(), AudioError> {
        if !self.is_playing.load(Ordering::Relaxed) {
            return Err(AudioError::InvalidState("Playback is paused".to_string()));
        }

        let mut buf = self
            .buffer
            .lock()
            .map_err(|e| AudioError::Output(e.to_string()))?;
        buf.extend_from_slice(buffer.as_slice());
        Ok(())
    }

    fn pause(&self) -> Result<(), AudioError> {
        self.is_playing.store(false, Ordering::Relaxed);
        Ok(())
    }

    fn resume(&self) -> Result<(), AudioError> {
        self.is_playing.store(true, Ordering::Relaxed);
        Ok(())
    }

    fn is_playing(&self) -> bool {
        self.is_playing.load(Ordering::Relaxed)
    }

    fn format(&self) -> StreamParams {
        self.requested_format
    }
}

// ============================================================================
// Тесты
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_list_devices() {
        let devices = list_devices();
        assert!(
            !devices.is_empty(),
            "Should have at least one output device"
        );

        // Проверяем что есть устройство по умолчанию
        let has_default = devices.iter().any(|d| d.is_default);
        assert!(has_default, "Should have a default device");

        // У всех устройств должно быть имя
        for dev in &devices {
            assert!(!dev.name.is_empty(), "Device should have a name");
            assert!(!dev.id.is_empty(), "Device should have an id");
        }
    }

    #[test]
    fn test_default_device() {
        let device = default_device();
        assert!(device.is_ok(), "Should get default device");

        let device = device.unwrap();
        let info = device.info();
        assert!(
            info.is_default,
            "Default device should be marked as default"
        );
    }

    #[test]
    fn test_get_device_by_id() {
        let devices = list_devices();
        if let Some(first) = devices.first() {
            let device = get_device(&first.id);
            assert!(device.is_ok(), "Should find device by id");
        }
    }

    #[test]
    fn test_get_device_default_string() {
        let device = get_device("default");
        assert!(device.is_ok(), "Should accept 'default' string");
    }

    #[test]
    fn test_get_nonexistent_device() {
        let result = get_device("nonexistent_device_12345");
        assert!(result.is_err(), "Should fail for nonexistent device");
    }

    #[test]
    fn test_supported_formats() {
        let device = default_device().unwrap();
        let formats = device.supported_formats();
        assert!(
            !formats.is_empty(),
            "Device should support at least one format"
        );

        for format in &formats {
            assert!(
                *format.sample_rates.start() <= *format.sample_rates.end(),
                "Sample rate range should be valid"
            );
            assert!(format.channels.to_u8() > 0, "Channels should be > 0");
        }
    }

    #[test]
    fn test_device_info() {
        let device = default_device().unwrap();
        let info = device.info();

        assert!(!info.id.is_empty());
        assert!(!info.name.is_empty());
    }
}
