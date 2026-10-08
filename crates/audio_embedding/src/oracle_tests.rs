use std::f64::consts::PI;
use std::path::PathBuf;

use crate::decode::tests::FakeSource;
use crate::frontend::{BANDS, MelFrontend, resample_linear};
use crate::{Analyzer, DIM, EMBEDDING_VERSION, Job, prepare};

const CHORD: [f64; 4] = [1.0, 1.25, 1.5, 1.335];

fn signal(rate: u32, length: usize) -> Vec<f32> {
    let mut state: u32 = 0x1234_5678;
    (0..length)
        .map(|i| {
            let t = i as f64 / rate as f64;
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let noise = (state >> 8) as f64 / 16_777_216.0 * 2.0 - 1.0;
            let k = CHORD[(t / 2.0).floor() as usize % 4];
            let env = 0.6 + 0.4 * (2.0 * PI * 0.5 * t).sin();
            let tone = 0.3 * (2.0 * PI * 220.0 * k * t).sin()
                + 0.2 * (2.0 * PI * 554.37 * k * t).sin()
                + 0.1 * (2.0 * PI * 1760.0 * k * t + 0.3).sin();
            (env * tone + 0.05 * noise) as f32
        })
        .collect()
}

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

fn model() -> Analyzer {
    let path = std::env::var("PAWSE_EFFNET_MODEL").expect("PAWSE_EFFNET_MODEL");
    Analyzer::load(path.as_ref()).unwrap()
}

fn embed_signal(analyzer: &Analyzer, rate: u32, samples: Vec<f32>) -> Box<[f32]> {
    let job = Job {
        source: Box::new(FakeSource::mono(rate, samples)),
        range: None,
    };
    let prepared = prepare(job).unwrap();
    let mut out = analyzer.embed(&[prepared]);
    let embedding = out.pop().unwrap().unwrap();
    assert_eq!(embedding.version, EMBEDDING_VERSION);
    embedding.vector
}

fn cosine(a: &[f32], b: &[f32]) -> f64 {
    let dot: f64 = a.iter().zip(b).map(|(x, y)| *x as f64 * *y as f64).sum();
    let na: f64 = a.iter().map(|x| (*x as f64).powi(2)).sum::<f64>().sqrt();
    let nb: f64 = b.iter().map(|x| (*x as f64).powi(2)).sum::<f64>().sqrt();
    dot / (na * nb)
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
#[ignore = "needs PAWSE_EFFNET_MODEL"]
fn embedding_matches_essentia_and_onnx_runtime() {
    let expected = golden("embedding.f32");
    assert_eq!(expected.len(), DIM);
    let ours = embed_signal(&model(), 16_000, signal(16_000, 75 * 16_000));
    let cos = cosine(&ours, &expected);
    assert!(cos >= 0.9999, "cosine {cos}");
}

#[test]
#[ignore = "needs PAWSE_EFFNET_MODEL"]
fn a_track_embeds_the_same_alone_and_in_a_batch() {
    let analyzer = model();
    let jobs = || {
        [75 * 16_000, 5 * 44_100, 3 * 22_050]
            .into_iter()
            .zip([16_000, 44_100, 22_050])
            .map(|(len, rate)| {
                prepare(Job {
                    source: Box::new(FakeSource::mono(rate, signal(rate, len))),
                    range: None,
                })
                .unwrap()
            })
            .collect::<Vec<_>>()
    };
    let together = analyzer.embed(&jobs());
    for (ix, prepared) in jobs().into_iter().enumerate() {
        let alone = analyzer.embed(&[prepared]).pop().unwrap().unwrap();
        let batched = together[ix].as_ref().unwrap();
        let worst = alone
            .vector
            .iter()
            .zip(batched.vector.iter())
            .map(|(a, b)| (a - b).abs())
            .fold(0f32, f32::max);
        assert!(worst <= 1e-6, "track {ix}: max abs diff {worst:e}");
    }
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
