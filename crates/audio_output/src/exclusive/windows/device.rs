use std::iter::once;

use audio_common::AudioError;
use windows::Win32::Media::Audio::{
    DEVICE_STATE_ACTIVE, IMMDevice, IMMDeviceEnumerator, MMDeviceEnumerator, eConsole, eRender,
};
use windows::Win32::System::Com::{CLSCTX_ALL, CoCreateInstance, CoTaskMemFree};
use windows::core::PCWSTR;

/// Creates the system device enumerator. COM must already be initialised on the
/// calling thread.
pub(super) fn create_enumerator() -> Result<IMMDeviceEnumerator, AudioError> {
    unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) }
        .map_err(|e| AudioError::Output(format!("CoCreateInstance(MMDeviceEnumerator): {}", e)))
}

/// Resolves a device UID (cpal's `Device::id().1`, which is the WASAPI endpoint
/// id string) to an `IMMDevice`.
///
/// Empty UID → the system default render endpoint. Tries `GetDevice` first; on
/// failure falls back to scanning all active render endpoints and matching ids
/// (mirrors the macOS UID-scan fallback for hot-plugged devices).
pub(super) fn resolve_device(
    enumerator: &IMMDeviceEnumerator,
    uid: &str,
) -> Result<IMMDevice, AudioError> {
    if uid.is_empty() {
        return unsafe { enumerator.GetDefaultAudioEndpoint(eRender, eConsole) }
            .map_err(|e| AudioError::DeviceNotFound(format!("GetDefaultAudioEndpoint: {}", e)));
    }

    let wide: Vec<u16> = uid.encode_utf16().chain(once(0)).collect();
    if let Ok(device) = unsafe { enumerator.GetDevice(PCWSTR(wide.as_ptr())) } {
        return Ok(device);
    }

    if let Some(device) = scan_for_uid(enumerator, uid) {
        return Ok(device);
    }

    Err(AudioError::DeviceNotFound(format!(
        "No render endpoint matches UID '{}'",
        uid
    )))
}

fn scan_for_uid(enumerator: &IMMDeviceEnumerator, target: &str) -> Option<IMMDevice> {
    let collection = unsafe { enumerator.EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE) }.ok()?;
    let count = unsafe { collection.GetCount() }.ok()?;
    for i in 0..count {
        let device = unsafe { collection.Item(i) }.ok()?;
        if let Some(id) = device_id(&device)
            && id == target
        {
            return Some(device);
        }
    }
    None
}

/// Reads a device's endpoint id string, freeing the COM-allocated buffer.
pub(super) fn device_id(device: &IMMDevice) -> Option<String> {
    let pwstr = unsafe { device.GetId() }.ok()?;
    if pwstr.is_null() {
        return None;
    }
    let s = unsafe { pwstr.to_string() }.ok();
    unsafe { CoTaskMemFree(Some(pwstr.0 as *const _)) };
    s
}
