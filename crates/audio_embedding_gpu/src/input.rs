use audio_embedding::{HALF, HOP, PATCH_SAMPLES, PatchInput, Prepared, SAMPLE_RATE};

pub(crate) const INFO_WORDS: usize = 12;
pub(crate) const PCM_CAP: usize = 32_000_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Segment {
    pub(crate) item: usize,
    pub(crate) start: usize,
    pub(crate) len: usize,
}

pub(crate) fn pack(items: &[&Prepared], batch: usize, cap: usize) -> Vec<Vec<Segment>> {
    let sizes: Vec<Vec<usize>> = items
        .iter()
        .map(|item| {
            (0..item.patch_count())
                .map(|p| item.patch(p).samples.len())
                .collect()
        })
        .collect();
    pack_sizes(&sizes, batch, cap)
}

pub(crate) fn pack_sizes(items: &[Vec<usize>], batch: usize, cap: usize) -> Vec<Vec<Segment>> {
    let mut runs = Vec::new();
    let mut run: Vec<Segment> = Vec::new();
    let (mut rows, mut samples) = (0, 0);
    for (item, patches) in items.iter().enumerate() {
        for (ix, &size) in patches.iter().enumerate() {
            if rows == batch || (rows > 0 && samples + size > cap) {
                runs.push(std::mem::take(&mut run));
                (rows, samples) = (0, 0);
            }
            match run.last_mut() {
                Some(last) if last.item == item && last.start + last.len == ix => last.len += 1,
                _ => run.push(Segment {
                    item,
                    start: ix,
                    len: 1,
                }),
            }
            rows += 1;
            samples += size;
        }
    }
    if !run.is_empty() {
        runs.push(run);
    }
    runs
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Ratio {
    pub(crate) p: u64,
    pub(crate) q: u64,
}

pub(crate) fn ratio(rate: u32) -> Ratio {
    let g = gcd(rate as u64, SAMPLE_RATE as u64);
    Ratio {
        p: rate as u64 / g,
        q: SAMPLE_RATE as u64 / g,
    }
}

fn gcd(mut a: u64, mut b: u64) -> u64 {
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a
}

pub(crate) fn supported(rate: u32) -> bool {
    if rate == 0 {
        return false;
    }
    let Ratio { p, q } = ratio(rate);
    let largest = (PATCH_SAMPLES as u64 / q + 1) * p + q * (p + 1);
    largest < u32::MAX as u64 && q * (p + 1) < u32::MAX as u64
}

pub(crate) fn patch_info(
    rate: u32,
    resampled: usize,
    patch: &PatchInput,
    pcm_offset: usize,
) -> [u32; INFO_WORDS] {
    let y0 = (patch.first_frame * HOP) as i64 - HALF as i64;
    let jlo = (-y0).clamp(0, PATCH_SAMPLES as i64);
    let jhi = (resampled as i64 - y0).clamp(0, PATCH_SAMPLES as i64);
    let (base, r0, Ratio { p, q }) = if rate == SAMPLE_RATE {
        (y0 - patch.start as i64, 0, Ratio { p: 1, q: 1 })
    } else {
        let ratio = ratio(rate);
        let n0 = y0 as i128 * ratio.p as i128;
        let a = n0.div_euclid(ratio.q as i128) as i64;
        let r = n0.rem_euclid(ratio.q as i128) as u64;
        (a - 1 - patch.start as i64, r, ratio)
    };
    [
        pcm_offset as u32,
        patch.samples.len() as u32,
        (base as i32) as u32,
        r0 as u32,
        jlo as u32,
        jhi as u32,
        p as u32,
        q as u32,
        patch.frames as u32,
        0,
        0,
        0,
    ]
}

pub(crate) const EMPTY_INFO: [u32; INFO_WORDS] = [0, 0, 0, 0, 0, 0, 1, 1, 0, 0, 0, 0];

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case::cd(44_100, 441, 160)]
    #[case::dat(48_000, 3, 1)]
    #[case::model(16_000, 1, 1)]
    #[case::low(22_050, 441, 320)]
    #[case::hires(192_000, 12, 1)]
    #[case::odd(44_101, 44_101, 16_000)]
    fn rates_reduce_to_a_fraction_of_16_khz(#[case] rate: u32, #[case] p: u64, #[case] q: u64) {
        assert_eq!(ratio(rate), Ratio { p, q });
    }

    #[test]
    fn every_real_rate_fits_the_shader_arithmetic() {
        for rate in [
            8_000, 11_025, 16_000, 22_050, 32_000, 37_800, 44_056, 44_100, 47_250, 48_000, 88_200,
            96_000, 176_400, 192_000, 352_800, 384_000, 705_600, 768_000, 44_101,
        ] {
            assert!(supported(rate), "{rate}");
        }
        assert!(!supported(0));
        assert!(!supported(1_000_003));
    }

    #[test]
    fn runs_hold_at_most_a_batch_and_split_tracks_in_order() {
        let items = vec![vec![1; 40], vec![1; 40], vec![1; 3]];
        let runs = pack_sizes(&items, 64, usize::MAX);
        assert_eq!(
            runs,
            vec![
                vec![
                    Segment {
                        item: 0,
                        start: 0,
                        len: 40
                    },
                    Segment {
                        item: 1,
                        start: 0,
                        len: 24
                    }
                ],
                vec![
                    Segment {
                        item: 1,
                        start: 24,
                        len: 16
                    },
                    Segment {
                        item: 2,
                        start: 0,
                        len: 3
                    }
                ],
            ]
        );
    }

    #[test]
    fn a_run_closes_before_its_samples_pass_the_cap() {
        let items = vec![vec![10; 4], vec![10; 4]];
        let runs = pack_sizes(&items, 64, 25);
        assert_eq!(runs.len(), 4);
        assert!(
            runs.iter()
                .all(|r| r.iter().map(|s| s.len).sum::<usize>() == 2)
        );
        let single = pack_sizes(&[vec![100]], 64, 25);
        assert_eq!(
            single,
            vec![vec![Segment {
                item: 0,
                start: 0,
                len: 1
            }]]
        );
    }

    #[test]
    fn nothing_packs_into_nothing() {
        assert!(pack_sizes(&[], 64, 100).is_empty());
        assert!(pack_sizes(&[vec![]], 64, 100).is_empty());
    }
}
