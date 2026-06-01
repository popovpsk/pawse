use std::mem;
use std::ptr::{self, NonNull};

use audio_common::AudioError;
use objc2_core_audio::{
    AudioObjectGetPropertyData, AudioObjectGetPropertyDataSize, AudioObjectPropertyAddress,
    AudioObjectSetPropertyData, kAudioDevicePropertyDeviceUID, kAudioDevicePropertyHogMode,
    kAudioHardwarePropertyDefaultOutputDevice, kAudioHardwarePropertyDevices,
    kAudioHardwarePropertyTranslateUIDToDevice, kAudioObjectPropertyElementMain,
    kAudioObjectPropertyScopeGlobal, kAudioObjectSystemObject,
};

use super::cf::{CFRelease, CFStringRef, cfstring_from_str, cfstring_to_string};

// kAudioDevicePropertyScopeOutput = 'outp'
pub(super) const K_SCOPE_OUTPUT: u32 = 0x6f757470;

pub(super) fn get_default_device_id() -> Result<u32, AudioError> {
    let addr = AudioObjectPropertyAddress {
        mSelector: kAudioHardwarePropertyDefaultOutputDevice,
        mScope: kAudioObjectPropertyScopeGlobal,
        mElement: kAudioObjectPropertyElementMain,
    };
    let mut device_id: u32 = 0;
    let mut data_size = mem::size_of::<u32>() as u32;
    unsafe {
        let status = AudioObjectGetPropertyData(
            kAudioObjectSystemObject as u32,
            NonNull::from(&addr),
            0,
            ptr::null(),
            NonNull::from(&mut data_size),
            NonNull::from(&mut device_id).cast(),
        );
        if status != 0 {
            return Err(AudioError::DeviceNotFound(format!(
                "kAudioHardwarePropertyDefaultOutputDevice: {:#x}",
                status
            )));
        }
    }
    if device_id == 0 {
        return Err(AudioError::DeviceNotFound(
            "System has no default output device".to_string(),
        ));
    }
    Ok(device_id)
}

/// Resolves a CoreAudio device UID (the same string cpal exposes via `Device::id()`)
/// to the device's `AudioDeviceID`.
///
/// Tries `kAudioHardwarePropertyTranslateUIDToDevice` first (fast path). If that
/// returns 0/error — common for hot-plugged devices when cpal's UID format
/// doesn't roundtrip cleanly — falls back to scanning all known audio devices
/// and matching each one's `kAudioDevicePropertyDeviceUID`.
///
/// Returns `DeviceNotFound` only if both paths fail.
pub(super) fn get_device_id_by_uid(uid: &str) -> Result<u32, AudioError> {
    if uid.is_empty() {
        return get_default_device_id();
    }

    if let Ok(id) = translate_uid_fast(uid)
        && id != 0
    {
        return Ok(id);
    }

    if let Some(id) = find_device_by_uid_scan(uid) {
        return Ok(id);
    }

    Err(AudioError::DeviceNotFound(format!(
        "No device matches UID '{}'",
        uid
    )))
}

fn translate_uid_fast(uid: &str) -> Result<u32, AudioError> {
    let cfstr = cfstring_from_str(uid);
    if cfstr.is_null() {
        return Err(AudioError::DeviceNotFound(format!(
            "Failed to wrap device UID '{}' as CFString",
            uid
        )));
    }

    let addr = AudioObjectPropertyAddress {
        mSelector: kAudioHardwarePropertyTranslateUIDToDevice,
        mScope: kAudioObjectPropertyScopeGlobal,
        mElement: kAudioObjectPropertyElementMain,
    };
    let mut device_id: u32 = 0;
    let mut data_size = mem::size_of::<u32>() as u32;
    let qualifier_ptr: *const CFStringRef = &cfstr;
    let status = unsafe {
        AudioObjectGetPropertyData(
            kAudioObjectSystemObject as u32,
            NonNull::from(&addr),
            mem::size_of::<CFStringRef>() as u32,
            qualifier_ptr as *const _,
            NonNull::from(&mut data_size),
            NonNull::from(&mut device_id).cast(),
        )
    };
    unsafe { CFRelease(cfstr) };

    if status != 0 {
        return Err(AudioError::DeviceNotFound(format!(
            "TranslateUIDToDevice '{}': {:#x}",
            uid, status
        )));
    }
    Ok(device_id)
}

/// Enumerates all audio devices and returns the first whose UID equals `target`.
/// Used as a fallback when `kAudioHardwarePropertyTranslateUIDToDevice` returns 0
/// even though the device is enumerable (observed on some hot-plugged USB DACs).
pub(super) fn find_device_by_uid_scan(target: &str) -> Option<u32> {
    let devices = enumerate_audio_device_ids().ok()?;
    for device_id in devices {
        if let Some(uid) = read_device_uid(device_id)
            && uid == target
        {
            return Some(device_id);
        }
    }
    None
}

fn enumerate_audio_device_ids() -> Result<Vec<u32>, AudioError> {
    let addr = AudioObjectPropertyAddress {
        mSelector: kAudioHardwarePropertyDevices,
        mScope: kAudioObjectPropertyScopeGlobal,
        mElement: kAudioObjectPropertyElementMain,
    };
    let mut data_size: u32 = 0;
    let status = unsafe {
        AudioObjectGetPropertyDataSize(
            kAudioObjectSystemObject as u32,
            NonNull::from(&addr),
            0,
            ptr::null(),
            NonNull::from(&mut data_size),
        )
    };
    if status != 0 || data_size == 0 {
        return Err(AudioError::Output(format!(
            "kAudioHardwarePropertyDevices size: {:#x}",
            status
        )));
    }
    let count = data_size as usize / mem::size_of::<u32>();
    let mut ids = vec![0u32; count];
    let status = unsafe {
        AudioObjectGetPropertyData(
            kAudioObjectSystemObject as u32,
            NonNull::from(&addr),
            0,
            ptr::null(),
            NonNull::from(&mut data_size),
            NonNull::new(ids.as_mut_ptr()).unwrap().cast(),
        )
    };
    if status != 0 {
        return Err(AudioError::Output(format!(
            "kAudioHardwarePropertyDevices data: {:#x}",
            status
        )));
    }
    Ok(ids)
}

pub(super) fn read_device_uid(device_id: u32) -> Option<String> {
    let addr = AudioObjectPropertyAddress {
        mSelector: kAudioDevicePropertyDeviceUID,
        mScope: kAudioObjectPropertyScopeGlobal,
        mElement: kAudioObjectPropertyElementMain,
    };
    let mut cfstr: CFStringRef = ptr::null();
    let mut data_size = mem::size_of::<CFStringRef>() as u32;
    let status = unsafe {
        AudioObjectGetPropertyData(
            device_id,
            NonNull::from(&addr),
            0,
            ptr::null(),
            NonNull::from(&mut data_size),
            NonNull::from(&mut cfstr).cast(),
        )
    };
    if status != 0 || cfstr.is_null() {
        return None;
    }
    let s = cfstring_to_string(cfstr);
    unsafe { CFRelease(cfstr) };
    s
}

/// Acquires exclusive hog mode on the device.
///
/// Returns `Ok(true)` if hog was freshly acquired — caller must release on teardown.
/// Returns `Ok(false)` if this process already held hog before the call — caller
/// must NOT release (doing so would incorrectly give up a hold another code path owns).
pub(super) fn acquire_hog_mode(device_id: u32) -> Result<bool, AudioError> {
    let current_pid = get_hogging_pid(device_id)?;
    let self_pid = std::process::id() as i32;

    if current_pid == self_pid {
        // Already holding — do not release on teardown.
        return Ok(false);
    }
    if current_pid != -1 {
        return Err(AudioError::DeviceBusy(
            "Device is in use by another application".to_string(),
        ));
    }

    let addr = AudioObjectPropertyAddress {
        mSelector: kAudioDevicePropertyHogMode,
        mScope: kAudioObjectPropertyScopeGlobal,
        mElement: kAudioObjectPropertyElementMain,
    };
    unsafe {
        let status = AudioObjectSetPropertyData(
            device_id,
            NonNull::from(&addr),
            0,
            ptr::null(),
            mem::size_of::<i32>() as u32,
            NonNull::from(&self_pid).cast(),
        );
        if status != 0 {
            return Err(AudioError::DeviceBusy(format!(
                "Failed to acquire hog mode (OSStatus: {:#x})",
                status
            )));
        }
    }

    let new_pid = get_hogging_pid(device_id)?;
    if new_pid != self_pid {
        return Err(AudioError::DeviceBusy(
            "Failed to acquire exclusive access to device".to_string(),
        ));
    }

    Ok(true)
}

pub(super) fn release_hog_mode(device_id: u32) {
    let addr = AudioObjectPropertyAddress {
        mSelector: kAudioDevicePropertyHogMode,
        mScope: kAudioObjectPropertyScopeGlobal,
        mElement: kAudioObjectPropertyElementMain,
    };
    let pid: i32 = -1;
    unsafe {
        let status = AudioObjectSetPropertyData(
            device_id,
            NonNull::from(&addr),
            0,
            ptr::null(),
            mem::size_of::<i32>() as u32,
            NonNull::from(&pid).cast(),
        );
        if status != 0 {
            log::warn!("coreaudio: release_hog_mode failed: {:#x}", status);
        }
    }
}

pub(super) fn get_hogging_pid(device_id: u32) -> Result<i32, AudioError> {
    let addr = AudioObjectPropertyAddress {
        mSelector: kAudioDevicePropertyHogMode,
        mScope: kAudioObjectPropertyScopeGlobal,
        mElement: kAudioObjectPropertyElementMain,
    };
    let mut pid: i32 = -1;
    let mut data_size = mem::size_of::<i32>() as u32;
    unsafe {
        let status = AudioObjectGetPropertyData(
            device_id,
            NonNull::from(&addr),
            0,
            ptr::null(),
            NonNull::from(&mut data_size),
            NonNull::from(&mut pid).cast(),
        );
        if status != 0 {
            return Err(AudioError::DeviceBusy(format!(
                "Failed to check hog mode: {:#x}",
                status
            )));
        }
    }
    Ok(pid)
}
