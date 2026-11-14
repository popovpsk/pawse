use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use audio_common::{
    AudioBatch, AudioError, AudioSamples, AudioSource, ChannelCount, Metadata, ReplayGain,
    StreamParams,
};
use rstest::rstest;

use crate::measure::spread_order;
use crate::{GainStage, Meter, Span, Switch, factor, full, quick};

fn sine(sample_rate: u32, channels: usize, amplitude: f32, seconds: f32) -> Vec<f32> {
    let frames = (sample_rate as f32 * seconds) as usize;
    let mut out = Vec::with_capacity(frames * channels);
    for i in 0..frames {
        let t = i as f32 / sample_rate as f32;
        let value = amplitude * (2.0 * std::f32::consts::PI * 997.0 * t).sin();
        out.extend(std::iter::repeat_n(value, channels));
    }
    out
}

fn measure(sample_rate: u32, channels: usize, samples: &[f32]) -> Option<f32> {
    let mut meter = Meter::new(sample_rate, channels);
    meter.push(samples);
    meter.finish().map(|m| m.lufs)
}

#[rstest]
#[case::stereo_48k(48_000, 2)]
#[case::stereo_44k(44_100, 2)]
#[case::stereo_96k(96_000, 2)]
#[case::mono_counts_as_both_speakers(48_000, 1)]
fn a_sine_measures_at_its_level(#[case] sample_rate: u32, #[case] channels: usize) {
    let lufs = measure(
        sample_rate,
        channels,
        &sine(sample_rate, channels, 0.1, 10.0),
    )
    .unwrap();
    assert!((lufs + 20.0).abs() < 0.1, "{lufs}");
}

#[test]
fn the_relative_gate_drops_a_much_quieter_part() {
    let mut samples = sine(48_000, 2, 0.1, 10.0);
    samples.extend(sine(48_000, 2, 0.001, 10.0));
    let lufs = measure(48_000, 2, &samples).unwrap();
    assert!((lufs + 20.0).abs() < 0.2, "{lufs}");
}

#[test]
fn silence_has_no_loudness() {
    assert_eq!(measure(48_000, 2, &vec![0.0; 48_000 * 2 * 5]), None);
}

#[test]
fn shorter_than_one_block_has_no_loudness() {
    assert_eq!(measure(48_000, 2, &sine(48_000, 2, 0.1, 0.3)), None);
}

#[test]
fn the_peak_is_the_largest_sample() {
    let mut meter = Meter::new(48_000, 2);
    meter.push(&sine(48_000, 2, 0.25, 2.0));
    let peak = meter.finish().unwrap().peak;
    assert!((peak - 0.25).abs() < 1e-3, "{peak}");
}

#[test]
fn a_window_warms_its_filters_up_before_counting() {
    let mut meter = Meter::new(48_000, 2);
    meter.start_window();
    meter.push(&sine(48_000, 2, 0.1, 0.5));
    assert_eq!(meter.blocks(), 1);
}

fn gain(gain_db: f32, peak: Option<f32>) -> Option<ReplayGain> {
    Some(ReplayGain { gain_db, peak })
}

fn db(factor: f32) -> f32 {
    20.0 * factor.log10()
}

#[rstest]
#[case::cut(gain(-6.0, Some(1.0)), -6.0)]
#[case::cut_without_peak(gain(-6.0, None), -6.0)]
#[case::boost_with_room(gain(4.0, Some(0.25)), 4.0)]
#[case::boost_keeps_3_db_under_full_scale(gain(12.0, Some(0.25)), 20.0 * 0.25f32.log10().abs() - 3.0)]
#[case::boost_without_peak_is_dropped(gain(6.0, None), 0.0)]
#[case::boost_with_a_loud_peak_is_dropped(gain(6.0, Some(0.9)), 0.0)]
#[case::hostile_cut_is_clamped(gain(-80.0, None), -30.0)]
#[case::none(None, 0.0)]
fn factor_follows_the_gain_and_the_ceiling(
    #[case] gain: Option<ReplayGain>,
    #[case] expected_db: f32,
) {
    assert!(
        (db(factor(gain)) - expected_db).abs() < 0.01,
        "{}",
        db(factor(gain))
    );
}

#[test]
fn a_tiny_or_broken_gain_is_exactly_unity() {
    assert_eq!(factor(gain(0.001, None)), 1.0);
    assert_eq!(factor(gain(f32::NAN, None)), 1.0);
}

fn s16(frames: usize) -> AudioSamples {
    AudioSamples::S16(vec![16_384; frames * 2])
}

#[test]
fn unity_leaves_the_batch_untouched() {
    let mut stage = GainStage::new(Arc::new(Switch::new(true)));
    stage.set_track(1.0, false);
    let mut batch = s16(64);
    stage.process(&mut batch, 2);
    assert!(matches!(batch, AudioSamples::S16(_)));
    assert!(!stage.is_active());
}

#[test]
fn a_new_track_starts_at_its_gain() {
    let mut stage = GainStage::new(Arc::new(Switch::new(true)));
    stage.set_track(0.5, false);
    let mut batch = s16(64);
    stage.process(&mut batch, 2);
    let AudioSamples::F32(data) = batch else {
        panic!("not converted");
    };
    assert!(data.iter().all(|&s| (s - 0.25).abs() < 1e-6));
    assert!(stage.is_active());
}

#[test]
fn a_gapless_track_ramps_from_the_previous_gain() {
    let mut stage = GainStage::new(Arc::new(Switch::new(true)));
    stage.set_track(0.5, false);
    stage.set_track(1.0, true);
    let mut batch = AudioSamples::F32(vec![1.0; 200]);
    stage.process(&mut batch, 2);
    let AudioSamples::F32(data) = batch else {
        unreachable!()
    };
    assert!(data[0] > 0.5 && data[0] < 0.52);
    assert!(data.windows(2).all(|pair| pair[1] >= pair[0]));
    assert_eq!(*data.last().unwrap(), 1.0);
    assert!(!stage.is_active());
}

#[test]
fn turning_the_switch_off_ramps_back_to_unity() {
    let switch = Arc::new(Switch::new(true));
    let mut stage = GainStage::new(switch.clone());
    stage.set_track(0.5, false);
    let mut first = AudioSamples::F32(vec![1.0; 20]);
    stage.process(&mut first, 2);
    switch.set(false);
    let mut second = AudioSamples::F32(vec![1.0; 20]);
    stage.process(&mut second, 2);
    let AudioSamples::F32(data) = second else {
        unreachable!()
    };
    assert!(data[0] < 0.6);
    assert_eq!(*data.last().unwrap(), 1.0);
    let mut third = s16(8);
    stage.process(&mut third, 2);
    assert!(matches!(third, AudioSamples::S16(_)));
}

#[test]
fn spread_order_visits_every_window_once_starting_wide() {
    let order = spread_order(24);
    let mut sorted = order.clone();
    sorted.sort_unstable();
    assert_eq!(sorted, (0..24).collect::<Vec<_>>());
    assert_eq!(&order[..2], &[0, 16]);
    assert_eq!(spread_order(1), vec![0]);
}

struct Synth {
    sample_rate: u32,
    total: u64,
    position: u64,
    loud_from: u64,
    loud_to: u64,
    rate_changes_at: u64,
    lands_early: u64,
}

impl Synth {
    fn new(seconds: u64) -> Self {
        Self {
            sample_rate: 8_000,
            total: seconds * 8_000,
            position: 0,
            loud_from: 0,
            loud_to: 0,
            rate_changes_at: u64::MAX,
            lands_early: 0,
        }
    }

    fn amplitude(&self, frame: u64) -> f32 {
        if (self.loud_from..self.loud_to).contains(&frame) {
            0.4
        } else {
            0.05 + 0.04 * ((frame / 8_000) % 7) as f32 / 7.0
        }
    }
}

impl AudioSource for Synth {
    fn params(&self) -> StreamParams {
        StreamParams {
            sample_rate: self.sample_rate,
            channels: ChannelCount::Stereo,
            bit_depth: 32,
            dsd_rate: None,
        }
    }

    fn next_buffer(&mut self) -> Result<Option<AudioBatch>, AudioError> {
        if self.position >= self.total {
            return Ok(None);
        }
        let end = (self.position + 1024).min(self.total);
        let mut data = Vec::with_capacity(((end - self.position) * 2) as usize);
        for frame in self.position..end {
            let t = frame as f32 / self.sample_rate as f32;
            let value = self.amplitude(frame) * (2.0 * std::f32::consts::PI * 440.0 * t).sin();
            data.push(value);
            data.push(value);
        }
        let sample_rate = if self.position >= self.rate_changes_at {
            self.sample_rate * 2
        } else {
            self.sample_rate
        };
        self.position = end;
        Ok(Some(AudioBatch {
            data: AudioSamples::F32(data),
            metadata: Metadata {
                sample_rate,
                channels: ChannelCount::Stereo,
                bit_depth: 32,
            },
        }))
    }

    fn seek(&mut self, position: f32) -> Result<Duration, AudioError> {
        let target = (position as f64 * self.total as f64) as u64;
        self.position = target.saturating_sub(self.lands_early);
        Ok(Duration::from_secs_f64(
            self.position as f64 / self.sample_rate as f64,
        ))
    }

    fn duration(&self) -> Option<Duration> {
        Some(Duration::from_secs_f64(
            self.total as f64 / self.sample_rate as f64,
        ))
    }
}

#[test]
fn quick_lands_close_to_full_on_a_long_track() {
    let whole = full(&mut Synth::new(600), Span::default(), &|| false).unwrap();
    let sampled = quick(&mut Synth::new(600), Span::default(), None).unwrap();
    assert!(whole.full);
    assert!(!sampled.full);
    let difference = (whole.measurement.lufs - sampled.measurement.lufs).abs();
    assert!(difference < 0.5, "{difference}");
}

#[test]
fn quick_measures_a_short_track_whole() {
    let estimate = quick(&mut Synth::new(40), Span::default(), None).unwrap();
    assert!(estimate.full);
}

#[test]
fn a_span_measures_only_its_own_part() {
    let mut source = Synth::new(600);
    source.loud_from = 200 * 8_000;
    source.loud_to = 400 * 8_000;
    let span = Span {
        start: Duration::from_secs(200),
        length: Some(Duration::from_secs(200)),
    };
    let whole = full(&mut source, span, &|| false).unwrap();
    source.position = 0;
    let sampled = quick(&mut source, span, None).unwrap();
    let mut loud = Synth::new(200);
    loud.loud_to = loud.total;
    let level = full(&mut loud, Span::default(), &|| false)
        .unwrap()
        .measurement
        .lufs;
    assert!(
        (whole.measurement.lufs - level).abs() < 0.3,
        "{}",
        whole.measurement.lufs
    );
    assert!(
        (sampled.measurement.lufs - level).abs() < 0.3,
        "{}",
        sampled.measurement.lufs
    );
}

#[test]
fn a_seek_that_lands_early_does_not_meter_the_previous_track() {
    let mut source = Synth::new(600);
    source.loud_from = 0;
    source.loud_to = 200 * 8_000;
    source.lands_early = 3 * 8_000;
    let quiet = Span {
        start: Duration::from_secs(200),
        length: Some(Duration::from_secs(100)),
    };
    let whole = full(&mut source, quiet, &|| false).unwrap();
    assert!(whole.measurement.peak < 0.2, "{}", whole.measurement.peak);
}

#[test]
fn quick_gives_up_when_the_deadline_passed_before_enough_windows() {
    let deadline = Instant::now();
    assert_eq!(
        quick(&mut Synth::new(600), Span::default(), Some(deadline)),
        None
    );
}

#[test]
fn a_format_change_midway_is_not_a_full_measurement() {
    let mut source = Synth::new(60);
    source.rate_changes_at = 30 * 8_000;
    let estimate = full(&mut source, Span::default(), &|| false).unwrap();
    assert!(!estimate.full);
}

#[test]
fn full_stops_when_asked() {
    assert_eq!(full(&mut Synth::new(60), Span::default(), &|| true), None);
}

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap())
        .join("../../fixtures")
        .join(name)
}

#[test]
#[cfg_attr(miri, ignore)]
fn a_decoded_file_measures_quick_close_to_full() {
    let path = fixture("long_44_stereo.flac");
    let mut decoder = audio_decoder::Decoder::open(&path).unwrap();
    let whole = full(&mut decoder, Span::default(), &|| false).unwrap();
    let mut decoder = audio_decoder::Decoder::open(&path).unwrap();
    let sampled = quick(&mut decoder, Span::default(), None).unwrap();
    assert!(whole.full && !sampled.full);
    let difference = (whole.measurement.lufs - sampled.measurement.lufs).abs();
    assert!(difference < 1.0, "{difference}");
}
