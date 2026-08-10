use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::time::Duration;

use crate::container::{dff, dsf};
use crate::convert::{self, ChannelState, DsdTables};
use crate::error::DsdError;
use crate::halfband::HalfbandDecimator;

const DFF_BLOCK_BYTES_PER_CHANNEL: u64 = 4096;

/// DSD64's own rate — the native ÷8 output of the ported filter (352.8kHz)
/// is already validated bit-exact against ffmpeg and sits comfortably under
/// the ~384kHz ceiling most DACs support. Higher DSD multiples (128/256/512)
/// get cascaded down to this same rate instead of handing out 705.6kHz,
/// 1.4112MHz or 2.8224MHz PCM that no real hardware accepts and nobody
/// needs — see the ported filter's own comment about a further ÷8 stage
/// being "practically alias-free below 70 kHz".
const NATIVE_DSD64_RATE: u32 = 2_822_400;
const HALFBAND_TAPS: usize = 31;

/// Number of extra decimate-by-2 halfband stages needed to bring `dsd_rate`
/// down to [`NATIVE_DSD64_RATE`]'s own ÷8 rate. Falls back to 0 (no extra
/// stage, old ÷8-only behaviour) for anything that isn't a clean power-of-two
/// multiple of DSD64 — better to hand out a higher-than-ideal rate on an
/// unusual file than to guess wrong and corrupt the stream.
fn extra_decimation_stages(dsd_rate: u32) -> u32 {
    if dsd_rate <= NATIVE_DSD64_RATE || !dsd_rate.is_multiple_of(NATIVE_DSD64_RATE) {
        return 0;
    }
    let multiplier = dsd_rate / NATIVE_DSD64_RATE;
    if !multiplier.is_power_of_two() {
        return 0;
    }
    multiplier.ilog2()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DsdKind {
    Dsf,
    Dff,
}

pub fn sniff(path: &Path) -> Option<DsdKind> {
    let mut file = File::open(path).ok()?;
    let mut magic = [0u8; 4];
    file.read_exact(&mut magic).ok()?;
    match &magic {
        b"DSD " => Some(DsdKind::Dsf),
        b"FRM8" => Some(DsdKind::Dff),
        _ => None,
    }
}

#[derive(Debug, Clone, Copy)]
pub struct DsdParams {
    pub channels: u8,
    pub dsd_rate: u32,
    pub pcm_sample_rate: u32,
    pub pcm_bit_depth: u8,
}

enum Backend {
    Dsf(dsf::DsfBlockReader<File>),
    Dff(dff::DffBlockReader<File>),
}

impl Backend {
    fn total_blocks(&self) -> u64 {
        match self {
            Backend::Dsf(r) => r.info().total_blocks(),
            Backend::Dff(r) => r.total_blocks(),
        }
    }

    fn bytes_per_channel_total(&self) -> u64 {
        match self {
            Backend::Dsf(r) => r.info().bytes_per_channel(),
            Backend::Dff(r) => r.bytes_per_channel_total(),
        }
    }

    fn seek_to_block(&mut self, block_index: u64) -> Result<(), DsdError> {
        match self {
            Backend::Dsf(r) => r.seek_to_block(block_index),
            Backend::Dff(r) => r.seek_to_block(block_index),
        }
    }

    fn next_block(&mut self) -> Result<Option<Vec<Vec<u8>>>, DsdError> {
        match self {
            Backend::Dsf(r) => r.next_block(),
            Backend::Dff(r) => r.next_block(),
        }
    }
}

pub struct DsdSource {
    backend: Backend,
    channels: u8,
    dsd_rate: u32,
    pcm_sample_rate: u32,
    lsbf: bool,
    tables: DsdTables,
    channel_states: Vec<ChannelState>,
    /// `halfband_stages[stage][channel]`. Empty for DSD64 (native ÷8 output
    /// already lands on the target rate, no extra work).
    halfband_stages: Vec<Vec<HalfbandDecimator>>,
    block_bytes_per_channel: u64,
}

impl DsdSource {
    pub fn open(path: &Path) -> Result<Self, DsdError> {
        let kind = sniff(path).ok_or(DsdError::NotDsd)?;
        let file = File::open(path)?;

        let (backend, channels, dsd_rate, lsbf, block_bytes_per_channel) = match kind {
            DsdKind::Dsf => {
                let mut f = file;
                let info = dsf::parse_header(&mut f)?;
                let block_bytes_per_channel = info.block_size as u64;
                let mut reader = dsf::DsfBlockReader::new(f, info);
                reader.seek_to_block(0)?;
                (
                    Backend::Dsf(reader),
                    info.channels,
                    info.dsd_rate,
                    true,
                    block_bytes_per_channel,
                )
            }
            DsdKind::Dff => {
                let mut f = file;
                let info = dff::parse_header(&mut f)?;
                let reader = dff::DffBlockReader::new(f, info, DFF_BLOCK_BYTES_PER_CHANNEL)?;
                (
                    Backend::Dff(reader),
                    info.channels,
                    info.dsd_rate,
                    false,
                    DFF_BLOCK_BYTES_PER_CHANNEL,
                )
            }
        };

        let channel_states = (0..channels).map(|_| ChannelState::new(lsbf)).collect();

        let stage_count = extra_decimation_stages(dsd_rate);
        let halfband_stages = (0..stage_count)
            .map(|_| {
                (0..channels)
                    .map(|_| HalfbandDecimator::new(HALFBAND_TAPS))
                    .collect()
            })
            .collect();
        let pcm_sample_rate = (dsd_rate / 8) >> stage_count;

        Ok(Self {
            backend,
            channels,
            dsd_rate,
            pcm_sample_rate,
            lsbf,
            tables: DsdTables::new(),
            channel_states,
            halfband_stages,
            block_bytes_per_channel,
        })
    }

    pub fn params(&self) -> DsdParams {
        DsdParams {
            channels: self.channels,
            dsd_rate: self.dsd_rate,
            pcm_sample_rate: self.pcm_sample_rate,
            pcm_bit_depth: 24,
        }
    }

    pub fn duration(&self) -> Option<Duration> {
        let bytes_per_channel = self.backend.bytes_per_channel_total();
        Some(Duration::from_secs_f64(
            bytes_per_channel as f64 * 8.0 / self.dsd_rate as f64,
        ))
    }

    pub fn next_buffer(&mut self) -> Result<Option<Vec<f32>>, DsdError> {
        loop {
            let Some(per_channel_bytes) = self.backend.next_block()? else {
                return Ok(None);
            };
            let block_len = per_channel_bytes.first().map(Vec::len).unwrap_or(0);
            if block_len == 0 {
                return Ok(None);
            }

            let mut per_channel_pcm: Vec<Vec<f32>> = Vec::with_capacity(self.channels as usize);
            for (bytes, state) in per_channel_bytes.iter().zip(self.channel_states.iter_mut()) {
                let mut out = vec![0f32; bytes.len()];
                convert::translate(state, &self.tables, self.lsbf, bytes, &mut out);
                per_channel_pcm.push(out);
            }

            for stage in &mut self.halfband_stages {
                let mut next_pcm: Vec<Vec<f32>> = Vec::with_capacity(self.channels as usize);
                for (channel_pcm, decimator) in per_channel_pcm.iter().zip(stage.iter_mut()) {
                    let mut out = Vec::with_capacity(channel_pcm.len() / 2 + 1);
                    decimator.process(channel_pcm, &mut out);
                    next_pcm.push(out);
                }
                per_channel_pcm = next_pcm;
            }

            let out_len = per_channel_pcm.first().map(Vec::len).unwrap_or(0);
            if out_len == 0 {
                // A very small trailing native block can be fully absorbed by
                // the halfband cascade's decimation phase — pull the next
                // block instead of handing back an empty (but not EOF) batch.
                continue;
            }

            let mut interleaved = Vec::with_capacity(out_len * self.channels as usize);
            for i in 0..out_len {
                for channel in per_channel_pcm.iter() {
                    interleaved.push(channel[i]);
                }
            }
            return Ok(Some(interleaved));
        }
    }

    pub fn seek(&mut self, position: f32) -> Result<Duration, DsdError> {
        let position = position.clamp(0.0, 1.0) as f64;
        let total_blocks = self.backend.total_blocks();
        let target_block = ((total_blocks as f64) * position) as u64;
        self.backend.seek_to_block(target_block)?;

        for state in &mut self.channel_states {
            state.reset(self.lsbf);
        }
        for stage in &mut self.halfband_stages {
            for decimator in stage {
                decimator.reset();
            }
        }

        let seconds =
            (target_block * self.block_bytes_per_channel * 8) as f64 / self.dsd_rate as f64;
        Ok(Duration::from_secs_f64(seconds))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_test_dsf(name: &str, channels: u32, dsd_rate: u32, blocks: u32) -> std::path::PathBuf {
        const BLOCK_SIZE: u32 = 64;
        let sample_count = (blocks * BLOCK_SIZE * 8) as u64;
        let bytes_per_channel = sample_count.div_ceil(8);
        let total_blocks = bytes_per_channel.div_ceil(BLOCK_SIZE as u64);
        let data_size = total_blocks * BLOCK_SIZE as u64 * channels as u64;

        let mut buf = Vec::new();
        buf.extend_from_slice(b"DSD ");
        buf.extend_from_slice(&28u64.to_le_bytes());
        buf.extend_from_slice(&(28 + 52 + 12 + data_size).to_le_bytes());
        buf.extend_from_slice(&0u64.to_le_bytes());

        buf.extend_from_slice(b"fmt ");
        buf.extend_from_slice(&52u64.to_le_bytes());
        buf.extend_from_slice(&1u32.to_le_bytes());
        buf.extend_from_slice(&0u32.to_le_bytes());
        buf.extend_from_slice(&2u32.to_le_bytes());
        buf.extend_from_slice(&channels.to_le_bytes());
        buf.extend_from_slice(&dsd_rate.to_le_bytes());
        buf.extend_from_slice(&1u32.to_le_bytes());
        buf.extend_from_slice(&sample_count.to_le_bytes());
        buf.extend_from_slice(&BLOCK_SIZE.to_le_bytes());
        buf.extend_from_slice(&0u32.to_le_bytes());

        buf.extend_from_slice(b"data");
        buf.extend_from_slice(&(12 + data_size).to_le_bytes());
        // Silence pattern so decoded output should stay near zero throughout.
        buf.extend(std::iter::repeat_n(0x69u8, data_size as usize));

        let path = std::env::temp_dir().join(name);
        let mut f = File::create(&path).unwrap();
        f.write_all(&buf).unwrap();
        path
    }

    #[test]
    fn open_reports_correct_params() {
        let path = write_test_dsf("pawse_dsd_test_params.dsf", 2, 2_822_400, 4);
        let source = DsdSource::open(&path).unwrap();
        let params = source.params();
        assert_eq!(params.channels, 2);
        assert_eq!(params.dsd_rate, 2_822_400);
        assert_eq!(params.pcm_sample_rate, 352_800);
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn open_rejects_zero_sample_rate_instead_of_panicking_later() {
        // `audio_engine` calls `.duration()` on every track load — if a
        // zero rate ever reached that point it would feed NaN into
        // `Duration::from_secs_f64` and panic. It must be refused here, at
        // `open()`, so no caller can ever reach that state.
        let path = write_test_dsf("pawse_dsd_test_zero_rate.dsf", 2, 0, 4);
        assert!(DsdSource::open(&path).is_err());
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn dsd256_still_targets_dsd64s_native_pcm_rate() {
        let path = write_test_dsf("pawse_dsd_test_dsd256.dsf", 2, 2_822_400 * 4, 16);
        let mut source = DsdSource::open(&path).unwrap();
        let params = source.params();
        assert_eq!(params.dsd_rate, 11_289_600);
        assert_eq!(
            params.pcm_sample_rate, 352_800,
            "DSD256 must land on the same fixed target as DSD64, not 1.4112 MHz"
        );

        let mut total_samples = 0;
        while let Some(batch) = source.next_buffer().unwrap() {
            for s in &batch {
                assert!(s.abs() < 1e-2, "expected near-silence, got {s}");
            }
            total_samples += batch.len();
        }
        assert!(total_samples > 0);
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn dsd128_halves_correctly() {
        let path = write_test_dsf("pawse_dsd_test_dsd128.dsf", 2, 2_822_400 * 2, 16);
        let source = DsdSource::open(&path).unwrap();
        assert_eq!(source.params().pcm_sample_rate, 352_800);
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn decodes_silence_to_near_zero_pcm() {
        let path = write_test_dsf("pawse_dsd_test_silence.dsf", 2, 2_822_400, 4);
        let mut source = DsdSource::open(&path).unwrap();
        let mut total_samples = 0;
        while let Some(batch) = source.next_buffer().unwrap() {
            for s in &batch {
                assert!(s.abs() < 1e-2, "expected near-silence, got {s}");
            }
            total_samples += batch.len();
        }
        assert!(total_samples > 0);
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn duration_matches_sample_count() {
        let path = write_test_dsf("pawse_dsd_test_duration.dsf", 2, 2_822_400, 4);
        let source = DsdSource::open(&path).unwrap();
        let expected_secs = (4.0 * 64.0 * 8.0) / 2_822_400.0;
        let got = source.duration().unwrap().as_secs_f64();
        assert!(
            (got - expected_secs).abs() < 1e-6,
            "{got} vs {expected_secs}"
        );
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn seek_resets_filter_state_and_reports_position() {
        let path = write_test_dsf("pawse_dsd_test_seek.dsf", 2, 2_822_400, 8);
        let mut source = DsdSource::open(&path).unwrap();
        let pos = source.seek(0.5).unwrap();
        assert!(pos.as_secs_f64() > 0.0);
        // Post-seek output must still be near-zero (silence fixture) — proves
        // filter history was reset to a valid silence state, not garbage.
        let batch = source.next_buffer().unwrap().unwrap();
        for s in &batch {
            assert!(s.abs() < 1e-2, "expected near-silence after seek, got {s}");
        }
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn sniff_detects_dsf() {
        let path = write_test_dsf("pawse_dsd_test_sniff.dsf", 2, 2_822_400, 1);
        assert_eq!(sniff(&path), Some(DsdKind::Dsf));
        std::fs::remove_file(&path).ok();
    }
}
