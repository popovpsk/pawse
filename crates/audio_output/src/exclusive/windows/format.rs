use audio_common::AudioError;
use windows::Win32::Media::Audio::{
    AUDCLNT_E_BUFFER_SIZE_NOT_ALIGNED, AUDCLNT_SHAREMODE_EXCLUSIVE,
    AUDCLNT_STREAMFLAGS_EVENTCALLBACK, IAudioClient, IMMDevice, WAVEFORMATEX, WAVEFORMATEXTENSIBLE,
    WAVEFORMATEXTENSIBLE_0,
};
use windows::Win32::System::Com::CLSCTX_ALL;
use windows::core::GUID;

use crate::cpal_stream::OutputConfig;

// WAVE_FORMAT_EXTENSIBLE tag and the KSDATAFORMAT subtype GUIDs.
const WAVE_FORMAT_EXTENSIBLE: u16 = 0xFFFE;
// {00000003-0000-0010-8000-00AA00389B71}
const KSDATAFORMAT_SUBTYPE_IEEE_FLOAT: GUID =
    GUID::from_u128(0x00000003_0000_0010_8000_00aa00389b71);
// {00000001-0000-0010-8000-00AA00389B71}
const KSDATAFORMAT_SUBTYPE_PCM: GUID = GUID::from_u128(0x00000001_0000_0010_8000_00aa00389b71);
// 100-nanosecond units per second (REFERENCE_TIME resolution).
const HNS_PER_SEC: f64 = 10_000_000.0;

/// Sample encoding the device accepted in exclusive mode. The render thread
/// always produces f32 internally and converts to this on the way out.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum SampleFmt {
    /// 32-bit IEEE float — bit-perfect, the preferred format.
    F32,
    /// 32-bit signed PCM.
    S32,
    /// 24 valid bits left-justified in a 32-bit container.
    S24In32,
    /// 16-bit signed PCM.
    S16,
}

impl SampleFmt {
    fn bits(self) -> u16 {
        match self {
            SampleFmt::S16 => 16,
            _ => 32,
        }
    }

    fn valid_bits(self) -> u16 {
        match self {
            SampleFmt::S16 => 16,
            SampleFmt::S24In32 => 24,
            _ => 32,
        }
    }

    fn subformat(self) -> GUID {
        match self {
            SampleFmt::F32 => KSDATAFORMAT_SUBTYPE_IEEE_FLOAT,
            _ => KSDATAFORMAT_SUBTYPE_PCM,
        }
    }

    fn label(self) -> &'static str {
        match self {
            SampleFmt::F32 => "f32",
            SampleFmt::S32 => "s32",
            SampleFmt::S24In32 => "s24/32",
            SampleFmt::S16 => "s16",
        }
    }
}

/// Result of a successful exclusive-format negotiation + client init.
pub(super) struct InitializedClient {
    pub(super) client: IAudioClient,
    pub(super) buffer_frames: u32,
    pub(super) sample_rate: u32,
    pub(super) sample_fmt: SampleFmt,
}

fn channel_mask(channels: u16) -> u32 {
    match channels {
        1 => 0x4, // SPEAKER_FRONT_CENTER
        2 => 0x3, // FRONT_LEFT | FRONT_RIGHT
        _ => 0,   // let the driver decide
    }
}

fn build_wfx(config: &OutputConfig, fmt: SampleFmt) -> WAVEFORMATEXTENSIBLE {
    let channels = config.channels as u16;
    let bits = fmt.bits();
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
            wValidBitsPerSample: fmt.valid_bits(),
        },
        dwChannelMask: channel_mask(channels),
        SubFormat: fmt.subformat(),
    }
}

/// Negotiates an exclusive format at the source sample rate and initialises an
/// event-driven exclusive `IAudioClient`.
///
/// Tries f32 first (bit-perfect), then falls back to integer PCM (32-bit,
/// 24-in-32, 16-bit). Most DACs only advertise integer formats in exclusive
/// mode, so float-only negotiation fails on hardware that is otherwise fully
/// capable. Returns `UnsupportedFormat` only if no candidate is accepted — the
/// caller then falls back to shared mode (cpal resampling).
pub(super) fn negotiate_and_init(
    device: &IMMDevice,
    config: &OutputConfig,
) -> Result<InitializedClient, AudioError> {
    let client: IAudioClient = unsafe { device.Activate(CLSCTX_ALL, None) }
        .map_err(|e| AudioError::Output(format!("IMMDevice::Activate(IAudioClient): {}", e)))?;

    const CANDIDATES: [SampleFmt; 4] = [
        SampleFmt::F32,
        SampleFmt::S32,
        SampleFmt::S24In32,
        SampleFmt::S16,
    ];

    let mut chosen: Option<(WAVEFORMATEXTENSIBLE, SampleFmt)> = None;
    for fmt in CANDIDATES {
        let wfx = build_wfx(config, fmt);
        let hr = unsafe {
            client.IsFormatSupported(
                AUDCLNT_SHAREMODE_EXCLUSIVE,
                &wfx.Format as *const WAVEFORMATEX,
                None,
            )
        };
        if hr.is_ok() {
            chosen = Some((wfx, fmt));
            break;
        }
    }

    let (wfx, sample_fmt) = chosen.ok_or_else(|| {
        AudioError::UnsupportedFormat(format!(
            "Device does not support any exclusive format (f32/s32/s24/s16) @ {} Hz / {} ch",
            config.sample_rate, config.channels
        ))
    })?;
    let p_wfx = &wfx.Format as *const WAVEFORMATEX;

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
                "IAudioClient::Initialize (exclusive {}): {}",
                sample_fmt.label(),
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
        sample_fmt,
    })
}
