use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
use windows::Win32::Media::Audio::IMMDevice;
use windows::Win32::System::Com::CLSCTX_ALL;

/// Activates the endpoint-volume control for a device, if it exposes one.
pub(super) fn activate(device: &IMMDevice) -> Option<IAudioEndpointVolume> {
    unsafe { device.Activate(CLSCTX_ALL, None) }.ok()
}

/// Reads the master volume scalar in [0.0, 1.0]; 1.0 if unavailable.
pub(super) fn read_volume(endpoint: &IAudioEndpointVolume) -> f32 {
    unsafe { endpoint.GetMasterVolumeLevelScalar() }.unwrap_or(1.0)
}

/// Reads the master mute state; false if unavailable.
pub(super) fn read_muted(endpoint: &IAudioEndpointVolume) -> bool {
    unsafe { endpoint.GetMute() }
        .map(|b| b.as_bool())
        .unwrap_or(false)
}

/// Writes the master volume scalar (0.0–1.0). Best-effort.
pub(super) fn set_volume(endpoint: &IAudioEndpointVolume, volume: f32) {
    let _ =
        unsafe { endpoint.SetMasterVolumeLevelScalar(volume.clamp(0.0, 1.0), std::ptr::null()) };
}
