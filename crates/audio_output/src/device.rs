use std::sync::Arc;

use audio_common::AudioError;
use cpal::traits::{DeviceTrait, HostTrait};

pub struct OutputDeviceInfo {
    pub name: String,
    pub is_default: bool,
}

pub struct DeviceManager {
    host: Arc<cpal::Host>,
    selected_device: Arc<cpal::Device>,
    selected_device_name: String,
}

impl DeviceManager {
    pub fn from_host(host: &Arc<cpal::Host>) -> Result<Self, AudioError> {
        let default_device = host
            .default_output_device()
            .ok_or_else(|| AudioError::DeviceNotFound("No default output device".to_string()))?;
        let name = default_device
            .description()
            .map(|d| d.name().to_string())
            .unwrap_or_else(|_| "Unknown".to_string());

        Ok(Self {
            host: host.clone(),
            selected_device: Arc::new(default_device),
            selected_device_name: name,
        })
    }

    pub fn output_devices(&self) -> Result<Vec<OutputDeviceInfo>, AudioError> {
        let default_device_name = self
            .host
            .default_output_device()
            .map(|d| d.description().map(|desc| desc.name().to_string()).unwrap_or_default());

        let devices: Vec<OutputDeviceInfo> = self
            .host
            .output_devices()
            .map_err(|e| AudioError::Output(e.to_string()))?
            .map(|d| {
                let name = d.description()
                    .map(|desc| desc.name().to_string())
                    .unwrap_or_else(|_| "Unknown".to_string());
                let is_default = default_device_name
                    .as_ref()
                    .is_some_and(|dn| dn == &name);
                OutputDeviceInfo { name, is_default }
            })
            .collect();

        Ok(devices)
    }

    pub fn selected_device(&self) -> &Arc<cpal::Device> {
        &self.selected_device
    }

    pub fn select_device(&mut self, index: usize) -> Result<Arc<cpal::Device>, AudioError> {
        let devices: Vec<cpal::Device> = self
            .host
            .output_devices()
            .map_err(|e| AudioError::Output(e.to_string()))?
            .collect();

        let device = devices
            .into_iter()
            .nth(index)
            .ok_or_else(|| AudioError::DeviceNotFound(format!("Device index {} out of range", index)))?;

        let name = device
            .description()
            .map(|d| d.name().to_string())
            .unwrap_or_else(|_| "Unknown".to_string());

        self.selected_device = Arc::new(device);
        self.selected_device_name = name;

        Ok(self.selected_device.clone())
    }
}

#[cfg(target_os = "macos")]
impl DeviceManager {
    pub fn audio_device_id(&self) -> Result<u32, AudioError> {
        use coreaudio::audio_unit::macos_helpers;

        let coreaudio_ids = macos_helpers::get_audio_device_ids_for_scope(
            coreaudio::audio_unit::Scope::Output,
        )
        .map_err(|e| AudioError::DeviceNotFound(format!("Failed to enumerate CoreAudio devices: {:?}", e)))?;

        for id in &coreaudio_ids {
            let ca_name = macos_helpers::get_device_name(*id)
                .map_err(|e| AudioError::DeviceNotFound(format!("Failed to get device name: {:?}", e)))?;

            if ca_name == self.selected_device_name {
                return Ok(*id);
            }
        }

        Err(AudioError::DeviceNotFound(
            "No CoreAudio device matches selected device name".to_string(),
        ))
    }
}