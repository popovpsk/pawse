use std::ops::Range;

use audio_common::AudioSource;

use crate::decode::{analysed_limit, decode_error, push_mono, seek_start};
use crate::frontend::{
    BANDS, HALF, HOP, MelFrontend, MelScratch, PATCH, SAMPLE_RATE, frame_count, patch_starts,
    resampled_at, resampled_len, spread, step_for,
};
use crate::{EmbedError, Prepared, TrackRange};

const MAX_COVERAGE: f64 = 0.6;

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Plan {
    pub(crate) len: u64,
    pub(crate) resampled: usize,
    pub(crate) patches: Vec<usize>,
    pub(crate) ranges: Vec<Range<u64>>,
}

pub(crate) fn plan(len: u64, rate: u32) -> Option<Plan> {
    if len < rate as u64 {
        return None;
    }
    let resampled = resampled_len(len as usize, rate);
    let frames = frame_count(resampled);
    let starts = patch_starts(frames);
    let patches: Vec<usize> = spread(starts.len())
        .into_iter()
        .map(|ix| starts[ix])
        .collect();
    if patches.iter().any(|&first| first + PATCH > frames) {
        return None;
    }
    let mut ranges: Vec<Range<u64>> = Vec::with_capacity(patches.len());
    for &first in &patches {
        let needed = source_range(first, resampled, len, rate);
        match ranges.last_mut() {
            Some(last) if needed.start <= last.end => last.end = last.end.max(needed.end),
            _ => ranges.push(needed),
        }
    }
    let covered: u64 = ranges.iter().map(|r| r.end - r.start).sum();
    if covered as f64 > MAX_COVERAGE * len as f64 {
        return None;
    }
    Some(Plan {
        len,
        resampled,
        patches,
        ranges,
    })
}

fn resampled_range(first: usize, resampled: usize) -> Range<usize> {
    let start = (first * HOP).saturating_sub(HALF);
    let end = ((first + PATCH - 1) * HOP + HALF).min(resampled);
    start.min(end)..end
}

fn source_range(first: usize, resampled: usize, len: u64, rate: u32) -> Range<u64> {
    let needed = resampled_range(first, resampled);
    if needed.is_empty() {
        return 0..0;
    }
    if rate == SAMPLE_RATE {
        return needed.start as u64..needed.end as u64;
    }
    let step = step_for(rate);
    let lo = (needed.start as f64 * step - 1.0).floor().max(0.0) as u64;
    let hi = ((needed.end - 1) as f64 * step - 1.0).floor() as u64 + 2;
    lo.min(len)..hi.min(len)
}

pub(crate) fn prepare(
    source: &mut dyn AudioSource,
    range: Option<TrackRange>,
) -> Result<Option<Prepared>, EmbedError> {
    let Some(total) = source.exact_frames() else {
        return Ok(None);
    };
    let rate = source.params().sample_rate;
    if rate == 0 {
        return Ok(None);
    }
    let start = seek_start(source, range, rate)?;
    let first = start.landed.map_or(0, |landed| {
        (landed.as_secs_f64() * rate as f64).round() as u64
    }) + start.skip;
    let Some(available) = total.checked_sub(first) else {
        return Ok(None);
    };
    let Some(plan) = plan(available.min(analysed_limit(range, rate)), rate) else {
        return Ok(None);
    };
    match windowed(source, first, total, rate, &plan) {
        Ok(Some(prepared)) => Ok(Some(prepared)),
        Ok(None) | Err(_) => restore(source, range),
    }
}

fn restore(
    source: &mut dyn AudioSource,
    range: Option<TrackRange>,
) -> Result<Option<Prepared>, EmbedError> {
    let from_start = range.is_none_or(|r| r.start.is_zero());
    if from_start && source.seek_frame(0).map_err(decode_error)? != 0 {
        return Err(EmbedError::Decode(
            "could not return to the start of the track".into(),
        ));
    }
    Ok(None)
}

fn windowed(
    source: &mut dyn AudioSource,
    first: u64,
    total: u64,
    rate: u32,
    plan: &Plan,
) -> Result<Option<Prepared>, EmbedError> {
    if !ends_at(source, total)? {
        return Ok(None);
    }
    let frontend = MelFrontend::new();
    let mut buffers = MelScratch::new(&frontend);
    let mut patches = vec![0f32; plan.patches.len() * PATCH * BANDS];
    let mut slots = patches.as_chunks_mut::<{ PATCH * BANDS }>().0.iter_mut();
    let mut pending = plan.patches.iter().copied().peekable();
    let mut y = Vec::new();
    let step = step_for(rate);
    for range in &plan.ranges {
        let Some(x) = read(source, first + range.start, range.end - range.start)? else {
            return Ok(None);
        };
        while let Some(&patch) = pending.peek() {
            if source_range(patch, plan.resampled, plan.len, rate).end > range.end {
                break;
            }
            pending.next();
            let ys = resampled_range(patch, plan.resampled);
            let at = |k: isize| -> f32 {
                usize::try_from(k)
                    .ok()
                    .and_then(|k| k.checked_sub(range.start as usize))
                    .and_then(|k| x.get(k))
                    .copied()
                    .unwrap_or(0.0)
            };
            y.clear();
            y.extend(ys.clone().map(|i| {
                if rate == SAMPLE_RATE {
                    at(i as isize)
                } else {
                    resampled_at(step, i, at)
                }
            }));
            let Some(slot) = slots.next() else {
                return Ok(None);
            };
            for (row, dst) in slot.as_chunks_mut::<BANDS>().0.iter_mut().enumerate() {
                *dst = frontend.frame(&mut buffers, patch + row, |k| {
                    if k < ys.start as isize || k >= ys.end as isize {
                        0.0
                    } else {
                        y[k as usize - ys.start]
                    }
                });
            }
        }
    }
    if pending.next().is_some() {
        return Ok(None);
    }
    Ok(Some(Prepared {
        patches,
        count: plan.patches.len(),
    }))
}

fn ends_at(source: &mut dyn AudioSource, total: u64) -> Result<bool, EmbedError> {
    let Some(last) = total.checked_sub(1) else {
        return Ok(false);
    };
    let landed = source.seek_frame(last).map_err(decode_error)?;
    if landed > last {
        return Ok(false);
    }
    let mut seen = landed;
    let mut mono = Vec::new();
    while seen < total {
        let Some(batch) = source.next_buffer().map_err(decode_error)? else {
            return Ok(false);
        };
        mono.clear();
        push_mono(&batch, &mut mono);
        seen += mono.len() as u64;
    }
    Ok(seen == total)
}

fn read(source: &mut dyn AudioSource, from: u64, len: u64) -> Result<Option<Vec<f32>>, EmbedError> {
    let landed = source.seek_frame(from).map_err(decode_error)?;
    if landed > from {
        return Ok(None);
    }
    let mut skip = from - landed;
    let mut out = Vec::with_capacity(len as usize);
    let mut mono = Vec::new();
    while (out.len() as u64) < len {
        let Some(batch) = source.next_buffer().map_err(decode_error)? else {
            return Ok(None);
        };
        mono.clear();
        push_mono(&batch, &mut mono);
        let mut chunk = &mono[..];
        if skip > 0 {
            let n = skip.min(chunk.len() as u64) as usize;
            skip -= n as u64;
            chunk = &chunk[n..];
        }
        let room = (len - out.len() as u64).min(chunk.len() as u64) as usize;
        out.extend_from_slice(&chunk[..room]);
    }
    Ok(Some(out))
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use rstest::rstest;

    use super::*;
    use crate::decode::tests::FakeSource;
    use crate::oracle_tests::signal;
    use crate::{Job, prepare as prepare_job};

    fn source(rate: u32, channels: u8, seconds: usize) -> FakeSource {
        let mono = signal(rate, rate as usize * seconds);
        let samples = mono
            .iter()
            .flat_map(|&s| (0..channels).map(move |c| s * (1.0 - 0.3 * c as f32)))
            .collect();
        let mut source = FakeSource::mono(rate, samples);
        source.channels = channels;
        source
    }

    fn full(mut source: FakeSource, range: Option<TrackRange>) -> Prepared {
        source.exact = None;
        prepare_job(Job {
            source: Box::new(source),
            range,
        })
        .unwrap()
    }

    #[rstest]
    #[case::cd_stereo(44_100, 2, 1152, 140, None)]
    #[case::model_rate(16_000, 1, 4096, 130, None)]
    #[case::low_rate(22_050, 1, 1, 135, None)]
    #[case::hi_res(96_000, 1, 4096, 125, None)]
    #[case::cue_track(44_100, 1, 4096, 260, Some((37_300, Some(150_000))))]
    #[case::last_cue_track(48_000, 2, 1152, 200, Some((61_777, None)))]
    fn windows_give_the_same_patches_as_a_full_decode(
        #[case] rate: u32,
        #[case] channels: u8,
        #[case] granule: usize,
        #[case] seconds: usize,
        #[case] range: Option<(u64, Option<u64>)>,
    ) {
        let range = range.map(|(start, length)| TrackRange {
            start: Duration::from_millis(start),
            length: length.map(Duration::from_millis),
        });
        let mut windowed = source(rate, channels, seconds);
        windowed.seek_granule = granule;
        windowed.exact = Some((rate as usize * seconds) as u64);
        let expected = full(source(rate, channels, seconds), range);

        let got = prepare(&mut windowed, range)
            .unwrap()
            .expect("windows apply");
        assert_eq!(got.count, expected.count);
        assert!(got.patches == expected.patches);
    }

    #[test]
    fn a_track_shorter_than_its_header_says_falls_back_to_a_full_decode() {
        let rate = 44_100;
        let mut truncated = source(rate, 1, 150);
        truncated.exact = Some(rate as u64 * 160);
        truncated.seek_granule = 4096;
        assert!(prepare(&mut truncated, None).unwrap().is_none());
        assert_eq!(truncated.position, 0);

        let mut truncated = source(rate, 1, 150);
        truncated.exact = Some(rate as u64 * 160);
        let got = prepare_job(Job {
            source: Box::new(truncated),
            range: None,
        })
        .unwrap();
        assert!(got.patches == full(source(rate, 1, 150), None).patches);
    }

    #[test]
    fn a_short_track_is_decoded_whole() {
        let mut short = source(44_100, 1, 90);
        short.exact = Some(44_100 * 90);
        assert!(prepare(&mut short, None).unwrap().is_none());
        assert!(plan(44_100 * 90, 44_100).is_none());
    }

    #[test]
    fn a_four_minute_track_reads_about_a_quarter_of_its_samples() {
        let len = 44_100 * 240;
        let plan = plan(len, 44_100).unwrap();
        assert_eq!(plan.patches.len(), 32);
        let covered: u64 = plan.ranges.iter().map(|r| r.end - r.start).sum();
        let share = covered as f64 / len as f64;
        assert!((0.25..0.3).contains(&share), "{share}");
        assert!(plan.ranges.windows(2).all(|w| w[0].end < w[1].start));
    }
}

#[cfg(test)]
mod real_files {
    use std::path::PathBuf;
    use std::time::Duration;

    use audio_common::{AudioBatch, AudioError, StreamParams};
    use audio_decoder::Decoder;
    use rstest::rstest;

    use super::*;
    use crate::{Job, prepare as prepare_job};

    struct Plain(Decoder);

    impl AudioSource for Plain {
        fn params(&self) -> StreamParams {
            self.0.params()
        }

        fn next_buffer(&mut self) -> Result<Option<AudioBatch>, AudioError> {
            self.0.next_buffer()
        }

        fn seek(&mut self, position: f32) -> Result<Duration, AudioError> {
            self.0.seek(position)
        }

        fn duration(&self) -> Option<Duration> {
            self.0.duration()
        }
    }

    fn open(name: &str) -> Decoder {
        let path = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap())
            .join("../../fixtures")
            .join(name);
        Decoder::open(&path).unwrap_or_else(|e| panic!("{name} (run `make generate`): {e}"))
    }

    #[rstest]
    #[case::flac("long_44_stereo.flac", None)]
    #[case::alac("long_44_stereo.m4a", None)]
    #[case::wav("long_22_mono.wav", None)]
    #[case::flac_cue_track("long_44_stereo.flac", Some(3_210))]
    fn windows_of_a_real_file_match_its_full_decode(
        #[case] name: &str,
        #[case] start_ms: Option<u64>,
    ) {
        let range = start_ms.map(|ms| TrackRange {
            start: Duration::from_millis(ms),
            length: None,
        });
        let mut windowed = open(name);
        assert!(windowed.exact_frames().is_some());
        let got = prepare(&mut windowed, range)
            .unwrap()
            .expect("windows apply");
        let full = prepare_job(Job {
            source: Box::new(Plain(open(name))),
            range,
        })
        .unwrap();
        assert_eq!(got.count, full.count);
        assert!(got.patches == full.patches);
    }

    #[test]
    fn a_flac_whose_frames_do_not_start_at_zero_takes_the_full_pass() {
        assert!(open("long_44_cut.flac").exact_frames().is_none());
        let prepared = prepare_job(Job {
            source: Box::new(open("long_44_cut.flac")),
            range: None,
        });
        assert!(prepared.is_ok());
    }
}
