pub mod cpal_stream;
pub mod device;
pub mod ring_buffer;
#[cfg(target_os = "macos")]
pub mod exclusive;
#[cfg(not(target_os = "macos"))]
pub mod exclusive;

use std::sync::Arc;

use audio_common::{AudioBatch, AudioError, Metadata};
pub use cpal_stream::{AudioOutput, CpalOutputStream, OutputConfig, PlaybackState, SelectedOutputDevice};
use device::DeviceManager;
use parking_lot::{Mutex, RwLock};

use crate::ring_buffer::AudioRingBuffer;

enum OutputMode {
    Shared(CpalOutputStream),
    #[cfg(target_os = "macos")]
    Exclusive(exclusive::ExclusiveOutput),
    #[cfg(not(target_os = "macos"))]
    Exclusive(exclusive::ExclusiveOutput),
}

pub struct Output {
    host: Arc<cpal::Host>,
    device_manager: RwLock<DeviceManager>,
    current: RwLock<OutputMode>,
    recreate_error: Mutex<Option<String>>,
    exclusive_original_rate: Mutex<Option<f64>>,
}

fn calc_buffer_size(cfg: &OutputConfig) -> usize {
    (cfg.bit_depth / 8) as usize
        * cfg.channels as usize
        * (cfg.sample_rate / 8) as usize
}

impl Output {
    pub fn new() -> Self {
        let host = Arc::new(cpal::default_host());
        let device_manager = DeviceManager::from_host(&host)
            .expect("Failed to initialize device manager");
        let device = device_manager.selected_device().clone();

        let output_config = OutputConfig {
            sample_rate: 44100,
            channels: 2,
            bit_depth: 16,
        };
        let selected = SelectedOutputDevice { host: host.clone(), device };
        let buffer = Arc::new(AudioRingBuffer::new(calc_buffer_size(&output_config)));

        let stream = CpalOutputStream::new(buffer, output_config, selected)
            .expect("Failed to create audio output stream");

        Self {
            host,
            device_manager: RwLock::new(device_manager),
            current: RwLock::new(OutputMode::Shared(stream)),
            recreate_error: Mutex::new(None),
            exclusive_original_rate: Mutex::new(None),
        }
    }

    fn current_config(&self) -> OutputConfig {
        match &*self.current.read() {
            OutputMode::Shared(s) => s.config,
            #[cfg(target_os = "macos")]
            OutputMode::Exclusive(e) => e.config,
            #[cfg(not(target_os = "macos"))]
            OutputMode::Exclusive(e) => e.config,
        }
    }

    fn recreate_stream(&self, metadata: Metadata) {
        let was_playing = self.is_playing();
        let new_config = OutputConfig {
            sample_rate: metadata.sample_rate,
            channels: metadata.channels.to_u8(),
            bit_depth: metadata.bit_depth,
        };

        let current_mode = {
            let current = self.current.read();
            let is_exclusive = matches!(*current, OutputMode::Exclusive(_));
            drop(current);
            is_exclusive
        };

        if !current_mode {
            let device = self.device_manager.read().selected_device().clone();
            let selected = SelectedOutputDevice { host: self.host.clone(), device };
            let buffer = Arc::new(AudioRingBuffer::new(calc_buffer_size(&new_config)));
            match CpalOutputStream::new(buffer, new_config, selected) {
                Ok(stream) => {
                    if was_playing {
                        stream.resume();
                    }
                    let mut current = self.current.write();
                    *current = OutputMode::Shared(stream);
                }
                Err(e) => {
                    *self.recreate_error.lock() =
                        Some(format!("Failed to recreate shared stream: {}", e));
                }
            }
        } else {
            #[cfg(target_os = "macos")]
            {
                let device_id = match self.device_manager.read().audio_device_id() {
                    Ok(id) => id,
                    Err(_) => return,
                };

                {
                    let current = self.current.read();
                    if let OutputMode::Exclusive(old) = &*current {
                        old.suppress_drop_cleanup();
                    }
                }

                let buffer = Arc::new(AudioRingBuffer::new(calc_buffer_size(&new_config)));
                match exclusive::ExclusiveOutput::new(buffer, new_config, device_id) {
                    Ok(mut excl) => {
                        if let Some(true_rate) = *self.exclusive_original_rate.lock() {
                            excl.original_sample_rate = true_rate;
                        }
                        if was_playing {
                            excl.resume();
                        }
                        let mut current = self.current.write();
                        *current = OutputMode::Exclusive(excl);
                    }
                    Err(e) => {
                        {
                            let current = self.current.read();
                            if let OutputMode::Exclusive(old) = &*current {
                                old.allow_drop_cleanup();
                            }
                        }
                        *self.recreate_error.lock() =
                            Some(format!("Failed to recreate exclusive stream: {}", e));
                    }
                }
            }
            #[cfg(not(target_os = "macos"))]
            {
                let _ = (was_playing, new_config);
            }
        }
    }

    pub fn set_exclusive(&self, exclusive: bool) -> Result<(), AudioError> {
        let was_playing = self.is_playing();
        let config = self.current_config();

        if exclusive {
            #[cfg(target_os = "macos")]
            {
                let audio_device_id = self.device_manager.read().audio_device_id()?;
                let buffer = Arc::new(AudioRingBuffer::new(calc_buffer_size(&config)));
                match exclusive::ExclusiveOutput::new(buffer, config, audio_device_id) {
                    Ok(exclusive_output) => {
                        *self.exclusive_original_rate.lock() =
                            Some(exclusive_output.original_sample_rate);
                        {
                            let mut current = self.current.write();
                            *current = OutputMode::Exclusive(exclusive_output);
                        }
                        if was_playing {
                            self.resume();
                        }
                        Ok(())
                    }
                    Err(e) => Err(e),
                }
            }
            #[cfg(not(target_os = "macos"))]
            {
                let _ = (was_playing, config);
                Err(AudioError::UnsupportedFormat(
                    "Exclusive mode is not supported on this platform".to_string(),
                ))
            }
        } else {
            *self.exclusive_original_rate.lock() = None;
            let device = self.device_manager.read().selected_device().clone();
            let selected = SelectedOutputDevice { host: self.host.clone(), device };
            let buffer = Arc::new(AudioRingBuffer::new(calc_buffer_size(&config)));
            let stream = CpalOutputStream::new(buffer, config, selected)?;

            {
                let mut current = self.current.write();
                *current = OutputMode::Shared(stream);
            }
            if was_playing {
                self.resume();
            }
            Ok(())
        }
    }

    pub fn is_exclusive(&self) -> bool {
        matches!(*self.current.read(), OutputMode::Exclusive(_))
    }

    pub fn selected_device_name(&self) -> String {
        self.device_manager.read().selected_device_name().to_string()
    }

    pub fn selected_device_index(&self) -> usize {
        self.device_manager.read().selected_device_index()
    }

    pub fn devices(&self) -> Vec<device::OutputDeviceInfo> {
        self.device_manager.read().output_devices()
            .unwrap_or_default()
    }

    pub fn take_recreate_error(&self) -> Option<String> {
        self.recreate_error.lock().take()
    }

    pub fn select_device(&self, index: usize) -> Result<(), AudioError> {
        let is_exclusive = self.is_exclusive();
        let was_playing = self.is_playing();

        let new_device = self.device_manager.write().select_device(index)?;

        if is_exclusive {
            #[cfg(target_os = "macos")]
            {
                let device_id = self.device_manager.read().audio_device_id()?;
                let config = self.current_config();
                let buffer = Arc::new(AudioRingBuffer::new(calc_buffer_size(&config)));
                let excl = exclusive::ExclusiveOutput::new(buffer, config, device_id)?;
                let mut current = self.current.write();
                *current = OutputMode::Exclusive(excl);
            }
            #[cfg(not(target_os = "macos"))]
            {
                let _ = (is_exclusive, new_device);
            }
        } else {
            let selected = SelectedOutputDevice {
                host: self.host.clone(),
                device: new_device,
            };
            let config = self.current_config();
            let buffer = Arc::new(AudioRingBuffer::new(calc_buffer_size(&config)));
            let stream = CpalOutputStream::new(buffer, config, selected)?;
            let mut current = self.current.write();
            *current = OutputMode::Shared(stream);
        }

        if was_playing {
            self.resume();
        }

        Ok(())
    }

    pub fn shutdown(&self) {
        self.pause();
        let mut current = self.current.write();
        let device = self.device_manager.read().selected_device().clone();
        *current = OutputMode::Shared(CpalOutputStream::new(
            Arc::new(AudioRingBuffer::new(1024)),
            OutputConfig {
                sample_rate: 44100,
                channels: 2,
                bit_depth: 16,
            },
            SelectedOutputDevice {
                host: self.host.clone(),
                device,
            },
        ).expect("Failed to create fallback stream"));
    }
}

fn is_config_match(config: &OutputConfig, metadata: &Metadata) -> bool {
    config.bit_depth == metadata.bit_depth
        && config.sample_rate == metadata.sample_rate
        && config.channels == metadata.channels.to_u8()
}

impl Default for Output {
    fn default() -> Self {
        Self::new()
    }
}

impl AudioOutput for Output {
    fn write(&self, batch: &AudioBatch) -> usize {
        let needs_recreate = {
            let current = self.current.read();
            let config = match &*current {
                OutputMode::Shared(s) => &s.config,
                #[cfg(target_os = "macos")]
                OutputMode::Exclusive(e) => &e.config,
                #[cfg(not(target_os = "macos"))]
                OutputMode::Exclusive(e) => &e.config,
            };
            !is_config_match(config, &batch.metadata)
        };

        if needs_recreate {
            self.recreate_stream(batch.metadata.clone());
        }

        match &*self.current.read() {
            OutputMode::Shared(s) => s.write(batch),
            #[cfg(target_os = "macos")]
            OutputMode::Exclusive(e) => e.write(batch),
            #[cfg(not(target_os = "macos"))]
            OutputMode::Exclusive(e) => e.write(batch),
        }
    }

    fn clear(&self) {
        match &*self.current.read() {
            OutputMode::Shared(s) => s.clear(),
            #[cfg(target_os = "macos")]
            OutputMode::Exclusive(e) => e.clear(),
            #[cfg(not(target_os = "macos"))]
            OutputMode::Exclusive(e) => e.clear(),
        }
    }

    fn pause(&self) {
        match &*self.current.read() {
            OutputMode::Shared(s) => s.pause(),
            #[cfg(target_os = "macos")]
            OutputMode::Exclusive(e) => e.pause(),
            #[cfg(not(target_os = "macos"))]
            OutputMode::Exclusive(e) => e.pause(),
        }
    }

    fn resume(&self) {
        match &*self.current.read() {
            OutputMode::Shared(s) => s.resume(),
            #[cfg(target_os = "macos")]
            OutputMode::Exclusive(e) => e.resume(),
            #[cfg(not(target_os = "macos"))]
            OutputMode::Exclusive(e) => e.resume(),
        }
    }

    fn is_playing(&self) -> bool {
        match &*self.current.read() {
            OutputMode::Shared(s) => s.is_playing(),
            #[cfg(target_os = "macos")]
            OutputMode::Exclusive(e) => e.is_playing(),
            #[cfg(not(target_os = "macos"))]
            OutputMode::Exclusive(e) => e.is_playing(),
        }
    }

    fn set_volume(&self, volume: f32) {
        match &*self.current.read() {
            OutputMode::Shared(s) => s.set_volume(volume),
            #[cfg(target_os = "macos")]
            OutputMode::Exclusive(_) => {}
            #[cfg(not(target_os = "macos"))]
            OutputMode::Exclusive(_) => {}
        }
    }
}