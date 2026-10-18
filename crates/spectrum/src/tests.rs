use rstest::rstest;

use super::*;

const FRAME: f32 = 1.0 / 60.0;

fn sine(analyzer: &mut Analyzer, rate: u32, hz: f32, amplitude: f32) {
    let len = MAX_WINDOW;
    for (n, slot) in analyzer.signal_mut().iter_mut().enumerate() {
        let t = (n as f64 - len as f64) / f64::from(rate);
        *slot = amplitude * (2.0 * std::f64::consts::PI * f64::from(hz) * t).sin() as f32;
    }
}

fn noise(analyzer: &mut Analyzer, seed: &mut u64, amplitude: f32) {
    for slot in analyzer.signal_mut() {
        *seed ^= *seed << 13;
        *seed ^= *seed >> 7;
        *seed ^= *seed << 17;
        let unit = (*seed >> 11) as f64 / (1u64 << 53) as f64;
        *slot = amplitude * (2.0 * unit - 1.0) as f32;
    }
}

fn run(analyzer: &mut Analyzer, rate: Option<u32>, seconds: f32) {
    for _ in 0..(seconds / FRAME).round() as usize {
        analyzer.update(rate, FRAME);
    }
}

fn level_near(analyzer: &Analyzer, hz: f32) -> f32 {
    let (i, _) = analyzer
        .frequencies()
        .iter()
        .enumerate()
        .min_by(|a, b| (a.1 - hz).abs().total_cmp(&(b.1 - hz).abs()))
        .unwrap();
    analyzer.levels()[i]
}

#[rstest]
#[case::cd(44_100, 8_192)]
#[case::dat(48_000, 8_192)]
#[case::hires(96_000, 16_384)]
#[case::dxd(352_800, MAX_WINDOW)]
#[case::low(8_000, 4_096)]
fn window_keeps_about_170_ms(#[case] rate: u32, #[case] expected: usize) {
    assert_eq!(window_len(rate), expected);
}

#[rstest]
#[case::bass(100.0, 1)]
#[case::mid(1_000.0, 3)]
#[case::air(10_000.0, 5)]
fn full_scale_sine_reads_0_dbfs_in_its_zone(#[case] hz: f32, #[case] zone: usize) {
    let mut analyzer = Analyzer::new();
    analyzer.set_points(200);
    sine(&mut analyzer, 48_000, hz, 1.0);
    run(&mut analyzer, Some(48_000), 3.0);
    assert!(
        analyzer.zone_level(zone).abs() < 0.3,
        "{}",
        analyzer.zone_level(zone)
    );
    for other in (0..ZONES.len()).filter(|&z| z != zone) {
        assert!(analyzer.zone_level(other) < -40.0, "zone {other}");
    }
}

#[test]
fn a_narrow_band_reads_a_sine_close_to_its_level() {
    let mut analyzer = Analyzer::new();
    analyzer.set_points(2_000);
    analyzer.set_settings(Settings {
        smoothing_octaves: 1.0 / 48.0,
        tilt_db_per_octave: 0.0,
        release_seconds: 0.2,
    });
    sine(&mut analyzer, 48_000, 1_000.0, 0.5);
    run(&mut analyzer, Some(48_000), 0.5);
    let level = level_near(&analyzer, 1_000.0);
    assert!((-9.0..=-5.0).contains(&level), "{level}");
}

#[test]
fn tilt_lifts_the_highs_by_its_slope() {
    let measure = |tilt: f32| {
        let mut analyzer = Analyzer::new();
        analyzer.set_points(300);
        analyzer.set_settings(Settings {
            smoothing_octaves: 1.0 / 3.0,
            tilt_db_per_octave: tilt,
            release_seconds: 0.2,
        });
        let mut seed = 0x9E37_79B9_7F4A_7C15;
        let mut sum = 0.0;
        for _ in 0..30 {
            noise(&mut analyzer, &mut seed, 0.5);
            analyzer.update(Some(48_000), FRAME);
            sum += level_near(&analyzer, 8_000.0) - level_near(&analyzer, 125.0);
        }
        sum / 30.0
    };
    let flat = measure(0.0);
    let tilted = measure(3.0);
    assert!(flat.abs() < 2.5, "white noise should read flat: {flat}");
    assert!((tilted - flat - 18.0).abs() < 1.5, "{tilted} vs {flat}");
}

#[test]
fn silence_releases_the_curve_and_the_peak_holds_first() {
    let mut analyzer = Analyzer::new();
    analyzer.set_points(200);
    sine(&mut analyzer, 48_000, 1_000.0, 1.0);
    run(&mut analyzer, Some(48_000), 0.5);
    let peak = level_near(&analyzer, 1_000.0);
    run(&mut analyzer, None, 0.5);
    let i = analyzer
        .frequencies()
        .iter()
        .position(|&hz| hz >= 1_000.0)
        .unwrap();
    assert!(analyzer.levels()[i] < -100.0);
    assert!((analyzer.peaks()[i] - peak).abs() < 3.0);
    assert!(analyzer.is_settled(false));
    assert!(!analyzer.is_settled(true));
    run(&mut analyzer, None, 12.0);
    assert!(analyzer.is_settled(true));
}

#[test]
fn nothing_above_nyquist() {
    let mut analyzer = Analyzer::new();
    analyzer.set_points(200);
    let mut seed = 7;
    noise(&mut analyzer, &mut seed, 0.5);
    run(&mut analyzer, Some(22_050), 0.3);
    let above = analyzer
        .frequencies()
        .iter()
        .zip(analyzer.levels())
        .filter(|(hz, _)| **hz > 11_025.0);
    for (_, &level) in above {
        assert_eq!(level, SILENCE_DB);
    }
}

#[test]
fn axis_round_trips() {
    for hz in [20.0, 100.0, 1_000.0, 20_000.0] {
        assert!((frequency(position(hz)) - hz).abs() / hz < 1e-4);
    }
    assert_eq!(zone_of(59.0).name, "SUB");
    assert_eq!(zone_of(3_000.0).name, "HIGH MID");
    assert_eq!(zone_of(25_000.0).name, "AIR");
}

#[test]
fn a_new_point_count_keeps_the_curve() {
    let mut analyzer = Analyzer::new();
    analyzer.set_points(200);
    sine(&mut analyzer, 48_000, 1_000.0, 0.5);
    run(&mut analyzer, Some(48_000), 1.0);
    let before = level_near(&analyzer, 1_000.0);
    let peak_before = analyzer.peaks().iter().copied().fold(SILENCE_DB, f32::max);
    analyzer.set_points(317);
    assert_eq!(analyzer.levels().len(), 317);
    assert!((level_near(&analyzer, 1_000.0) - before).abs() < 6.0);
    let peak_after = analyzer.peaks().iter().copied().fold(SILENCE_DB, f32::max);
    assert!(peak_after > peak_before - 6.0);
}

#[rstest]
#[case::sub(31.5)]
#[case::bass(125.0)]
#[case::mid(1_000.0)]
#[case::high(8_000.0)]
#[case::top(18_000.0)]
fn a_sine_is_centered_on_its_frequency(#[case] hz: f32) {
    let mut analyzer = Analyzer::new();
    analyzer.set_points(470);
    sine(&mut analyzer, 48_000, hz, 0.25);
    run(&mut analyzer, Some(48_000), 0.5);
    let top = analyzer.levels().iter().copied().fold(SILENCE_DB, f32::max);
    let hump: Vec<f32> = analyzer
        .frequencies()
        .iter()
        .zip(analyzer.levels())
        .filter(|(_, level)| **level > top - 3.0)
        .map(|(f, _)| *f)
        .collect();
    let center = (hump[0] * hump[hump.len() - 1]).sqrt();
    assert!((center / hz).log2().abs() < 1.0 / 24.0, "{center} vs {hz}");
}
