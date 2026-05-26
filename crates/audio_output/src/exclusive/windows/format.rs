use audio_common::AudioError;
use windows::Win32::Media::Audio::{
    AUDCLNT_E_BUFFER_SIZE_NOT_ALIGNED, AUDCLNT_SHAREMODE_EXCLUSIVE,
    AUDCLNT_STREAMFLAGS_EVENTCALLBACK, IAudioClient, IMMDevice, WAVEFORMATEX, WAVEFORMATEXTENSIBLE,
    WAVEFORMATEXTENSIBLE_0,
};
use windows::Win32::System::Com::CLSCTX_ALL;
use windows::core::GUID;

use crate::cpal_stream::OutputConfig;

// WAVE_FORMAT_EXTENSIBLE tag and the IEEE-float KSDATAFORMAT subtype GUID.
const WAVE_FORMAT_EXTENSIBLE: u16 = 0xFFFE;
// {00000003-0000-0010-8000-00AA00389B71}
const KSDATAFORMAT_SUBTYPE_IEEE_FLOAT: GUID =
    GUID::from_u128(0x00000003_0000_0010_8000_00aa00389b71);
// 100-nanosecond units per second (REFERENCE_TIME resolution).
const HNS_PER_SEC: f64 = 10_000_000.0;

/// Result of a successful exclusive-format negotiation + client init.
pub(super) struct InitializedClient {
    pub(super) client: IAudioClient,
    pub(super) buffer_frames: u32,
    pub(super) sample_rate: u32,
}

fn channel_mask(channels: u16) -> u32 {
    match channels {
        1 => 0x4, // SPEAKER_FRONT_CENTER
        2 => 0x3, // FRONT_LEFT | FRONT_RIGHT
        _ => 0,   // let the driver decide
    }
}

fn build_wfx(config: &OutputConfig) -> WAVEFORMATEXTENSIBLE {
    let channels = config.channels as u16;
    let bits = 32u16; // we always push f32 into the ring buffer
    let block_align = channels * (bits / 8);
    let avg_bytes = config.sample_rate * block_align as u32;
    let ext_size =
        (std::mem::size_of::<WAVEFORMATEXTENSIBLE>() - std::mem::size_of::<WAVEFORMATEX>()) as u16;

    WAVEFORMATEXTENSIBLE {
        Format: WAVEFORMATEX {
            wFormatTag: WAVE_FORMAT_EXTENSIBLE,
            nChannels: channels,
            nSamplesPerSec: config.sample_rate,
            nAvgBytesPerSec: avg_bytes,
            nBlockAlign: block_align,
            wBitsPerSample: bits,
            cbSize: ext_size,
        },
        Samples: WAVEFORMATEXTENSIBLE_0 {
            wValidBitsPerSample: bits,
        },
        dwChannelMask: channel_mask(channels),
        SubFormat: KSDATAFORMAT_SUBTYPE_IEEE_FLOAT,
    }
}

/// Negotiates a packed f32 exclusive format at the source sample rate and
/// initialises an event-driven exclusive `IAudioClient`.
///
/// Returns `UnsupportedFormat` if the device won't accept f32 at the requested
/// rate — the caller falls back to shared mode (cpal resampling), matching the
/// macOS contract in `apply_format`.
pub(super) fn negotiate_and_init(
    device: &IMMDevice,
    config: &OutputConfig,
) -> Result<InitializedClient, AudioError> {
    let wfx = build_wfx(config);
    let p_wfx = &wfx.Format as *const WAVEFORMATEX;

    let client: IAudioClient = unsafe { device.Activate(CLSCTX_ALL, None) }
        .map_err(|e| AudioError::Output(format!("IMMDevice::Activate(IAudioClient): {}", e)))?;

    let supported = unsafe { client.IsFormatSupported(AUDCLNT_SHAREMODE_EXCLUSIVE, p_wfx, None) };
    if supported.is_err() {
        return Err(AudioError::UnsupportedFormat(format!(
            "Device does not support exclusive f32 @ {} Hz / {} ch (hr {:#x})",
            config.sample_rate, config.channels, supported.0
        )));
    }

    let mut min_period: i64 = 0;
    unsafe { client.GetDevicePeriod(None, Some(&mut min_period)) }
        .map_err(|e| AudioError::Output(format!("GetDevicePeriod: {}", e)))?;

    let init = unsafe {
        client.Initialize(
            AUDCLNT_SHAREMODE_EXCLUSIVE,
            AUDCLNT_STREAMFLAGS_EVENTCALLBACK,
            min_period,
            min_period,
            p_wfx,
            None,
        )
    };

    let client = match init {
        Ok(()) => client,
        Err(e) if e.code() == AUDCLNT_E_BUFFER_SIZE_NOT_ALIGNED => {
            // The driver rejected our period; re-query the aligned buffer size,
            // recompute the period, and re-init on a fresh client (a client that
            // failed Initialize cannot be reused).
            let frames = unsafe { client.GetBufferSize() }
                .map_err(|e| AudioError::Output(format!("GetBufferSize (realign): {}", e)))?;
            let aligned_period =
                (HNS_PER_SEC / config.sample_rate as f64 * frames as f64 + 0.5) as i64;
            let client2: IAudioClient = unsafe { device.Activate(CLSCTX_ALL, None) }
                .map_err(|e| AudioError::Output(format!("Re-Activate (realign): {}", e)))?;
            unsafe {
                client2.Initialize(
                    AUDCLNT_SHAREMODE_EXCLUSIVE,
                    AUDCLNT_STREAMFLAGS_EVENTCALLBACK,
                    aligned_period,
                    aligned_period,
                    p_wfx,
                    None,
                )
            }
            .map_err(|e| AudioError::Output(format!("Initialize (realign): {}", e)))?;
            client2
        }
        Err(e) => {
            return Err(AudioError::Output(format!(
                "IAudioClient::Initialize (exclusive): {}",
                e
            )));
        }
    };

    let buffer_frames = unsafe { client.GetBufferSize() }
        .map_err(|e| AudioError::Output(format!("GetBufferSize: {}", e)))?;

    Ok(InitializedClient {
        client,
        buffer_frames,
        sample_rate: config.sample_rate,
    })
}
