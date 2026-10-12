use std::path::PathBuf;

use crate::frontend::{BANDS, MelFrontend, resample_linear};
use crate::test_support::{FakeSource, signal};
use crate::{Job, prepare};

fn golden(name: &str) -> Vec<f32> {
    let path = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap())
        .join("tests")
        .join("data")
        .join(name);
    std::fs::read(&path)
        .unwrap_or_else(|e| panic!("{}: {e}", path.display()))
        .as_chunks::<4>()
        .0
        .iter()
        .map(|b| f32::from_le_bytes(*b))
        .collect()
}

#[test]
fn mel_matches_essentia() {
    let expected = golden("mel_16k.f32");
    let ours = MelFrontend::new().compute(&signal(16_000, 3 * 16_000));
    let theirs = expected.as_chunks::<BANDS>().0;
    assert_eq!(theirs.len(), ours.len() + 1);
    let worst = ours
        .iter()
        .zip(theirs)
        .flat_map(|(a, b)| a.iter().zip(b.iter()).map(|(x, y)| (x - y).abs()))
        .fold(0f32, f32::max);
    assert!(worst < 1e-4, "max abs diff {worst:e}");
}

#[test]
fn resampler_matches_essentia() {
    let expected = golden("resample_44k.f32");
    let ours = resample_linear(&signal(44_100, 44_100), 44_100);
    let n = ours.len().min(expected.len());
    assert!(n > 15_000, "{} vs {}", ours.len(), expected.len());
    let diff: f64 = ours[..n]
        .iter()
        .zip(&expected[..n])
        .map(|(a, b)| (a - b).abs() as f64)
        .sum::<f64>()
        / n as f64;
    let level: f64 = expected[..n].iter().map(|v| v.abs() as f64).sum::<f64>() / n as f64;
    assert!(
        diff < 0.01 * level,
        "mean abs diff {diff:e} vs level {level:e}"
    );
}

#[test]
fn a_track_under_one_second_is_too_short() {
    let job = Job {
        source: Box::new(FakeSource::mono(44_100, signal(44_100, 44_000))),
        range: None,
    };
    assert!(matches!(prepare(job), Err(crate::EmbedError::TooShort)));
}

#[test]
fn a_long_track_prepares_32_patches() {
    let job = Job {
        source: Box::new(FakeSource::mono(16_000, signal(16_000, 75 * 16_000))),
        range: None,
    };
    assert_eq!(prepare(job).unwrap().patch_count(), 32);
}
