use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::thread::JoinHandle;
use std::time::Duration;

use atomic_float::AtomicF32;

use crate::device::{PulseSink, sink_status};

const TICK_MS: u64 = 100;
const TICKS_PER_POLL: u32 = 10;
const SINK_POLL_EVERY: u32 = 2;

/// Device-side state of the current sink, refreshed off the writer thread and
/// read lock-free by `device_snapshot`.
pub(super) struct StatusShared {
    pub(super) device_sample_rate: AtomicU32,
    pub(super) hw_volume: AtomicF32,
    pub(super) hw_muted: AtomicBool,
    pub(super) running: AtomicBool,
}

impl StatusShared {
    pub(super) fn new() -> Self {
        Self {
            device_sample_rate: AtomicU32::new(0),
            hw_volume: AtomicF32::new(1.0),
            hw_muted: AtomicBool::new(false),
            running: AtomicBool::new(true),
        }
    }
}

pub(super) struct HwParams {
    pub(super) rate: u32,
    pub(super) format: String,
}

pub(super) fn spawn(
    shared: Arc<StatusShared>,
    node: String,
    source_rate: u32,
) -> Option<JoinHandle<()>> {
    match std::thread::Builder::new()
        .name("pw-status".into())
        .spawn(move || poll_loop(&shared, &node, source_rate))
    {
        Ok(handle) => Some(handle),
        Err(e) => {
            log::warn!("native rate: status thread not started: {}", e);
            None
        }
    }
}

fn poll_loop(shared: &StatusShared, node: &str, source_rate: u32) {
    let mut sink: Option<PulseSink> = None;
    let mut tick: u32 = 0;
    let mut mismatch_logged = false;

    while shared.running.load(Ordering::Relaxed) {
        if tick.is_multiple_of(SINK_POLL_EVERY)
            && let Some(status) = sink_status(node)
        {
            shared.hw_volume.store(status.volume, Ordering::Relaxed);
            shared.hw_muted.store(status.muted, Ordering::Relaxed);
            sink = Some(status);
        }

        let hw = sink.as_ref().and_then(read_hw_params);
        let rate = hw
            .as_ref()
            .map(|p| p.rate)
            .or_else(|| sink.as_ref().and_then(|s| s.rate))
            .unwrap_or(0);
        shared.device_sample_rate.store(rate, Ordering::Relaxed);

        if rate != 0 && rate != source_rate {
            if !mismatch_logged {
                log::warn!(
                    "native rate: sink '{}' runs at {} Hz (format {}) for a {} Hz source; pipewire settings: {}",
                    node,
                    rate,
                    hw.as_ref().map_or("unknown", |p| p.format.as_str()),
                    source_rate,
                    settings_dump()
                );
                mismatch_logged = true;
            }
        } else {
            mismatch_logged = false;
        }

        tick = tick.wrapping_add(1);
        for _ in 0..TICKS_PER_POLL {
            if !shared.running.load(Ordering::Relaxed) {
                return;
            }
            std::thread::sleep(Duration::from_millis(TICK_MS));
        }
    }
}

fn read_hw_params(sink: &PulseSink) -> Option<HwParams> {
    let card = sink.card?;
    let device = sink.device.unwrap_or(0);
    let text = std::fs::read_to_string(format!(
        "/proc/asound/card{card}/pcm{device}p/sub0/hw_params"
    ))
    .ok()?;
    parse_hw_params(&text)
}

/// Parses what the kernel reports for an open playback substream. Returns
/// `None` for a closed substream (the file then holds just `closed`).
pub(super) fn parse_hw_params(text: &str) -> Option<HwParams> {
    let mut rate = None;
    let mut format = None;

    for line in text.lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        match key.trim() {
            "rate" => rate = value.split_whitespace().next().and_then(|v| v.parse().ok()),
            "format" => format = Some(value.trim().to_string()),
            _ => {}
        }
    }

    Some(HwParams {
        rate: rate?,
        format: format.unwrap_or_else(|| "unknown".to_string()),
    })
}

fn settings_dump() -> String {
    let Ok(output) = std::process::Command::new("pw-metadata")
        .args(["-n", "settings"])
        .output()
    else {
        return "unavailable".to_string();
    };
    if !output.status.success() {
        return "unavailable".to_string();
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|line| line.contains("clock."))
        .map(str::trim)
        .collect::<Vec<_>>()
        .join("; ")
}

#[cfg(test)]
mod tests {
    use super::*;

    const OPEN: &str = "access: MMAP_INTERLEAVED\n\
                        format: FLOAT_LE\n\
                        subformat: STD\n\
                        channels: 2\n\
                        rate: 44100 (44100/1)\n\
                        period_size: 1024\n\
                        buffer_size: 4096\n";

    #[test]
    fn open_substream_yields_rate_and_format() {
        let parsed = parse_hw_params(OPEN).expect("an open substream must parse");
        assert_eq!(parsed.rate, 44100);
        assert_eq!(parsed.format, "FLOAT_LE");
    }

    #[test]
    fn closed_substream_yields_nothing() {
        assert!(parse_hw_params("closed\n").is_none());
        assert!(parse_hw_params("").is_none());
    }

    #[test]
    fn missing_format_still_yields_the_rate() {
        let parsed = parse_hw_params("rate: 96000 (96000/1)\n").expect("rate alone is enough");
        assert_eq!(parsed.rate, 96000);
        assert_eq!(parsed.format, "unknown");
    }
}
