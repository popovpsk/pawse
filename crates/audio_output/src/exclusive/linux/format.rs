use alsa::ValueOr;
use alsa::pcm::{Access, Format, HwParams, PCM};
use audio_common::AudioError;

use crate::cpal_stream::OutputConfig;

/// The sample format the device accepted. The pipeline always produces f32, so
/// the render thread converts to whichever of these the hardware negotiated.
#[derive(Clone, Copy)]
pub(super) enum FmtKind {
    F32,
    S32,
    S16,
}

pub(super) struct DeviceFormat {
    pub(super) kind: FmtKind,
    pub(super) period_frames: usize,
}

/// Configures the PCM for direct exclusive playback at the source rate/channels,
/// preferring f32, then S32LE, then S16LE.
///
/// Requires the exact source sample rate (bit-perfect): if the device can't hit
/// it, returns `UnsupportedFormat` so the caller falls back to shared mode.
pub(super) fn configure(pcm: &PCM, config: &OutputConfig) -> Result<DeviceFormat, AudioError> {
    let hwp =
        HwParams::any(pcm).map_err(|e| AudioError::Output(format!("HwParams::any: {}", e)))?;

    hwp.set_access(Access::RWInterleaved)
        .map_err(|e| AudioError::Output(format!("set_access: {}", e)))?;
    hwp.set_channels(config.channels as u32)
        .map_err(|e| AudioError::UnsupportedFormat(format!("set_channels: {}", e)))?;
    hwp.set_rate(config.sample_rate, ValueOr::Nearest)
        .map_err(|e| AudioError::UnsupportedFormat(format!("set_rate: {}", e)))?;

    // Probe with `test_format` (which never narrows the param space) and commit
    // exactly one with `set_format`, so a rejected candidate can't leave the
    // params in a refined state for the next probe.
    let (format, kind) = [
        (Format::FloatLE, FmtKind::F32),
        (Format::S32LE, FmtKind::S32),
        (Format::S16LE, FmtKind::S16),
    ]
    .into_iter()
    .find(|(f, _)| hwp.test_format(*f).is_ok())
    .ok_or_else(|| {
        AudioError::UnsupportedFormat("Device accepts none of f32 / S32LE / S16LE".to_string())
    })?;
    hwp.set_format(format)
        .map_err(|e| AudioError::Output(format!("set_format: {}", e)))?;

    hwp.set_period_size_near(1024, ValueOr::Nearest)
        .map_err(|e| AudioError::Output(format!("set_period_size_near: {}", e)))?;
    hwp.set_buffer_size_near(4096)
        .map_err(|e| AudioError::Output(format!("set_buffer_size_near: {}", e)))?;

    pcm.hw_params(&hwp)
        .map_err(|e| AudioError::Output(format!("hw_params commit: {}", e)))?;

    let actual_rate = hwp
        .get_rate()
        .map_err(|e| AudioError::Output(format!("get_rate: {}", e)))?;
    if actual_rate != config.sample_rate {
        return Err(AudioError::UnsupportedFormat(format!(
            "Device negotiated {} Hz but source is {} Hz — not bit-perfect",
            actual_rate, config.sample_rate
        )));
    }

    let period_frames = hwp
        .get_period_size()
        .map_err(|e| AudioError::Output(format!("get_period_size: {}", e)))?
        .max(1) as usize;

    pcm.prepare()
        .map_err(|e| AudioError::Output(format!("pcm.prepare: {}", e)))?;

    Ok(DeviceFormat {
        kind,
        period_frames,
    })
}
