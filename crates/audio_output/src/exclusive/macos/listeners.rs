use std::mem;
use std::os::raw::c_void;
use std::ptr::{self, NonNull};
use std::sync::atomic::Ordering;
use std::sync::Arc;

use objc2_core_audio::{
    kAudioDevicePropertyDeviceIsAlive, kAudioDevicePropertyMute,
    kAudioDevicePropertyVolumeScalar, kAudioObjectPropertyElementMain,
    kAudioObjectPropertyScopeGlobal, kAudioObjectPropertyScopeOutput,
    AudioObjectAddPropertyListener, AudioObjectGetPropertyData, AudioObjectHasProperty,
    AudioObjectID, AudioObjectPropertyAddress, AudioObjectRemovePropertyListener,
};

use super::format::{get_stream_format_addr, read_device_format};
use super::MacosShared;
use crate::exclusive::ExclusiveEvent;

// ----- Format-change listener ------------------------------------------------

unsafe extern "C-unwind" fn fmt_changed_cb(
    object_id: AudioObjectID,
    _num_addrs: u32,
    _addrs: NonNull<AudioObjectPropertyAddress>,
    client_data: *mut c_void,
) -> i32 {
    let shared = unsafe { &*(client_data as *const MacosShared) };
    // Only the atomic is consumed (by `bit_perfect_status` via `DeviceSnapshot`).
    // We don't emit a queued event here: nobody acts on format changes from
    // another process — the SampleRateMismatch indicator is enough.
    if let Ok(asbd) = read_device_format(object_id) {
        shared
            .device_sample_rate
            .store(asbd.mSampleRate as u32, Ordering::Relaxed);
    }
    0
}

/// Registers the format-change listener on `device_id`.
///
/// Leaks one Arc refcount into the registration (recovered by `unregister_format_listener`).
pub(super) fn register_format_listener(device_id: u32, shared: Arc<MacosShared>) -> usize {
    let raw = Arc::into_raw(shared) as usize;
    let addr = get_stream_format_addr();
    unsafe {
        let status = AudioObjectAddPropertyListener(
            device_id,
            NonNull::from(&addr),
            Some(fmt_changed_cb),
            raw as *mut c_void,
        );
        if status != 0 {
            eprintln!(
                "coreaudio: AudioObjectAddPropertyListener (stream format): {:#x}",
                status
            );
        }
    }
    raw
}

/// Unregisters the format-change listener and recovers the Arc refcount.
pub(super) fn unregister_format_listener(device_id: u32, raw: usize) {
    let addr = get_stream_format_addr();
    unsafe {
        let status = AudioObjectRemovePropertyListener(
            device_id,
            NonNull::from(&addr),
            Some(fmt_changed_cb),
            raw as *mut c_void,
        );
        if status != 0 {
            eprintln!(
                "coreaudio: AudioObjectRemovePropertyListener (stream format): {:#x}",
                status
            );
        }
        drop(Arc::from_raw(raw as *const MacosShared));
    }
}

// ----- Device-is-alive listener ----------------------------------------------

unsafe extern "C-unwind" fn is_alive_cb(
    _object_id: AudioObjectID,
    _num_addrs: u32,
    _addrs: NonNull<AudioObjectPropertyAddress>,
    client_data: *mut c_void,
) -> i32 {
    let shared = unsafe { &*(client_data as *const MacosShared) };
    shared.alive.store(false, Ordering::SeqCst);
    shared.push_event(ExclusiveEvent::DeviceDisconnected);
    0
}

fn is_alive_addr() -> AudioObjectPropertyAddress {
    AudioObjectPropertyAddress {
        mSelector: kAudioDevicePropertyDeviceIsAlive,
        mScope: kAudioObjectPropertyScopeGlobal,
        mElement: kAudioObjectPropertyElementMain,
    }
}

/// Registers the device-is-alive listener. Fires when the device disconnects.
///
/// Leaks one Arc refcount (recovered by `unregister_is_alive_listener`).
pub(super) fn register_is_alive_listener(device_id: u32, shared: Arc<MacosShared>) -> usize {
    let raw = Arc::into_raw(shared) as usize;
    let addr = is_alive_addr();
    unsafe {
        let status = AudioObjectAddPropertyListener(
            device_id,
            NonNull::from(&addr),
            Some(is_alive_cb),
            raw as *mut c_void,
        );
        if status != 0 {
            eprintln!(
                "coreaudio: AudioObjectAddPropertyListener (device is alive): {:#x}",
                status
            );
        }
    }
    raw
}

pub(super) fn unregister_is_alive_listener(device_id: u32, raw: usize) {
    let addr = is_alive_addr();
    unsafe {
        let status = AudioObjectRemovePropertyListener(
            device_id,
            NonNull::from(&addr),
            Some(is_alive_cb),
            raw as *mut c_void,
        );
        if status != 0 {
            eprintln!(
                "coreaudio: AudioObjectRemovePropertyListener (device is alive): {:#x}",
                status
            );
        }
        drop(Arc::from_raw(raw as *const MacosShared));
    }
}

// ----- Property read helpers -------------------------------------------------

fn read_f32_property(device_id: u32, addr: &AudioObjectPropertyAddress) -> Option<f32> {
    let mut value: f32 = 0.0;
    let mut size = mem::size_of::<f32>() as u32;
    let status = unsafe {
        AudioObjectGetPropertyData(
            device_id,
            NonNull::from(addr),
            0,
            ptr::null(),
            NonNull::from(&mut size),
            NonNull::from(&mut value).cast(),
        )
    };
    if status == 0 { Some(value) } else { None }
}

fn read_u32_property(device_id: u32, addr: &AudioObjectPropertyAddress) -> Option<u32> {
    let mut value: u32 = 0;
    let mut size = mem::size_of::<u32>() as u32;
    let status = unsafe {
        AudioObjectGetPropertyData(
            device_id,
            NonNull::from(addr),
            0,
            ptr::null(),
            NonNull::from(&mut size),
            NonNull::from(&mut value).cast(),
        )
    };
    if status == 0 { Some(value) } else { None }
}

// ----- Volume scalar listener ------------------------------------------------

/// Per-registration context allocated on the heap. Each registered element
/// owns one Arc<MacosShared> refcount (via `shared_raw`) and records which
/// property address element it was registered on.
struct VolumeEntry {
    shared_raw: usize,
    channels: u8,
    element: u32,
}

fn vol_addr(element: u32) -> AudioObjectPropertyAddress {
    AudioObjectPropertyAddress {
        mSelector: kAudioDevicePropertyVolumeScalar,
        mScope: kAudioObjectPropertyScopeOutput,
        mElement: element,
    }
}

/// Reads the effective hardware output volume. Tries the main element first;
/// if the device exposes per-channel volume instead, takes the minimum across
/// channels 1..=channels (most-attenuated is the binding constraint).
fn read_hw_volume(device_id: u32, channels: u8) -> f32 {
    let main_addr = vol_addr(kAudioObjectPropertyElementMain);
    if unsafe { AudioObjectHasProperty(device_id, NonNull::from(&main_addr)) }
        && let Some(v) = read_f32_property(device_id, &main_addr) {
            return v;
        }
    let mut min_vol = 1.0f32;
    for ch in 1..=channels {
        let addr = vol_addr(ch as u32);
        if unsafe { AudioObjectHasProperty(device_id, NonNull::from(&addr)) }
            && let Some(v) = read_f32_property(device_id, &addr)
                && v < min_vol {
                    min_vol = v;
                }
    }
    min_vol
}

unsafe extern "C-unwind" fn vol_changed_cb(
    object_id: AudioObjectID,
    _num_addrs: u32,
    _addrs: NonNull<AudioObjectPropertyAddress>,
    client_data: *mut c_void,
) -> i32 {
    let entry = unsafe { &*(client_data as *const VolumeEntry) };
    let shared = unsafe { &*(entry.shared_raw as *const MacosShared) };
    let vol = read_hw_volume(object_id, entry.channels);
    shared.hw_volume.store(vol, Ordering::Relaxed);
    0
}

/// Registers the hardware volume-scalar listener on each property element that
/// exposes volume (main first; per-channel fallback). Returns one raw pointer
/// per successfully registered element; each must be passed to
/// `unregister_volume_listener` to clean up.
pub(super) fn register_volume_listener(
    device_id: u32,
    channels: u8,
    shared: Arc<MacosShared>,
) -> Vec<usize> {
    let init_vol = read_hw_volume(device_id, channels);
    shared.hw_volume.store(init_vol, Ordering::Relaxed);

    let main_addr = vol_addr(kAudioObjectPropertyElementMain);
    let elements: Vec<u32> =
        if unsafe { AudioObjectHasProperty(device_id, NonNull::from(&main_addr)) } {
            vec![kAudioObjectPropertyElementMain]
        } else {
            (1..=channels as u32)
                .filter(|&ch| {
                    let addr = vol_addr(ch);
                    unsafe { AudioObjectHasProperty(device_id, NonNull::from(&addr)) }
                })
                .collect()
        };

    let mut raws = Vec::new();
    for element in elements {
        let addr = vol_addr(element);
        let entry = Box::into_raw(Box::new(VolumeEntry {
            shared_raw: Arc::into_raw(Arc::clone(&shared)) as usize,
            channels,
            element,
        }));
        let status = unsafe {
            AudioObjectAddPropertyListener(
                device_id,
                NonNull::from(&addr),
                Some(vol_changed_cb),
                entry as *mut c_void,
            )
        };
        if status != 0 {
            eprintln!("coreaudio: AudioObjectAddPropertyListener (volume): {:#x}", status);
            let e = unsafe { Box::from_raw(entry) };
            unsafe { drop(Arc::from_raw(e.shared_raw as *const MacosShared)) };
        } else {
            raws.push(entry as usize);
        }
    }
    raws
}

/// Unregisters one volume listener registration and frees its resources.
///
/// SAFETY: `AudioObjectRemovePropertyListener` blocks until any in-flight
/// callback completes (documented behaviour since macOS 10.4 — see the
/// "Listener Procedures" section in Apple's CoreAudio framework reference).
/// That's why it's safe to `Box::from_raw` the entry right after: no callback
/// can be holding a reference to it past the Remove call. We free the entry
/// even if Remove returned an error — leaking the entry would just hide the
/// failure, and the listener registration is logically gone either way.
pub(super) fn unregister_volume_listener(device_id: u32, raw: usize) {
    let entry = unsafe { &*(raw as *const VolumeEntry) };
    let addr = vol_addr(entry.element);
    unsafe {
        let status = AudioObjectRemovePropertyListener(
            device_id,
            NonNull::from(&addr),
            Some(vol_changed_cb),
            raw as *mut c_void,
        );
        if status != 0 {
            eprintln!(
                "coreaudio: AudioObjectRemovePropertyListener (volume): {:#x}",
                status
            );
        }
        let e = Box::from_raw(raw as *mut VolumeEntry);
        drop(Arc::from_raw(e.shared_raw as *const MacosShared));
    }
}

// ----- Mute listener ---------------------------------------------------------

struct MuteEntry {
    shared_raw: usize,
    channels: u8,
    element: u32,
}

fn mute_addr(element: u32) -> AudioObjectPropertyAddress {
    AudioObjectPropertyAddress {
        mSelector: kAudioDevicePropertyMute,
        mScope: kAudioObjectPropertyScopeOutput,
        mElement: element,
    }
}

fn read_hw_muted(device_id: u32, channels: u8) -> bool {
    let main_addr = mute_addr(kAudioObjectPropertyElementMain);
    if unsafe { AudioObjectHasProperty(device_id, NonNull::from(&main_addr)) }
        && let Some(v) = read_u32_property(device_id, &main_addr) {
            return v != 0;
        }
    for ch in 1..=channels {
        let addr = mute_addr(ch as u32);
        if unsafe { AudioObjectHasProperty(device_id, NonNull::from(&addr)) }
            && let Some(v) = read_u32_property(device_id, &addr)
                && v != 0 {
                    return true;
                }
    }
    false
}

unsafe extern "C-unwind" fn mute_changed_cb(
    object_id: AudioObjectID,
    _num_addrs: u32,
    _addrs: NonNull<AudioObjectPropertyAddress>,
    client_data: *mut c_void,
) -> i32 {
    let entry = unsafe { &*(client_data as *const MuteEntry) };
    let shared = unsafe { &*(entry.shared_raw as *const MacosShared) };
    let muted = read_hw_muted(object_id, entry.channels);
    shared.hw_muted.store(muted, Ordering::Relaxed);
    0
}

pub(super) fn register_mute_listener(
    device_id: u32,
    channels: u8,
    shared: Arc<MacosShared>,
) -> Vec<usize> {
    let init_muted = read_hw_muted(device_id, channels);
    shared.hw_muted.store(init_muted, Ordering::Relaxed);

    let main_addr = mute_addr(kAudioObjectPropertyElementMain);
    let elements: Vec<u32> =
        if unsafe { AudioObjectHasProperty(device_id, NonNull::from(&main_addr)) } {
            vec![kAudioObjectPropertyElementMain]
        } else {
            (1..=channels as u32)
                .filter(|&ch| {
                    let addr = mute_addr(ch);
                    unsafe { AudioObjectHasProperty(device_id, NonNull::from(&addr)) }
                })
                .collect()
        };

    let mut raws = Vec::new();
    for element in elements {
        let addr = mute_addr(element);
        let entry = Box::into_raw(Box::new(MuteEntry {
            shared_raw: Arc::into_raw(Arc::clone(&shared)) as usize,
            channels,
            element,
        }));
        let status = unsafe {
            AudioObjectAddPropertyListener(
                device_id,
                NonNull::from(&addr),
                Some(mute_changed_cb),
                entry as *mut c_void,
            )
        };
        if status != 0 {
            eprintln!("coreaudio: AudioObjectAddPropertyListener (mute): {:#x}", status);
            let e = unsafe { Box::from_raw(entry) };
            unsafe { drop(Arc::from_raw(e.shared_raw as *const MacosShared)) };
        } else {
            raws.push(entry as usize);
        }
    }
    raws
}

/// SAFETY: see `unregister_volume_listener` for the rationale on why Remove
/// must precede `Box::from_raw` and why we free unconditionally.
pub(super) fn unregister_mute_listener(device_id: u32, raw: usize) {
    let entry = unsafe { &*(raw as *const MuteEntry) };
    let addr = mute_addr(entry.element);
    unsafe {
        let status = AudioObjectRemovePropertyListener(
            device_id,
            NonNull::from(&addr),
            Some(mute_changed_cb),
            raw as *mut c_void,
        );
        if status != 0 {
            eprintln!(
                "coreaudio: AudioObjectRemovePropertyListener (mute): {:#x}",
                status
            );
        }
        let e = Box::from_raw(raw as *mut MuteEntry);
        drop(Arc::from_raw(e.shared_raw as *const MacosShared));
    }
}
