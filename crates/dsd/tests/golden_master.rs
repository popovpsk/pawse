//! Cross-checks our DSD->PCM decode against ffmpeg's independent
//! implementation of the same dsd2pcm-derived filter. Requires `ffmpeg` on
//! PATH (`brew install ffmpeg`) — skips (not fails) if it's unavailable, so
//! this stays safe to run in environments without it.

use std::f64::consts::PI;
use std::path::Path;
use std::process::Command;

const DSD_HEADER_SIZE: u64 = 28;
const FMT_CHUNK_SIZE: u64 = 52;
const BLOCK_SIZE: u32 = 4096;

fn generate_sine_dsf(path: &Path, freq_hz: f64, dsd_rate: u32, channels: u8, duration_secs: f64) {
    let total_samples = (dsd_rate as f64 * duration_secs) as u64;
    let bytes_per_channel = total_samples.div_ceil(8);
    let total_blocks = bytes_per_channel.div_ceil(BLOCK_SIZE as u64);
    let padded_bits = total_blocks * BLOCK_SIZE as u64 * 8;
    let data_size = total_blocks * BLOCK_SIZE as u64 * channels as u64;

    let mut per_channel_bytes: Vec<Vec<u8>> = Vec::with_capacity(channels as usize);
    for _ in 0..channels {
        // First-order delta-sigma modulator: a minimal but valid DSD
        // encoder, good enough to carry a recognizable tone for a
        // cross-implementation sanity check (not audiophile quality).
        let mut integrator = 0.0f64;
        let mut prev = -1.0f64;
        let mut bits: Vec<u8> = Vec::with_capacity(padded_bits as usize);
        for n in 0..padded_bits {
            let x = if n < total_samples {
                (2.0 * PI * freq_hz * (n as f64 / dsd_rate as f64)).sin() * 0.5
            } else {
                0.0
            };
            integrator += x - prev;
            let bit = if integrator > 0.0 { 1u8 } else { 0u8 };
            prev = if bit == 1 { 1.0 } else { -1.0 };
            bits.push(bit);
        }
        let bytes: Vec<u8> = bits
            .chunks(8)
            .map(|chunk| {
                let mut byte = 0u8;
                for (i, &b) in chunk.iter().enumerate() {
                    if b == 1 {
                        byte |= 1 << i; // DSF is LSB-first: earliest sample -> bit 0
                    }
                }
                byte
            })
            .collect();
        per_channel_bytes.push(bytes);
    }

    let mut buf = Vec::new();
    buf.extend_from_slice(b"DSD ");
    buf.extend_from_slice(&DSD_HEADER_SIZE.to_le_bytes());
    let total_file_size = DSD_HEADER_SIZE + FMT_CHUNK_SIZE + 12 + data_size;
    buf.extend_from_slice(&total_file_size.to_le_bytes());
    buf.extend_from_slice(&0u64.to_le_bytes());

    buf.extend_from_slice(b"fmt ");
    buf.extend_from_slice(&FMT_CHUNK_SIZE.to_le_bytes());
    buf.extend_from_slice(&1u32.to_le_bytes());
    buf.extend_from_slice(&0u32.to_le_bytes());
    buf.extend_from_slice(&(if channels == 1 { 1u32 } else { 2u32 }).to_le_bytes());
    buf.extend_from_slice(&(channels as u32).to_le_bytes());
    buf.extend_from_slice(&dsd_rate.to_le_bytes());
    buf.extend_from_slice(&1u32.to_le_bytes());
    buf.extend_from_slice(&total_samples.to_le_bytes());
    buf.extend_from_slice(&BLOCK_SIZE.to_le_bytes());
    buf.extend_from_slice(&0u32.to_le_bytes());

    buf.extend_from_slice(b"data");
    buf.extend_from_slice(&(12 + data_size).to_le_bytes());

    for block in 0..total_blocks as usize {
        let start = block * BLOCK_SIZE as usize;
        let end = start + BLOCK_SIZE as usize;
        for channel_bytes in &per_channel_bytes {
            buf.extend_from_slice(&channel_bytes[start..end]);
        }
    }

    std::fs::write(path, buf).unwrap();
}

/// Looks on PATH first, then falls back to the repo's own gitignored
/// `./bin/ffmpeg` (a local dev-tool checkout some contributors keep there),
/// so this test doesn't require a PATH-wide install to be useful.
fn find_ffmpeg() -> Option<std::path::PathBuf> {
    if Command::new("ffmpeg").arg("-version").output().is_ok() {
        return Some("ffmpeg".into());
    }
    let repo_bin = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("bin")
        .join("ffmpeg");
    Command::new(&repo_bin)
        .arg("-version")
        .output()
        .ok()
        .map(|_| repo_bin)
}

#[test]
fn matches_ffmpeg_reference_decode() {
    let Some(ffmpeg) = find_ffmpeg() else {
        eprintln!("skipping golden-master test: no ffmpeg on PATH or in the repo's ./bin");
        return;
    };

    let dsf_path = std::env::temp_dir().join("pawse_dsd_golden_master.dsf");
    generate_sine_dsf(&dsf_path, 1000.0, 2_822_400, 2, 0.05);

    let mut ours = Vec::new();
    let mut source = dsd::DsdSource::open(&dsf_path).expect("open fixture");
    while let Some(batch) = source.next_buffer().expect("decode") {
        ours.extend(batch);
    }

    let output = Command::new(&ffmpeg)
        .args(["-v", "error", "-i"])
        .arg(&dsf_path)
        .args(["-f", "f32le", "-"])
        .output()
        .expect("run ffmpeg");
    assert!(
        output.status.success(),
        "ffmpeg failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let theirs: Vec<f32> = output
        .stdout
        .chunks_exact(4)
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect();

    let n = ours.len().min(theirs.len());
    assert!(
        n > 1000,
        "too few samples to compare: ours={} theirs={}",
        ours.len(),
        theirs.len()
    );

    let max_diff = ours[..n]
        .iter()
        .zip(theirs[..n].iter())
        .map(|(a, b)| (a - b).abs())
        .fold(0f32, f32::max);
    // Observed bit-exact (max_diff == 0.0) against ffmpeg 7.1 on this
    // fixture; a small epsilon avoids being brittle to float-arithmetic
    // differences across ffmpeg builds/platforms rather than real bugs.
    assert!(max_diff < 1e-4, "max diff vs ffmpeg reference: {max_diff}");

    std::fs::remove_file(&dsf_path).ok();
}
