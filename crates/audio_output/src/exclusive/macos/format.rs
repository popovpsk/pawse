use std::mem;
use std::ptr::{self, NonNull};
use std::thread;
use std::time::Duration;

use audio_common::AudioError;
use objc2_core_audio::{
    AudioObjectGetPropertyData, AudioObjectPropertyAddress, AudioObjectSetPropertyData,
    kAudioObjectPropertyElementMain,
};
use objc2_core_audio_types::{
    AudioStreamBasicDescription, kAudioFormatFlagsNativeFloatPacked, kAudioFormatLinearPCM,
    kLinearPCMFormatFlagIsPacked, kLinearPCMFormatFlagIsSignedInteger,
};

use super::hog::K_SCOPE_OUTPUT;
use super::sample_rate::get_best_samplerate;
use crate::cpal_stream::OutputConfig;

// kAudioDevicePropertyStreamFormat = 'sfmt'
const K_STREAM_FORMAT: u32 = 0x73666d74;
// kAudioFormatFlagsNativeEndian: on little-endian (all Apple silicon + x86) this is 0.
// Since deadbeef uses kAudioFormatFlagsNativeEndian which expands to 0 on LE, we replicate:
const K_FLAGS_NATIVE_ENDIAN: u32 = 0;

pub(super) fn get_stream_format_addr() -> AudioObjectPropertyAddress {
    AudioObjectPropertyAddress {
        mSelector: K_STREAM_FORMAT,
        mScope: K_SCOPE_OUTPUT,
        mElement: kAudioObjectPropertyElementMain,
    }
}

pub(super) fn read_device_format(
    device_id: u32,
) -> Result<AudioStreamBasicDescription, AudioError> {
    let addr = get_stream_format_addr();
    let mut asbd = AudioStreamBasicDescription {
        mSampleRate: 0.0,
        mFormatID: 0,
        mFormatFlags: 0,
        mBytesPerPacket: 0,
        mFramesPerPacket: 0,
        mBytesPerFrame: 0,
        mChannelsPerFrame: 0,
        mBitsPerChannel: 0,
        mReserved: 0,
    };
    let mut data_size = mem::size_of::<AudioStreamBasicDescription>() as u32;
    unsafe {
        let status = AudioObjectGetPropertyData(
            device_id,
            NonNull::from(&addr),
            0,
            ptr::null(),
            NonNull::from(&mut data_size),
            NonNull::from(&mut asbd).cast(),
        );
        if status != 0 {
            return Err(AudioError::Output(format!(
                "kAudioDevicePropertyStreamFormat read failed: {:#x}",
                status
            )));
        }
    }
    Ok(asbd)
}

fn build_asbd(config: &OutputConfig, channels: u32) -> AudioStreamBasicDescription {
    let bps = 32u32; // We always push F32 into the ring buffer
    let is_float = true;

    let mut sample_rate = config.sample_rate as f64;
    if sample_rate > 192_000.0 {
        sample_rate = 192_000.0; // deadbeef caps at 192 kHz
    }

    let flags = if is_float {
        kAudioFormatFlagsNativeFloatPacked
    } else {
        kLinearPCMFormatFlagIsSignedInteger | kLinearPCMFormatFlagIsPacked | K_FLAGS_NATIVE_ENDIAN
    };

    AudioStreamBasicDescription {
        mSampleRate: sample_rate,
        mFormatID: kAudioFormatLinearPCM,
        mFormatFlags: flags,
        mBytesPerPacket: bps / 8 * channels,
        mFramesPerPacket: 1,
        mBytesPerFrame: bps / 8 * channels,
        mChannelsPerFrame: channels,
        mBitsPerChannel: bps,
        mReserved: 0,
    }
}

fn set_device_format(device_id: u32, asbd: &AudioStreamBasicDescription) -> i32 {
    let addr = get_stream_format_addr();
    unsafe {
        AudioObjectSetPropertyData(
            device_id,
            NonNull::from(&addr),
            0,
            ptr::null(),
            mem::size_of::<AudioStreamBasicDescription>() as u32,
            NonNull::from(asbd).cast(),
        )
    }
}

/// Sets the device sample rate via the nominal sample rate property.
pub(super) fn set_nominal_sample_rate(device_id: u32, rate: f64) -> Result<(), AudioError> {
    // kAudioDevicePropertyNominalSampleRate = 'nsrt'
    const K_NOMINAL_SAMPLE_RATE: u32 = 0x6e737274;
    let addr = AudioObjectPropertyAddress {
        mSelector: K_NOMINAL_SAMPLE_RATE,
        mScope: K_SCOPE_OUTPUT,
        mElement: kAudioObjectPropertyElementMain,
    };
    unsafe {
        let status = AudioObjectSetPropertyData(
            device_id,
            NonNull::from(&addr),
            0,
            ptr::null(),
            mem::size_of::<f64>() as u32,
            NonNull::from(&rate).cast(),
        );
        if status != 0 {
            return Err(AudioError::UnsupportedFormat(format!(
                "Failed to set nominal sample rate to {}: {:#x}",
                rate, status
            )));
        }
    }
    Ok(())
}

/// Applies the best matching format to the device.
///
/// Follows deadbeef's ca_apply_format logic:
/// 1. Score and pick best sample rate from available list (already capped at 192 kHz in build_asbd)
/// 2. Try to set the format as-is
/// 3. If mono fails, retry as stereo
/// 4. If stereo fails, fall back to the device's current format (ignored if same)
///
/// Returns the format that was actually negotiated (read back from the device).
pub(super) fn apply_format(
    device_id: u32,
    config: &OutputConfig,
    available_rates: &[i32],
) -> Result<AudioStreamBasicDescription, AudioError> {
    let channels = config.channels as u32;

    // Pick best sample rate
    let target_rate = if config.sample_rate > 192_000 {
        192_000
    } else {
        config.sample_rate
    };
    let best_rate = if available_rates.is_empty() {
        target_rate
    } else {
        get_best_samplerate(target_rate, available_rates)
    };

    // Build the ASBD with the scored rate
    let mut asbd = build_asbd(config, channels);
    asbd.mSampleRate = best_rate as f64;

    if best_rate != target_rate {
        eprintln!(
            "coreaudio: sample rate adjusted {} → {} Hz (closest device rate)",
            target_rate, best_rate
        );
    }

    let status = set_device_format(device_id, &asbd);

    if status != 0 {
        // Fall back: try to re-apply whatever is currently on the device.
        // This may fail too (e.g. same format), so we ignore the result.
        if let Ok(current) = read_device_format(device_id) {
            let _ = set_device_format(device_id, &current);
        }
    }

    // Read back the actual negotiated format and validate it matches what
    // our IOProc + ring buffer assume: packed native-endian f32, and the same
    // channel count the caller's config says. If the device fell back to an
    // incompatible format (e.g. 16-bit integer) the IOProc would push f32 bytes
    // into a buffer the device interprets differently → noise. If the device
    // accepted only a different channel layout (e.g. only stereo when we asked
    // for mono) the ring buffer would be sized wrong and the source data would
    // interleave incorrectly — also noise.
    //
    // We do NOT auto-promote mono to stereo here, because the caller's
    // OutputConfig (and ring-buffer sizing in `Output::recreate_*`) is already
    // committed to `config.channels`. The cleaner contract is to surface
    // `UnsupportedFormat` and let the caller fall back to shared (which then
    // goes through cpal's resampling/mixing pipeline).
    let actual = read_device_format(device_id)?;
    let is_float = (actual.mFormatFlags & kAudioFormatFlagsNativeFloatPacked)
        == kAudioFormatFlagsNativeFloatPacked;
    if actual.mFormatID != kAudioFormatLinearPCM || !is_float || actual.mBitsPerChannel != 32 {
        return Err(AudioError::UnsupportedFormat(format!(
            "Device only supports format id={:#x} flags={:#x} bits={} — exclusive mode needs packed native f32",
            actual.mFormatID, actual.mFormatFlags, actual.mBitsPerChannel
        )));
    }
    if actual.mChannelsPerFrame != channels {
        return Err(AudioError::UnsupportedFormat(format!(
            "Device negotiated {} channels but source has {} — channel mismatch would produce noise",
            actual.mChannelsPerFrame, channels
        )));
    }
    Ok(actual)
}

/// Sets the nominal sample rate and polls until the stream format reflects the new
/// value (up to ~500 ms). Returns Ok even if the poll times out — best-effort.
pub(super) fn set_and_wait_sample_rate(device_id: u32, rate: f64) -> Result<(), AudioError> {
    set_nominal_sample_rate(device_id, rate)?;
    for _ in 0..50 {
        if let Ok(asbd) = read_device_format(device_id)
            && (asbd.mSampleRate - rate).abs() < 0.5
        {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(10));
    }
    Ok(())
}
