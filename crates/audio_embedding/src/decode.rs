use std::time::Duration;

use audio_common::{AudioError, AudioSamples, AudioSource};

use crate::{EmbedError, TrackRange};

const MAX_ANALYSED: Duration = Duration::from_secs(30 * 60);

fn decode_error(e: AudioError) -> EmbedError {
    EmbedError::Decode(e.to_string())
}

fn frames_in(duration: Duration, rate: u32) -> u64 {
    (duration.as_nanos() * rate as u128 / 1_000_000_000) as u64
}

fn mix_down(samples: &AudioSamples, channels: usize, out: &mut Vec<f32>) {
    let scale = channels as f32;
    match samples {
        AudioSamples::S16(v) => out.extend(
            v.chunks_exact(channels)
                .map(|f| f.iter().map(|&s| s as f32 / 32_768.0).sum::<f32>() / scale),
        ),
        AudioSamples::S24(v) => out.extend(v.chunks_exact(channels).map(|f| {
            f.iter()
                .map(|s| s.into_i32() as f32 / 8_388_608.0)
                .sum::<f32>()
                / scale
        })),
        AudioSamples::S32(v) => out.extend(
            v.chunks_exact(channels)
                .map(|f| f.iter().map(|&s| s as f32 / 2_147_483_648.0).sum::<f32>() / scale),
        ),
        AudioSamples::F32(v) => out.extend(v.chunks_exact(channels).map(|f| {
            f.iter()
                .map(|&s| if s.is_finite() { s } else { 0.0 })
                .sum::<f32>()
                / scale
        })),
    }
}

pub(crate) fn read_mono(
    source: &mut dyn AudioSource,
    range: Option<TrackRange>,
    mut sink: impl FnMut(&[f32]),
) -> Result<u64, EmbedError> {
    let rate = source.params().sample_rate;
    if rate == 0 {
        return Err(EmbedError::Decode(
            "the source reports no sample rate".into(),
        ));
    }
    let mut skip = 0;
    if let Some(start) = range.map(|r| r.start).filter(|s| !s.is_zero()) {
        skip = match source.duration().filter(|d| !d.is_zero()) {
            Some(total) => {
                let fraction = (start.as_secs_f64() / total.as_secs_f64()) as f32;
                let landed = source.seek(fraction).map_err(decode_error)?;
                frames_in(start.saturating_sub(landed), rate)
            }
            None => frames_in(start, rate),
        };
    }
    let length = range
        .and_then(|r| r.length)
        .map_or(MAX_ANALYSED, |l| l.min(MAX_ANALYSED));
    let limit = frames_in(length, rate);
    let mut taken = 0;
    let mut mono = Vec::new();
    while taken < limit {
        let Some(batch) = source.next_buffer().map_err(decode_error)? else {
            break;
        };
        let channels = batch.metadata.channels.to_u8().max(1) as usize;
        mono.clear();
        mix_down(&batch.data, channels, &mut mono);
        let mut chunk = &mono[..];
        if skip > 0 {
            let n = skip.min(chunk.len() as u64) as usize;
            skip -= n as u64;
            chunk = &chunk[n..];
        }
        let room = (limit - taken).min(chunk.len() as u64) as usize;
        chunk = &chunk[..room];
        taken += chunk.len() as u64;
        if !chunk.is_empty() {
            sink(chunk);
        }
    }
    Ok(taken)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use audio_common::{AudioBatch, ChannelCount, I24, Metadata, StreamParams};

    pub(crate) struct FakeSource {
        pub(crate) rate: u32,
        pub(crate) channels: u8,
        pub(crate) samples: Vec<f32>,
        pub(crate) batch: usize,
        pub(crate) position: usize,
        pub(crate) seek_granule: usize,
        pub(crate) report_duration: bool,
    }

    impl FakeSource {
        pub(crate) fn mono(rate: u32, samples: Vec<f32>) -> Self {
            Self {
                rate,
                channels: 1,
                samples,
                batch: 1152,
                position: 0,
                seek_granule: 1,
                report_duration: true,
            }
        }

        fn frames(&self) -> usize {
            self.samples.len() / self.channels as usize
        }
    }

    impl AudioSource for FakeSource {
        fn params(&self) -> StreamParams {
            StreamParams::new(self.rate, ChannelCount::from_u8(self.channels), 32)
        }

        fn next_buffer(&mut self) -> Result<Option<AudioBatch>, AudioError> {
            let ch = self.channels as usize;
            if self.position >= self.frames() {
                return Ok(None);
            }
            let end = (self.position + self.batch).min(self.frames());
            let data = self.samples[self.position * ch..end * ch].to_vec();
            self.position = end;
            Ok(Some(AudioBatch {
                data: AudioSamples::F32(data),
                metadata: Metadata {
                    sample_rate: self.rate,
                    channels: ChannelCount::from_u8(self.channels),
                    bit_depth: 32,
                },
            }))
        }

        fn seek(&mut self, position: f32) -> Result<Duration, AudioError> {
            let target = (position as f64 * self.frames() as f64) as usize;
            self.position = target / self.seek_granule * self.seek_granule;
            Ok(Duration::from_secs_f64(
                self.position as f64 / self.rate as f64,
            ))
        }

        fn duration(&self) -> Option<Duration> {
            self.report_duration
                .then(|| Duration::from_secs_f64(self.frames() as f64 / self.rate as f64))
        }
    }

    fn ramp(len: usize) -> Vec<f32> {
        (0..len).map(|i| i as f32).collect()
    }

    fn collect(source: &mut dyn AudioSource, range: Option<TrackRange>) -> Vec<f32> {
        let mut out = Vec::new();
        let taken = read_mono(source, range, |chunk| out.extend_from_slice(chunk)).unwrap();
        assert_eq!(taken as usize, out.len());
        out
    }

    #[test]
    fn a_whole_file_is_read_to_the_end() {
        let mut source = FakeSource::mono(1000, ramp(5000));
        assert_eq!(collect(&mut source, None), ramp(5000));
    }

    #[test]
    fn a_cue_range_starts_exactly_even_when_the_seek_lands_early() {
        let mut source = FakeSource::mono(1000, ramp(10_000));
        source.seek_granule = 1152;
        let range = TrackRange {
            start: Duration::from_millis(3_500),
            length: Some(Duration::from_millis(2_000)),
        };
        let out = collect(&mut source, Some(range));
        assert_eq!(out, (3_500..5_500).map(|i| i as f32).collect::<Vec<_>>());
    }

    #[test]
    fn a_source_without_duration_skips_to_the_range_by_decoding() {
        let mut source = FakeSource::mono(1000, ramp(10_000));
        source.report_duration = false;
        let range = TrackRange {
            start: Duration::from_millis(9_000),
            length: None,
        };
        let out = collect(&mut source, Some(range));
        assert_eq!(out, (9_000..10_000).map(|i| i as f32).collect::<Vec<_>>());
    }

    #[test]
    fn a_range_at_zero_stops_at_its_length() {
        let mut source = FakeSource::mono(1000, ramp(10_000));
        let range = TrackRange {
            start: Duration::ZERO,
            length: Some(Duration::from_millis(1_234)),
        };
        assert_eq!(collect(&mut source, Some(range)), ramp(1_234));
    }

    #[test]
    fn analysis_stops_after_thirty_minutes() {
        let rate = 10;
        let mut source = FakeSource::mono(rate, ramp(31 * 60 * rate as usize));
        assert_eq!(collect(&mut source, None).len(), 30 * 60 * rate as usize);
        let mut source = FakeSource::mono(rate, ramp(31 * 60 * rate as usize));
        let range = TrackRange {
            start: Duration::from_secs(10),
            length: Some(Duration::from_secs(40 * 60)),
        };
        let out = collect(&mut source, Some(range));
        assert_eq!(out.len(), 30 * 60 * rate as usize);
        assert_eq!(out[0], 100.0);
    }

    #[test]
    fn non_finite_samples_become_silence() {
        let mut source = FakeSource::mono(1000, vec![f32::NAN, 0.5, f32::INFINITY, -0.25]);
        assert_eq!(collect(&mut source, None), vec![0.0, 0.5, 0.0, -0.25]);
    }

    #[test]
    fn channels_are_averaged() {
        let mut source = FakeSource::mono(1000, vec![1.0, 0.0, 0.5, 0.5, -1.0, 0.0]);
        source.channels = 2;
        assert_eq!(collect(&mut source, None), vec![0.5, 0.5, -0.5]);
    }

    #[test]
    fn integer_samples_are_scaled_to_unit_range() {
        let mut out = Vec::new();
        mix_down(&AudioSamples::S16(vec![-32_768, 16_384]), 1, &mut out);
        mix_down(
            &AudioSamples::S24(vec![I24::new(8_388_607), I24::new(-4_194_304)]),
            1,
            &mut out,
        );
        mix_down(&AudioSamples::S32(vec![i32::MIN, 0]), 1, &mut out);
        assert_eq!(out[0], -1.0);
        assert_eq!(out[1], 0.5);
        assert!((out[2] - 1.0).abs() < 1e-6);
        assert_eq!(out[3], -0.5);
        assert_eq!(out[4], -1.0);
        assert_eq!(out[5], 0.0);
    }
}
