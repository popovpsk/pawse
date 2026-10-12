use std::path::PathBuf;

use audio_embedding::test_support::{FakeSource, signal};
use audio_embedding::{BANDS, DIM, Job, PATCH, PATCH_SAMPLES, PatchInput, Prepared, prepare};
use rstest::rstest;
use tract_onnx::prelude::Framework;

use crate::arch::BATCH;
use crate::engine::{Buf, Engine, FRONTEND_STEPS, Tap};
use crate::input::{INFO_WORDS, patch_info};
use crate::probe::cosine;
use crate::weights::{Weights, encode};
use crate::{Device, onnx, probe_input};

fn engine(weights: Option<&Weights>) -> Engine {
    let gpu = crate::gpu::any().expect("a GPU");
    Engine::new(gpu, weights)
}

fn model_path() -> PathBuf {
    std::env::var("PAWSE_EFFNET_MODEL")
        .expect("PAWSE_EFFNET_MODEL")
        .into()
}

pub(crate) fn weights_from_onnx() -> Weights {
    let path = model_path();
    let mut tensors = onnx::conv_tensors(&path).unwrap();
    let mel: Vec<f32> = probe_input()
        .iter()
        .flat_map(Prepared::reference_mel)
        .collect();
    tensors.push(onnx::run(&path, &mel, &[]).unwrap().remove(0));
    Weights::parse(encode(&tensors).unwrap()).unwrap()
}

fn device(weights: &Weights) -> Device {
    let adapter = crate::gpu::candidates(false)
        .into_iter()
        .next()
        .expect("a GPU");
    Device::build(&adapter, weights).unwrap()
}

fn inputs(items: &[Prepared]) -> (Vec<[u32; INFO_WORDS]>, Vec<&[f32]>) {
    let mut infos = Vec::new();
    let mut pcm = Vec::new();
    let mut offset = 0;
    for item in items {
        for ix in 0..item.patch_count() {
            let patch = item.patch(ix);
            infos.push(patch_info(
                item.rate(),
                item.resampled_len(),
                &patch,
                offset,
            ));
            offset += patch.samples.len();
            pcm.push(patch.samples);
        }
    }
    (infos, pcm)
}

fn hashed(k: i64) -> f32 {
    let h = (k as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) >> 40;
    h as f32 / (1u64 << 24) as f32 - 0.5
}

fn reference_y(rate: u32, resampled: usize, patch: &PatchInput, len: u64) -> Vec<f32> {
    let step = rate as f64 / 16_000.0;
    let x = |k: i64| -> f32 {
        if k < 0 || k as u64 >= len {
            return 0.0;
        }
        let rel = k - patch.start as i64;
        assert!(
            rel >= 0 && (rel as usize) < patch.samples.len(),
            "sample {k} outside the window"
        );
        patch.samples[rel as usize]
    };
    let y0 = patch.first_frame as i64 * 256 - 256;
    (0..PATCH_SAMPLES as i64)
        .map(|j| {
            let i = y0 + j;
            if i < 0 || i as usize >= resampled {
                return 0.0;
            }
            if rate == 16_000 {
                return x(i);
            }
            let pos = i as f64 * step - 1.0;
            let idx = pos.floor();
            let frac = (pos - idx) as f32;
            let a = x(idx as i64);
            let b = x(idx as i64 + 1);
            a + (b - a) * frac
        })
        .collect()
}

struct Window {
    first_frame: usize,
    start: u64,
    samples: Vec<f32>,
}

fn window(rate: u32, first_frame: usize, resampled: usize, len: u64) -> Window {
    let step = rate as f64 / 16_000.0;
    let ys = (first_frame * 256).saturating_sub(256)
        ..((first_frame + PATCH - 1) * 256 + 256).min(resampled);
    let (lo, hi) = if rate == 16_000 {
        (ys.start as u64, ys.end as u64)
    } else {
        let lo = (ys.start as f64 * step - 1.0).floor().max(0.0) as u64;
        let hi = ((ys.end - 1) as f64 * step - 1.0).floor() as u64 + 2;
        (lo.min(len), hi.min(len))
    };
    Window {
        first_frame,
        start: lo,
        samples: (lo..hi).map(|k| hashed(k as i64)).collect(),
    }
}

#[rstest]
#[case::narrowband(8_000)]
#[case::model_rate(16_000)]
#[case::low(22_050)]
#[case::cd(44_100)]
#[case::dat(48_000)]
#[case::hires(96_000)]
#[case::hires_192(192_000)]
#[case::odd(44_101)]
#[ignore = "needs a GPU"]
fn resampling_matches_the_cpu_formula(#[case] rate: u32) {
    let engine = engine(None);
    let len = rate as u64 * 30 * 60;
    let resampled = (len as f64 / (rate as f64 / 16_000.0)).floor() as usize;
    let last = (resampled - 256) / 256 - 127;
    let firsts = [0, 1, 62, 7_000, 54_321, last - 1, last, last + 60];
    let windows: Vec<Window> = firsts
        .iter()
        .map(|&f| window(rate, f, resampled, len))
        .collect();
    let patches: Vec<PatchInput> = windows
        .iter()
        .map(|w| PatchInput {
            first_frame: w.first_frame,
            frames: PATCH,
            start: w.start,
            samples: &w.samples,
        })
        .collect();
    let mut infos = Vec::new();
    let mut offset = 0;
    for patch in &patches {
        infos.push(patch_info(rate, resampled, patch, offset));
        offset += patch.samples.len();
    }
    let pcm: Vec<&[f32]> = patches.iter().map(|p| p.samples).collect();
    let tap = Tap {
        after: 0,
        buffer: Buf::D,
        len: BATCH * PATCH_SAMPLES,
    };
    let pending = engine.submit_with_taps(&infos, &pcm, 0, &[tap]).unwrap();
    let y = engine.wait(pending).unwrap().remove(0);
    for (ix, patch) in patches.iter().enumerate() {
        let expected = reference_y(rate, resampled, patch, len);
        let got = &y[ix * PATCH_SAMPLES..(ix + 1) * PATCH_SAMPLES];
        let worst = expected
            .iter()
            .zip(got)
            .map(|(a, b)| (a - b).abs())
            .fold(0f32, f32::max);
        assert!(
            worst <= 1e-6,
            "{rate} Hz, patch at frame {}: max diff {worst:e}",
            patch.first_frame
        );
    }
    let unused = &y[patches.len() * PATCH_SAMPLES..];
    assert!(unused.iter().all(|&v| v == 0.0));
}

fn test_items() -> Vec<Prepared> {
    let mut items = Vec::new();
    for (rate, len, exact) in [
        (44_100u32, 150 * 44_100, true),
        (16_000, 24_000, false),
        (22_050, 9 * 22_050, false),
    ] {
        let mut source = FakeSource::mono(rate, signal(rate, len));
        if exact {
            source.exact = Some(len as u64);
        }
        items.push(
            prepare(Job {
                source: Box::new(source),
                range: None,
            })
            .unwrap(),
        );
    }
    items
}

fn gpu_mel(engine: &Engine, items: &[Prepared]) -> Vec<f32> {
    let (infos, pcm) = inputs(items);
    let tap = Tap {
        after: FRONTEND_STEPS,
        buffer: Buf::Q,
        len: BATCH * PATCH * BANDS,
    };
    let pending = engine
        .submit_with_taps(&infos, &pcm, FRONTEND_STEPS, &[tap])
        .unwrap();
    engine.wait(pending).unwrap().remove(0)
}

#[test]
#[ignore = "needs a GPU"]
fn the_mel_spectrogram_matches_the_cpu_reference() {
    let engine = engine(None);
    assert_eq!(engine.steps(), FRONTEND_STEPS);
    let items = test_items();
    assert_eq!(items[0].rate(), 44_100);
    let mel = gpu_mel(&engine, &items);
    let expected: Vec<f32> = items.iter().flat_map(Prepared::reference_mel).collect();
    let worst = expected
        .iter()
        .zip(&mel)
        .map(|(a, b)| (a - b).abs())
        .fold(0f32, f32::max);
    assert!(worst <= 2e-5, "max diff {worst:e}");
    assert!(mel[expected.len()..].iter().all(|&v| v == 0.0));
    let short = &items[1];
    let first_empty = items[0].patch_count() * PATCH * BANDS + short.patch(0).frames * BANDS;
    assert!(short.patch(0).frames < PATCH);
    assert!(
        mel[first_empty..first_empty + BANDS]
            .iter()
            .all(|&v| v == 0.0)
    );
}

#[test]
#[ignore = "needs a GPU"]
fn the_mel_spectrogram_matches_essentia() {
    let path = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap())
        .join("../audio_embedding/tests/data/mel_16k.f32");
    let golden: Vec<f32> = std::fs::read(&path)
        .unwrap()
        .as_chunks::<4>()
        .0
        .iter()
        .map(|b| f32::from_le_bytes(*b))
        .collect();
    let item = prepare(Job {
        source: Box::new(FakeSource::mono(16_000, signal(16_000, 3 * 16_000))),
        range: None,
    })
    .unwrap();
    assert_eq!(item.patch_count(), 1);
    assert_eq!(item.patch(0).first_frame, 0);
    let mel = gpu_mel(&engine(None), std::slice::from_ref(&item));
    let worst = mel[..PATCH * BANDS]
        .iter()
        .zip(&golden[..PATCH * BANDS])
        .map(|(a, b)| (a - b).abs())
        .fold(0f32, f32::max);
    assert!(worst < 1e-4, "max abs diff {worst:e}");
}

fn block_outputs(path: &std::path::Path) -> Vec<String> {
    let model = tract_onnx::onnx().proto_model_for_path(path).unwrap();
    let graph = model.graph.as_ref().unwrap();
    let convs: Vec<_> = graph.node.iter().filter(|n| n.op_type == "Conv").collect();
    crate::arch::BLOCKS
        .iter()
        .enumerate()
        .map(|(ix, block)| {
            let project = &convs[4 + 5 * ix].output[0];
            graph
                .node
                .iter()
                .filter(|_| block.residual())
                .find(|n| n.op_type == "Add" && n.input.first() == Some(project))
                .map_or(project.clone(), |n| n.output[0].clone())
        })
        .collect()
}

fn nchw(cbhw: &[f32], channels: usize, plane: usize) -> Vec<f32> {
    let mut out = vec![0f32; cbhw.len()];
    for c in 0..channels {
        for b in 0..BATCH {
            for s in 0..plane {
                out[(b * channels + c) * plane + s] = cbhw[(c * BATCH + b) * plane + s];
            }
        }
    }
    out
}

#[test]
#[ignore = "needs a GPU and PAWSE_EFFNET_MODEL"]
fn every_block_matches_tract() {
    let weights = weights_from_onnx();
    let engine = engine(Some(&weights));
    let items = probe_input();
    let (infos, pcm) = inputs(&items);
    let mut taps = vec![Tap {
        after: FRONTEND_STEPS,
        buffer: Buf::Q,
        len: BATCH * PATCH * BANDS,
    }];
    taps.extend(engine.block_taps.iter().copied());
    taps.push(Tap {
        after: engine.steps(),
        buffer: Buf::Out,
        len: BATCH * DIM,
    });
    let pending = engine
        .submit_with_taps(&infos, &pcm, engine.steps(), &taps)
        .unwrap();
    let got = engine.wait(pending).unwrap();
    let names = block_outputs(&model_path());
    let expected = onnx::run(&model_path(), &got[0], &names).unwrap();
    let planes = crate::arch::planes();
    let mut worst = 1.0f64;
    for (ix, block) in crate::arch::BLOCKS.iter().enumerate() {
        let plane = planes[ix].1.area();
        let ours = nchw(&got[ix + 1], block.output, plane);
        let cos = cosine(&ours, &expected[ix]);
        let scale = expected[ix].iter().map(|v| v.abs()).fold(0f32, f32::max);
        let diff = ours
            .iter()
            .zip(&expected[ix])
            .map(|(a, b)| (a - b).abs())
            .fold(0f32, f32::max);
        eprintln!("block {ix:2}: cosine {cos:.9}, max diff {diff:e} of {scale:e}");
        worst = worst.min(cos);
    }
    let rows = crate::probe::worst_cosine(&expected[16], &got[17]);
    eprintln!("embeddings: worst row cosine {rows:.9}");
    assert!(worst >= 0.99999, "worst block cosine {worst}");
    assert!(rows >= 0.99999, "worst row cosine {rows}");
}

#[test]
#[ignore = "needs a GPU and PAWSE_EFFNET_MODEL"]
fn the_device_passes_its_probe_and_matches_essentia() {
    let device = device(&weights_from_onnx());
    eprintln!("{} agrees to {:.9}", device.name(), device.agreement());
    let path = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap())
        .join("../audio_embedding/tests/data/embedding.f32");
    let golden: Vec<f32> = std::fs::read(&path)
        .unwrap()
        .as_chunks::<4>()
        .0
        .iter()
        .map(|b| f32::from_le_bytes(*b))
        .collect();
    let item = prepare(Job {
        source: Box::new(FakeSource::mono(16_000, signal(16_000, 75 * 16_000))),
        range: None,
    })
    .unwrap();
    let embedding = device.embed(&[item]).pop().unwrap().unwrap();
    let cos = cosine(&embedding.vector, &golden);
    eprintln!("cosine to Essentia {cos:.12}");
    assert!(cos >= 0.9999, "cosine {cos}");
}

#[test]
#[ignore = "needs a GPU and PAWSE_EFFNET_MODEL"]
fn a_track_embeds_the_same_alone_and_in_a_batch() {
    let device = device(&weights_from_onnx());
    let jobs = || {
        [
            (5 * 44_100, 44_100, false),
            (75 * 16_000, 16_000, false),
            (150 * 44_100, 44_100, true),
            (90 * 22_050, 22_050, false),
            (3 * 22_050, 22_050, false),
        ]
        .into_iter()
        .map(|(len, rate, exact)| {
            let mut source = FakeSource::mono(rate, signal(rate, len));
            if exact {
                source.exact = Some(len as u64);
            }
            prepare(Job {
                source: Box::new(source),
                range: None,
            })
            .unwrap()
        })
        .collect::<Vec<_>>()
    };
    let together = device.embed(&jobs());
    for (ix, prepared) in jobs().into_iter().enumerate() {
        let alone = device.embed(&[prepared]).pop().unwrap().unwrap();
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

fn music_files(dir: &std::path::Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            music_files(&path, out);
        } else if path.extension().and_then(|e| e.to_str()).is_some_and(|e| {
            ["flac", "mp3", "m4a", "ogg", "opus", "wav", "ape", "dsf"]
                .contains(&e.to_ascii_lowercase().as_str())
        }) {
            out.push(path);
        }
    }
}

#[test]
#[ignore = "needs a GPU, PAWSE_EFFNET_MODEL and PAWSE_MUSIC_DIR"]
fn real_tracks_match_the_tract_cpu_path() {
    let dir = PathBuf::from(std::env::var("PAWSE_MUSIC_DIR").expect("PAWSE_MUSIC_DIR"));
    let mut files = Vec::new();
    music_files(&dir, &mut files);
    let mut picked: Vec<PathBuf> = files
        .iter()
        .step_by(files.len().div_ceil(40).max(1))
        .cloned()
        .collect();
    for ext in ["mp3", "m4a", "opus", "ape", "dsf", "wav"] {
        picked.extend(
            files
                .iter()
                .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case(ext)))
                .take(2)
                .cloned(),
        );
    }
    let device = device(&weights_from_onnx());
    let mut worst = 1.0f64;
    for path in &picked {
        let decoder = audio_decoder::Decoder::open(path).unwrap();
        let item = match prepare(Job {
            source: Box::new(decoder),
            range: None,
        }) {
            Ok(item) => item,
            Err(audio_embedding::EmbedError::TooShort) => continue,
            Err(e) => panic!("{}: {e}", path.display()),
        };
        let mel = item.reference_mel();
        let rows = onnx::run(&model_path(), &mel, &[]).unwrap().remove(0);
        let count = item.patch_count();
        let reference: Vec<f32> = (0..DIM)
            .map(|d| {
                (0..count).map(|r| rows[r * DIM + d] as f64).sum::<f64>() as f32 / count as f32
            })
            .collect();
        let got = device
            .embed(std::slice::from_ref(&item))
            .pop()
            .unwrap()
            .unwrap();
        let cos = cosine(&got.vector, &reference);
        worst = worst.min(cos);
        eprintln!(
            "{cos:.9} {} Hz {} {}",
            item.rate(),
            count,
            path.strip_prefix(&dir).unwrap().display()
        );
    }
    eprintln!("{} tracks, worst cosine {worst:.9}", picked.len());
    assert!(worst >= 0.9999, "worst cosine {worst}");
}
