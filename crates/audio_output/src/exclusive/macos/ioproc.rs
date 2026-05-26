use std::os::raw::c_void;
use std::ptr::NonNull;
use std::slice;
use std::sync::Arc;

use audio_common::AudioError;
use objc2_core_audio::{
    AudioDeviceCreateIOProcID, AudioDeviceDestroyIOProcID, AudioDeviceIOProcID, AudioDeviceStart,
    AudioDeviceStop, AudioObjectID,
};
use objc2_core_audio_types::{AudioBufferList, AudioTimeStamp};

use crate::exclusive::render::{RenderCtx, fill};

unsafe extern "C-unwind" fn ioproc_callback(
    _device: AudioObjectID,
    _now: NonNull<AudioTimeStamp>,
    _input_data: NonNull<AudioBufferList>,
    _input_time: NonNull<AudioTimeStamp>,
    output_data: NonNull<AudioBufferList>,
    _output_time: NonNull<AudioTimeStamp>,
    client_data: *mut c_void,
) -> i32 {
    let ctx = unsafe { &*(client_data as *const RenderCtx) };
    let buf_list = unsafe { output_data.as_ref() };

    let buf_ptr = buf_list.mBuffers[0].mData as *mut f32;
    let num_bytes = buf_list.mBuffers[0].mDataByteSize as usize;
    let num_samples = num_bytes / std::mem::size_of::<f32>();

    if buf_ptr.is_null() || num_samples == 0 {
        return 0;
    }

    let out = unsafe { slice::from_raw_parts_mut(buf_ptr, num_samples) };
    fill(ctx, out);

    0 // noErr
}

/// Registers the IOProc against the given device.
///
/// The `ctx` Arc has its refcount incremented by 1 (leaked into the registration).
/// You must call `destroy_ioproc` with the returned `ctx_raw` to recover it.
pub(super) fn create_ioproc(
    device_id: u32,
    ctx: Arc<RenderCtx>,
) -> Result<(AudioDeviceIOProcID, usize), AudioError> {
    let ctx_raw = Arc::into_raw(ctx) as usize;
    let mut proc_id: AudioDeviceIOProcID = None;

    let status = unsafe {
        AudioDeviceCreateIOProcID(
            device_id,
            Some(ioproc_callback),
            ctx_raw as *mut c_void,
            NonNull::from(&mut proc_id),
        )
    };

    if status != 0 {
        // Recover leaked refcount before returning error
        unsafe { drop(Arc::from_raw(ctx_raw as *const RenderCtx)) };
        return Err(AudioError::Output(format!(
            "AudioDeviceCreateIOProcID: {:#x}",
            status
        )));
    }

    Ok((proc_id, ctx_raw))
}

/// Starts the IOProc (begins audio delivery).
pub(super) fn start_ioproc(device_id: u32, proc_id: AudioDeviceIOProcID) -> Result<(), AudioError> {
    let status = unsafe { AudioDeviceStart(device_id, proc_id) };
    if status != 0 {
        return Err(AudioError::Output(format!(
            "AudioDeviceStart: {:#x}",
            status
        )));
    }
    Ok(())
}

/// Stops the IOProc (halts audio delivery). Non-fatal if it fails.
pub(super) fn stop_ioproc(device_id: u32, proc_id: AudioDeviceIOProcID) {
    let status = unsafe { AudioDeviceStop(device_id, proc_id) };
    if status != 0 {
        eprintln!("coreaudio: AudioDeviceStop: {:#x}", status);
    }
}

/// Destroys the IOProc and recovers the leaked Arc refcount.
pub(super) fn destroy_ioproc(device_id: u32, proc_id: AudioDeviceIOProcID, ctx_raw: usize) {
    let status = unsafe { AudioDeviceDestroyIOProcID(device_id, proc_id) };
    if status != 0 {
        eprintln!("coreaudio: AudioDeviceDestroyIOProcID: {:#x}", status);
    }
    // Recover the leaked Arc refcount
    unsafe { drop(Arc::from_raw(ctx_raw as *const RenderCtx)) };
}
