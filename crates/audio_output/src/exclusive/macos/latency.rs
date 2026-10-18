use std::mem;
use std::ptr::{self, NonNull};
use std::time::Duration;

use objc2_core_audio::{
    AudioObjectGetPropertyData, AudioObjectGetPropertyDataSize, AudioObjectPropertyAddress,
    kAudioDevicePropertyBufferFrameSize, kAudioDevicePropertyLatency,
    kAudioDevicePropertyNominalSampleRate, kAudioDevicePropertySafetyOffset,
    kAudioDevicePropertyStreams, kAudioObjectPropertyElementMain, kAudioObjectPropertyScopeGlobal,
    kAudioObjectPropertyScopeOutput, kAudioStreamPropertyLatency,
};

use super::hog::get_device_id_by_uid;

fn read<T: Copy + Default>(object: u32, selector: u32, scope: u32) -> Option<T> {
    let addr = AudioObjectPropertyAddress {
        mSelector: selector,
        mScope: scope,
        mElement: kAudioObjectPropertyElementMain,
    };
    let mut value = T::default();
    let mut size = mem::size_of::<T>() as u32;
    let status = unsafe {
        AudioObjectGetPropertyData(
            object,
            NonNull::from(&addr),
            0,
            ptr::null(),
            NonNull::from(&mut size),
            NonNull::from(&mut value).cast(),
        )
    };
    (status == 0 && size as usize == mem::size_of::<T>()).then_some(value)
}

fn first_output_stream(device: u32) -> Option<u32> {
    let addr = AudioObjectPropertyAddress {
        mSelector: kAudioDevicePropertyStreams,
        mScope: kAudioObjectPropertyScopeOutput,
        mElement: kAudioObjectPropertyElementMain,
    };
    let mut size: u32 = 0;
    let status = unsafe {
        AudioObjectGetPropertyDataSize(
            device,
            NonNull::from(&addr),
            0,
            ptr::null(),
            NonNull::from(&mut size),
        )
    };
    if status != 0 || (size as usize) < mem::size_of::<u32>() {
        return None;
    }
    let mut streams = vec![0u32; size as usize / mem::size_of::<u32>()];
    let status = unsafe {
        AudioObjectGetPropertyData(
            device,
            NonNull::from(&addr),
            0,
            ptr::null(),
            NonNull::from(&mut size),
            NonNull::new(streams.as_mut_ptr())?.cast(),
        )
    };
    (status == 0).then(|| streams.first().copied()).flatten()
}

pub(in crate::exclusive) fn output_latency(uid: Option<&str>) -> Option<Duration> {
    let device = get_device_id_by_uid(uid.unwrap_or("")).ok()?;
    let rate: f64 = read(
        device,
        kAudioDevicePropertyNominalSampleRate,
        kAudioObjectPropertyScopeGlobal,
    )?;
    if !(rate.is_finite() && rate > 0.) {
        return None;
    }
    let out = kAudioObjectPropertyScopeOutput;
    let frames = [
        read::<u32>(device, kAudioDevicePropertyLatency, out),
        read::<u32>(device, kAudioDevicePropertySafetyOffset, out),
        read::<u32>(device, kAudioDevicePropertyBufferFrameSize, out),
        first_output_stream(device).and_then(|stream| {
            read::<u32>(
                stream,
                kAudioStreamPropertyLatency,
                kAudioObjectPropertyScopeGlobal,
            )
        }),
    ]
    .into_iter()
    .flatten()
    .map(u64::from)
    .sum::<u64>();
    Some(Duration::from_secs_f64(frames as f64 / rate))
}
