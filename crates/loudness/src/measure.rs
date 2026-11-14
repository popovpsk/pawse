use std::time::{Duration, Instant};

use audio_common::AudioSource;

use crate::meter::{Measurement, Meter};

pub const WINDOWS: usize = 24;
pub const MIN_WINDOWS: usize = 8;
pub const WINDOW: Duration = Duration::from_millis(2500);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Span {
    pub start: Duration,
    pub length: Option<Duration>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Estimate {
    pub measurement: Measurement,
    pub full: bool,
}

pub fn full(source: &mut dyn AudioSource, span: Span, stop: &dyn Fn() -> bool) -> Option<Estimate> {
    let params = source.params();
    let channels = params.channels.to_u8() as usize;
    let mut meter = Meter::new(params.sample_rate, channels);
    let landed = if span.start > Duration::ZERO {
        let file = source.duration()?;
        source.seek(fraction(span.start, file)).ok()?
    } else {
        Duration::ZERO
    };
    let mut early = frames_in(span.start.saturating_sub(landed), params.sample_rate) as usize;
    let limit = span.length.map(|length| {
        frames_in(
            (span.start + length).saturating_sub(landed.max(span.start)),
            params.sample_rate,
        )
    });
    let mut scratch = Vec::new();
    let mut taken = 0u64;
    let mut whole = true;
    loop {
        if stop() {
            return None;
        }
        let batch = match source.next_buffer() {
            Ok(Some(batch)) => batch,
            Ok(None) => break,
            Err(e) => {
                log::debug!("loudness: decoding stopped: {e}");
                return None;
            }
        };
        if !same_format(&meter, batch.metadata.sample_rate, &batch.metadata.channels) {
            whole = false;
            break;
        }
        batch.data.to_f32_into(&mut scratch);
        let audio = skip_early(&scratch, &mut early, channels);
        let remaining = limit.map(|limit| limit.saturating_sub(taken));
        let samples = remaining.map_or(audio.len(), |frames| {
            audio.len().min(frames as usize * channels)
        });
        meter.push(&audio[..samples]);
        taken += (samples / channels) as u64;
        if remaining.is_some_and(|frames| frames as usize * channels <= samples) {
            break;
        }
    }
    meter.finish().map(|measurement| Estimate {
        measurement,
        full: whole,
    })
}

pub fn quick(
    source: &mut dyn AudioSource,
    span: Span,
    deadline: Option<Instant>,
) -> Option<Estimate> {
    let past_deadline = || deadline.is_some_and(|deadline| Instant::now() >= deadline);
    let file = source.duration()?;
    let available = file.saturating_sub(span.start);
    let length = span
        .length
        .map_or(available, |length| length.min(available));
    if length <= WINDOW * WINDOWS as u32 {
        return full(source, span, &past_deadline);
    }

    let params = source.params();
    let channels = params.channels.to_u8() as usize;
    let mut meter = Meter::new(params.sample_rate, channels);
    let window_frames = frames_in(WINDOW, params.sample_rate) as usize;
    let room = length - WINDOW;
    let mut scratch = Vec::new();
    let mut measured = 0;
    for index in spread_order(WINDOWS) {
        if past_deadline() {
            break;
        }
        let offset = span.start + room.mul_f64(index as f64 / (WINDOWS - 1) as f64);
        let Ok(landed) = source.seek(fraction(offset, file)) else {
            continue;
        };
        let mut early = frames_in(offset.saturating_sub(landed), params.sample_rate) as usize;
        meter.start_window();
        let wanted = window_frames + meter.sample_rate() as usize / 10;
        let mut taken = 0;
        while taken < wanted {
            let Ok(Some(batch)) = source.next_buffer() else {
                break;
            };
            if !same_format(&meter, batch.metadata.sample_rate, &batch.metadata.channels) {
                break;
            }
            batch.data.to_f32_into(&mut scratch);
            let audio = skip_early(&scratch, &mut early, channels);
            let samples = audio.len().min((wanted - taken) * channels);
            meter.push(&audio[..samples]);
            taken += samples / channels;
        }
        if taken > 0 {
            measured += 1;
        }
    }
    if measured < MIN_WINDOWS {
        return None;
    }
    meter.finish().map(|measurement| Estimate {
        measurement,
        full: false,
    })
}

fn skip_early<'a>(samples: &'a [f32], early: &mut usize, channels: usize) -> &'a [f32] {
    let skipped = samples.len().min(*early * channels);
    *early -= skipped / channels;
    &samples[skipped..]
}

fn same_format(meter: &Meter, sample_rate: u32, channels: &audio_common::ChannelCount) -> bool {
    sample_rate == meter.sample_rate() && channels.to_u8() as usize == meter.channels()
}

fn fraction(position: Duration, file: Duration) -> f32 {
    if file.is_zero() {
        return 0.0;
    }
    (position.as_secs_f64() / file.as_secs_f64()).clamp(0.0, 1.0) as f32
}

fn frames_in(duration: Duration, sample_rate: u32) -> u64 {
    (duration.as_secs_f64() * f64::from(sample_rate)).round() as u64
}

pub(crate) fn spread_order(count: usize) -> Vec<usize> {
    if count <= 1 {
        return (0..count).collect();
    }
    let bits = usize::BITS - count.saturating_sub(1).leading_zeros();
    (0..1usize << bits)
        .map(|i| i.reverse_bits() >> (usize::BITS - bits))
        .filter(|&i| i < count)
        .collect()
}
