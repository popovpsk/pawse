use std::f64::consts::PI;
use std::sync::Arc;

use rstest::rstest;

use crate::{
    Band, Control, DEFAULT_Q, Kind, ParseError, Processor, Response, parse, preamp_db, write,
};

const RATE: f64 = 48_000.0;

const HD600: &str = "Preamp: -6.3 dB\r
Filter 1: ON LSC Fc 105 Hz Gain 6.5 dB Q 0.70\r
Filter 2: ON PK Fc 125 Hz Gain -2.7 dB Q 0.55\r
Filter 3: ON PK Fc 8445 Hz Gain 3.3 dB Q 1.61\r
Filter 4: ON PK Fc 522 Hz Gain 0.7 dB Q 1.02\r
Filter 5: ON PK Fc 1298 Hz Gain -1.2 dB Q 2.14\r
Filter 6: ON HSC Fc 10000 Hz Gain -3.1 dB Q 0.70\r
Filter 7: ON PK Fc 3158 Hz Gain -1.8 dB Q 3.67\r
Filter 8: ON PK Fc 2166 Hz Gain 0.9 dB Q 3.32\r
Filter 9: ON PK Fc 6639 Hz Gain 2.2 dB Q 5.82\r
Filter 10: ON PK Fc 5433 Hz Gain -1.2 dB Q 5.70\r
";

fn db(bands: &[Band], freq: f64) -> f64 {
    Response::new(bands, RATE).db(freq)
}

fn sine(freq: f64, rate: u32, channels: usize, frames: usize, offset: usize) -> Vec<f32> {
    (0..frames)
        .flat_map(|i| {
            let t = (offset + i) as f64 / f64::from(rate);
            let value = (0.25 * (2.0 * PI * freq * t).sin()) as f32;
            std::iter::repeat_n(value, channels)
        })
        .collect()
}

fn rms(samples: &[f32]) -> f64 {
    let sum: f64 = samples.iter().map(|&s| f64::from(s).powi(2)).sum();
    (sum / samples.len() as f64).sqrt()
}

fn processor(bands: Option<Vec<Band>>) -> (Arc<Control>, Processor) {
    let control = Arc::new(Control::new(bands));
    let processor = Processor::new(control.clone());
    (control, processor)
}

#[rstest]
#[case::bass(60.0, 6.0, 0.7)]
#[case::mid(1000.0, -4.5, 2.0)]
#[case::treble(12_000.0, 3.0, 5.0)]
fn peak_reaches_its_gain_at_the_center(#[case] freq: f64, #[case] gain: f64, #[case] q: f64) {
    let bands = [Band::new(Kind::Peak, freq, gain, q)];
    assert!((db(&bands, freq) - gain).abs() < 0.01);
}

#[test]
fn shelves_reach_their_gain_on_their_side_only() {
    let low = [Band::new(Kind::LowShelf, 105.0, 6.5, 0.7)];
    assert!((db(&low, 15.0) - 6.5).abs() < 0.1);
    assert!(db(&low, 15_000.0).abs() < 0.05);
    let high = [Band::new(Kind::HighShelf, 10_000.0, -3.0, 0.7)];
    assert!((db(&high, 20_000.0) + 3.0).abs() < 0.3);
    assert!(db(&high, 50.0).abs() < 0.05);
}

#[rstest]
#[case::low_pass(Kind::LowPass)]
#[case::high_pass(Kind::HighPass)]
fn butterworth_passes_are_three_db_down_at_the_corner(#[case] kind: Kind) {
    let bands = [Band::new(kind, 2000.0, 9.0, DEFAULT_Q)];
    assert!((db(&bands, 2000.0) + 3.01).abs() < 0.02);
}

#[test]
fn flat_and_disabled_bands_do_nothing() {
    let disabled = Band {
        enabled: false,
        ..Band::new(Kind::Peak, 1000.0, 6.0, 1.0)
    };
    let flat = Band::new(Kind::Peak, 1000.0, 0.0, 1.0);
    assert_eq!(db(&[disabled, flat], 1000.0), 0.0);
    assert_eq!(preamp_db(&[disabled, flat], RATE), 0.0);
}

#[test]
fn preamp_cancels_the_highest_boost() {
    let bands = [
        Band::new(Kind::Peak, 1000.0, 6.0, 4.0),
        Band::new(Kind::Peak, 8000.0, -3.0, 4.0),
    ];
    assert!((preamp_db(&bands, RATE) + 6.0).abs() < 0.05);
    let cuts = [Band::new(Kind::Peak, 1000.0, -6.0, 1.0)];
    assert_eq!(preamp_db(&cuts, RATE), 0.0);
}

#[test]
fn autoeq_preamp_matches_the_file() {
    let bands = parse(HD600).unwrap();
    assert!((preamp_db(&bands, RATE) + 6.3).abs() < 0.15);
}

#[test]
fn garbage_values_are_clamped_not_propagated() {
    let bands = [
        Band::new(Kind::Peak, f64::NAN, 6.0, 0.0),
        Band::new(Kind::LowShelf, -5.0, f64::INFINITY, 1.0),
        Band::new(Kind::HighShelf, 1e9, 3.0, f64::NAN),
    ];
    let (_, mut eq) = processor(Some(bands.to_vec()));
    assert!(eq.prepare(44_100, 2));
    let mut samples = sine(1000.0, 44_100, 2, 4096, 0);
    eq.process(&mut samples);
    assert!(samples.iter().all(|s| s.is_finite()));
}

#[rstest]
#[case::boost(Kind::Peak, 1000.0, 6.0, 1.0, 1000.0)]
#[case::cut(Kind::Peak, 3000.0, -9.0, 2.0, 3000.0)]
#[case::shelf_below(Kind::LowShelf, 200.0, 4.0, 0.7, 80.0)]
fn processed_sine_follows_the_response(
    #[case] kind: Kind,
    #[case] freq: f64,
    #[case] gain: f64,
    #[case] q: f64,
    #[case] probe: f64,
) {
    let rate = 48_000;
    let bands = vec![Band::new(kind, freq, gain, q)];
    let expected =
        Response::new(&bands, f64::from(rate)).db(probe) + preamp_db(&bands, f64::from(rate));
    let (_, mut eq) = processor(Some(bands));
    assert!(eq.prepare(rate, 1));
    let warmup = rate as usize;
    let mut settle = sine(probe, rate, 1, warmup, 0);
    eq.process(&mut settle);
    let dry = sine(probe, rate, 1, warmup, warmup);
    let mut wet = dry.clone();
    eq.process(&mut wet);
    let measured = 20.0 * (rms(&wet) / rms(&dry)).log10();
    assert!(
        (measured - expected).abs() < 0.05,
        "{measured} vs {expected}"
    );
}

#[test]
fn nothing_to_do_is_a_bypass() {
    let (control, mut eq) = processor(None);
    assert!(!eq.prepare(44_100, 2));
    control.set(Some(vec![Band::new(Kind::Peak, 1000.0, 0.0, 1.0)]));
    assert!(!eq.prepare(44_100, 2));
    assert!(!eq.is_active());
}

#[test]
fn turning_off_fades_once_then_bypasses() {
    let (control, mut eq) = processor(Some(vec![Band::new(Kind::Peak, 1000.0, 6.0, 1.0)]));
    assert!(eq.prepare(44_100, 2));
    let mut samples = sine(440.0, 44_100, 2, 1024, 0);
    eq.process(&mut samples);
    control.set(None);
    assert!(eq.prepare(44_100, 2));
    assert!(!eq.is_active());
    let mut samples = sine(440.0, 44_100, 2, 1024, 1024);
    eq.process(&mut samples);
    assert!(!eq.prepare(44_100, 2));
}

#[test]
fn a_change_crossfades_without_a_jump() {
    let rate = 44_100;
    let frames = 2048;
    let (control, mut eq) = processor(Some(vec![Band::new(Kind::Peak, 300.0, 9.0, 0.7)]));
    let mut out = Vec::new();
    for batch in 0..8 {
        if batch == 4 {
            control.set(Some(vec![Band::new(Kind::Peak, 300.0, -9.0, 0.7)]));
        }
        let mut samples = sine(300.0, rate, 1, frames, batch * frames);
        if eq.prepare(rate, 1) {
            eq.process(&mut samples);
        }
        out.extend(samples);
    }
    let max_step = 0.25 * 2.0 * PI * 300.0 / f64::from(rate);
    let worst = out
        .windows(2)
        .skip(frames)
        .map(|w| f64::from(w[1] - w[0]).abs())
        .fold(0.0, f64::max);
    assert!(worst < max_step * 1.2, "{worst} vs {max_step}");
}

#[test]
fn a_format_change_rebuilds_without_a_fade() {
    let (control, mut eq) = processor(Some(vec![Band::new(Kind::Peak, 1000.0, 6.0, 1.0)]));
    assert!(eq.prepare(44_100, 2));
    control.set(None);
    assert!(!eq.prepare(96_000, 2));
}

#[test]
fn channels_are_filtered_independently() {
    let bands = vec![Band::new(Kind::Peak, 2000.0, 6.0, 1.5)];
    let left = sine(2000.0, 48_000, 1, 2048, 0);
    let right = sine(150.0, 48_000, 1, 2048, 0);
    let mut stereo: Vec<f32> = left
        .iter()
        .zip(&right)
        .flat_map(|(&l, &r)| [l, r])
        .collect();
    let (_, mut both) = processor(Some(bands.clone()));
    assert!(both.prepare(48_000, 2));
    both.process(&mut stereo);
    for (mono, channel) in [(left, 0), (right, 1)] {
        let (_, mut single) = processor(Some(bands.clone()));
        assert!(single.prepare(48_000, 1));
        let mut expected = mono;
        single.process(&mut expected);
        let got: Vec<f32> = stereo.iter().skip(channel).step_by(2).copied().collect();
        assert_eq!(got, expected);
    }
}

#[test]
fn parses_an_autoeq_file() {
    let bands = parse(HD600).unwrap();
    assert_eq!(bands.len(), 10);
    assert_eq!(bands[0], Band::new(Kind::LowShelf, 105.0, 6.5, 0.7));
    assert_eq!(bands[5], Band::new(Kind::HighShelf, 10_000.0, -3.1, 0.7));
    assert_eq!(bands[9], Band::new(Kind::Peak, 5433.0, -1.2, 5.7));
}

#[test]
fn parses_apo_variants() {
    let text = "# comment\nPreamp: -3 dB\nFilter: OFF PK Fc 1000 Hz Gain 3 dB BW Oct 1\nfilter 2: on hpq fc 30 hz q 0.5\nFilter 3: ON LP Fc 18000 Hz\nInclude: other.txt\n";
    let bands = parse(text).unwrap();
    assert_eq!(bands.len(), 3);
    assert!(!bands[0].enabled);
    assert!((bands[0].q - std::f64::consts::SQRT_2).abs() < 1e-9);
    assert_eq!(bands[1], Band::new(Kind::HighPass, 30.0, 0.0, 0.5));
    assert_eq!(bands[2], Band::new(Kind::LowPass, 18_000.0, 0.0, DEFAULT_Q));
}

#[rstest]
#[case::unsupported("Filter 1: ON NO Fc 1000 Hz\n", ParseError::Unsupported { line: 1, kind: "NO".into() })]
#[case::no_gain("Filter 1: ON PK Fc 1000 Hz Q 1\n", ParseError::Malformed { line: 1 })]
#[case::no_q("Filter 1: ON PK Fc 1000 Hz Gain 2 dB\n", ParseError::Malformed { line: 1 })]
#[case::graphic("GraphicEQ: 20 -1; 30 -2\n", ParseError::Empty)]
fn rejects_what_it_cannot_play(#[case] text: &str, #[case] error: ParseError) {
    assert_eq!(parse(text), Err(error));
}

#[test]
fn written_files_read_back() {
    let bands = vec![
        Band::new(Kind::LowShelf, 105.0, 6.5, 0.7),
        Band {
            enabled: false,
            ..Band::new(Kind::Peak, 31.5, -2.25, 1.234)
        },
        Band::new(Kind::HighPass, 25.0, 0.0, 0.5),
    ];
    let text = write(&bands);
    let preamp = format!("Preamp: {:.1} dB\n", preamp_db(&bands, RATE));
    assert!(text.starts_with(&preamp), "{text}");
    let back = parse(&text).unwrap();
    assert_eq!(back[0], bands[0]);
    assert_eq!(back[1].freq, 31.5);
    assert!(!back[1].enabled);
    assert!((back[1].gain + 2.25).abs() <= 0.05);
    assert!((back[1].q - 1.23).abs() < 1e-9);
    assert_eq!(back[2], bands[2]);
}

#[test]
fn preamp_covers_the_ultrasonic_range_of_hi_res() {
    let bands = [Band::new(Kind::HighShelf, 16_000.0, 12.0, 0.7)];
    assert!((preamp_db(&bands, 96_000.0) + 12.0).abs() < 0.1);
}

#[test]
fn setting_the_same_bands_changes_nothing() {
    let bands = vec![Band::new(Kind::Peak, 30.0, 9.0, 8.0)];
    let (_, mut steady) = processor(Some(bands.clone()));
    let (control, mut touched) = processor(Some(bands.clone()));
    for batch in 0..6 {
        let mut a = sine(30.0, 44_100, 1, 1152, batch * 1152);
        let mut b = a.clone();
        assert!(steady.prepare(44_100, 1));
        steady.process(&mut a);
        control.set(Some(bands.clone()));
        assert!(touched.prepare(44_100, 1));
        touched.process(&mut b);
        assert_eq!(a, b);
    }
}

#[test]
fn an_unchanged_band_keeps_ringing_through_a_change_elsewhere() {
    let narrow = Band::new(Kind::Peak, 30.0, 9.0, 8.0);
    let (control, mut eq) = processor(Some(vec![narrow, Band::new(Kind::Peak, 3000.0, -3.0, 1.0)]));
    let frames = 1470;
    let mut levels = Vec::new();
    for batch in 0..80 {
        if batch == 70 {
            control.set(Some(vec![narrow, Band::new(Kind::Peak, 3000.0, -6.0, 1.0)]));
        }
        let mut samples = sine(30.0, 44_100, 1, frames, batch * frames);
        assert!(eq.prepare(44_100, 1));
        eq.process(&mut samples);
        levels.push(20.0 * rms(&samples).log10());
    }
    let before = levels[69];
    assert!(
        levels[70..].iter().all(|db| (db - before).abs() < 0.3),
        "{levels:?}"
    );
}

#[test]
fn import_skips_empty_slots_and_a_byte_order_mark() {
    let text = "\u{feff}Filter 1: ON PK Fc 100 Hz Gain 2 dB Q 1\nFilter 2: ON None\n";
    let bands = parse(text).unwrap();
    assert_eq!(bands, [Band::new(Kind::Peak, 100.0, 2.0, 1.0)]);
}

#[test]
fn import_refuses_per_channel_filters() {
    let text = "Channel: all\nFilter 1: ON PK Fc 100 Hz Gain 2 dB Q 1\nChannel: L\nFilter 2: ON PK Fc 200 Hz Gain 2 dB Q 1\n";
    assert_eq!(parse(text), Err(ParseError::Channels { line: 3 }));
}

#[test]
fn import_stops_at_the_band_limit() {
    let file = |count: usize| {
        (1..=count)
            .map(|i| format!("Filter {i}: ON PK Fc {} Hz Gain 1 dB Q 1\n", 100 * i))
            .collect::<String>()
    };
    assert_eq!(
        parse(&file(crate::MAX_BANDS)).unwrap().len(),
        crate::MAX_BANDS
    );
    assert_eq!(
        parse(&format!("{}Filter 99: ON None\n", file(crate::MAX_BANDS)))
            .unwrap()
            .len(),
        crate::MAX_BANDS
    );
    assert_eq!(
        parse(&file(crate::MAX_BANDS + 1)),
        Err(ParseError::TooMany {
            count: crate::MAX_BANDS + 1
        })
    );
}
