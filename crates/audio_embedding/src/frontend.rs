use std::sync::Arc;

use realfft::{RealFftPlanner, RealToComplex};

pub(crate) const SAMPLE_RATE: u32 = 16_000;
pub(crate) const FRAME: usize = 512;
pub(crate) const HOP: usize = 256;
pub(crate) const BANDS: usize = 96;
pub(crate) const PATCH: usize = 128;
pub(crate) const PATCH_HOP: usize = 62;
pub(crate) const PATCHES: usize = 32;

const HALF: usize = FRAME / 2;

pub(crate) type MelFrame = [f32; BANDS];

pub(crate) struct Resampler {
    step: f64,
    identity: bool,
    window: Vec<f32>,
    base: usize,
    received: usize,
    next: usize,
}

impl Resampler {
    pub(crate) fn new(from: u32) -> Self {
        Self {
            step: from as f64 / SAMPLE_RATE as f64,
            identity: from == SAMPLE_RATE,
            window: Vec::new(),
            base: 0,
            received: 0,
            next: 0,
        }
    }

    pub(crate) fn push(&mut self, input: &[f32], out: &mut Vec<f32>) {
        if self.identity {
            out.extend_from_slice(input);
            return;
        }
        self.window.extend_from_slice(input);
        self.received += input.len();
        let ready = (self.received as f64 / self.step).floor() as usize;
        self.emit(ready, out);
        let keep_from = (self.next as f64 * self.step - 1.0).floor().max(0.0) as usize;
        if keep_from > self.base {
            let drop = (keep_from - self.base).min(self.window.len());
            self.window.drain(..drop);
            self.base += drop;
        }
    }

    pub(crate) fn finish(&mut self, out: &mut Vec<f32>) {
        if self.identity {
            return;
        }
        let total = (self.received as f64 / self.step).floor() as usize;
        self.emit(total, out);
    }

    fn emit(&mut self, until: usize, out: &mut Vec<f32>) {
        while self.next < until {
            let pos = self.next as f64 * self.step - 1.0;
            let idx = pos.floor();
            let frac = (pos - idx) as f32;
            let a = self.at(idx as isize);
            let b = self.at(idx as isize + 1);
            out.push(a + (b - a) * frac);
            self.next += 1;
        }
    }

    fn at(&self, k: isize) -> f32 {
        if k < self.base as isize || k >= self.received as isize {
            return 0.0;
        }
        self.window[k as usize - self.base]
    }
}

#[cfg(test)]
pub(crate) fn resample_linear(x: &[f32], from: u32) -> Vec<f32> {
    let mut resampler = Resampler::new(from);
    let mut out = Vec::new();
    resampler.push(x, &mut out);
    resampler.finish(&mut out);
    out
}

fn hz_to_mel(hz: f32) -> f32 {
    let lin_slope = 3.0 / 200.0;
    if hz < 1000.0 {
        hz * lin_slope
    } else {
        1000.0 * lin_slope + (hz / 1000.0).ln() / (6.4f32.ln() / 27.0)
    }
}

fn mel_to_hz(mel: f32) -> f32 {
    let lin_slope = 3.0 / 200.0;
    let min_log_mel = 1000.0 * lin_slope;
    if mel < min_log_mel {
        mel / lin_slope
    } else {
        1000.0 * ((mel - min_log_mel) * (6.4f32.ln() / 27.0)).exp()
    }
}

pub(crate) struct MelFrontend {
    window: Vec<f32>,
    filters: Vec<(usize, Vec<f32>)>,
    fft: Arc<dyn RealToComplex<f32>>,
}

impl MelFrontend {
    pub(crate) fn new() -> Self {
        let window = (0..FRAME)
            .map(|i| {
                0.5 - 0.5
                    * (2.0 * std::f64::consts::PI * i as f64 / (FRAME as f64 - 1.0)).cos() as f32
            })
            .collect();
        let lo = hz_to_mel(0.0);
        let hi = hz_to_mel(SAMPLE_RATE as f32 / 2.0);
        let inc = (hi - lo) / (BANDS as f32 + 1.0);
        let mut edges = Vec::with_capacity(BANDS + 2);
        let mut mel = lo;
        for _ in 0..BANDS + 2 {
            edges.push(mel_to_hz(mel));
            mel += inc;
        }
        let bin_hz = (SAMPLE_RATE as f32 / 2.0) / (FRAME / 2) as f32;
        let filters = (0..BANDS)
            .map(|band| {
                let (f_lo, f_c, f_hi) = (edges[band], edges[band + 1], edges[band + 2]);
                let (rise, fall) = (f_c - f_lo, f_hi - f_c);
                let first = (f_lo / bin_hz).ceil() as usize;
                let last = ((f_hi / bin_hz).floor() as usize).min(FRAME / 2);
                let norm = (rise + fall) / 2.0;
                let weights = (first..=last)
                    .map(|bin| {
                        let f = bin as f32 * bin_hz;
                        let w = if f < f_c {
                            (f - f_lo) / rise
                        } else {
                            (f_hi - f) / fall
                        };
                        w / norm
                    })
                    .collect();
                (first, weights)
            })
            .collect();
        let fft = RealFftPlanner::<f32>::new().plan_fft_forward(FRAME);
        Self {
            window,
            filters,
            fft,
        }
    }

    #[cfg(test)]
    pub(crate) fn compute(&self, x: &[f32]) -> Vec<MelFrame> {
        let mut stream = MelStream::new(self);
        stream.push(x);
        stream.finish()
    }
}

pub(crate) fn frame_count(samples: usize) -> usize {
    if samples <= HALF {
        1
    } else {
        1 + (samples - HALF).div_ceil(HOP)
    }
}

pub(crate) struct MelStream<'a> {
    frontend: &'a MelFrontend,
    window: Vec<f32>,
    base: usize,
    received: usize,
    frames: Vec<MelFrame>,
    input: Vec<f32>,
    spectrum: Vec<realfft::num_complex::Complex<f32>>,
    scratch: Vec<realfft::num_complex::Complex<f32>>,
    power: Vec<f32>,
}

impl<'a> MelStream<'a> {
    pub(crate) fn new(frontend: &'a MelFrontend) -> Self {
        Self {
            frontend,
            window: Vec::new(),
            base: 0,
            received: 0,
            frames: Vec::new(),
            input: frontend.fft.make_input_vec(),
            spectrum: frontend.fft.make_output_vec(),
            scratch: frontend.fft.make_scratch_vec(),
            power: vec![0.0; FRAME / 2 + 1],
        }
    }

    pub(crate) fn push(&mut self, samples: &[f32]) {
        self.window.extend_from_slice(samples);
        self.received += samples.len();
        while self.frames.len() * HOP + HALF <= self.received {
            self.compute_next();
        }
        let keep_from = (self.frames.len() * HOP).saturating_sub(HALF);
        if keep_from > self.base {
            let drop = (keep_from - self.base).min(self.window.len());
            self.window.drain(..drop);
            self.base += drop;
        }
    }

    pub(crate) fn finish(mut self) -> Vec<MelFrame> {
        while self.frames.len() < frame_count(self.received) {
            self.compute_next();
        }
        self.frames
    }

    fn compute_next(&mut self) {
        let start = (self.frames.len() * HOP) as isize - HALF as isize;
        for (i, slot) in self.input.iter_mut().enumerate() {
            let k = start + i as isize;
            let sample = if k < self.base as isize || k >= self.received as isize {
                0.0
            } else {
                self.window[k as usize - self.base]
            };
            *slot = sample * self.frontend.window[i];
        }
        self.frontend
            .fft
            .process_with_scratch(&mut self.input, &mut self.spectrum, &mut self.scratch)
            .expect("buffers come from the same FFT plan");
        for (p, c) in self.power.iter_mut().zip(&self.spectrum) {
            *p = c.norm_sqr();
        }
        let mut row = [0f32; BANDS];
        for (value, (first, weights)) in row.iter_mut().zip(&self.frontend.filters) {
            let energy: f32 = weights
                .iter()
                .zip(&self.power[*first..])
                .map(|(w, p)| w * p)
                .sum();
            *value = (1.0 + 10000.0 * energy).log10();
        }
        self.frames.push(row);
    }
}

pub(crate) fn patch_starts(frames: usize) -> Vec<usize> {
    if frames <= PATCH {
        return vec![0];
    }
    (0..=(frames - PATCH) / PATCH_HOP)
        .map(|p| p * PATCH_HOP)
        .collect()
}

pub(crate) fn spread(total: usize) -> Vec<usize> {
    if total <= PATCHES {
        return (0..total).collect();
    }
    (0..PATCHES)
        .map(|i| (i * total + total / 2) / PATCHES)
        .collect()
}

pub(crate) fn select_patches(mel: &[MelFrame]) -> (Vec<f32>, usize) {
    let starts = patch_starts(mel.len());
    let chosen = spread(starts.len());
    let mut data = vec![0f32; chosen.len() * PATCH * BANDS];
    for (slot, &ix) in data
        .as_chunks_mut::<{ PATCH * BANDS }>()
        .0
        .iter_mut()
        .zip(&chosen)
    {
        let rows = mel.iter().skip(starts[ix]).take(PATCH);
        for (dst, row) in slot.as_chunks_mut::<BANDS>().0.iter_mut().zip(rows) {
            *dst = *row;
        }
    }
    (data, chosen.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    fn noise(len: usize, seed: u32) -> Vec<f32> {
        let mut state = seed;
        (0..len)
            .map(|_| {
                state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                (state >> 8) as f32 / (1u32 << 24) as f32 * 2.0 - 1.0
            })
            .collect()
    }

    fn reference_resample(x: &[f32], from: u32) -> Vec<f32> {
        let step = from as f64 / SAMPLE_RATE as f64;
        let n = (x.len() as f64 / step).floor() as usize;
        let at = |k: isize| {
            if k >= 0 && (k as usize) < x.len() {
                x[k as usize]
            } else {
                0.0
            }
        };
        (0..n)
            .map(|i| {
                let pos = i as f64 * step - 1.0;
                let idx = pos.floor();
                let frac = (pos - idx) as f32;
                let a = at(idx as isize);
                let b = at(idx as isize + 1);
                a + (b - a) * frac
            })
            .collect()
    }

    #[rstest]
    #[case::cd(44_100)]
    #[case::dat(48_000)]
    #[case::hires(96_000)]
    #[case::low(11_025)]
    fn streaming_resample_matches_the_whole_signal_formula(#[case] rate: u32) {
        let x = noise(rate as usize + 777, 7);
        let expected = reference_resample(&x, rate);
        for chunk in [1, 13, 4096, x.len()] {
            let mut resampler = Resampler::new(rate);
            let mut out = Vec::new();
            for piece in x.chunks(chunk) {
                resampler.push(piece, &mut out);
            }
            resampler.finish(&mut out);
            assert_eq!(out, expected, "chunk {chunk}");
        }
    }

    #[test]
    fn the_resampler_lags_one_input_sample() {
        let x: Vec<f32> = (0..48).map(|i| i as f32).collect();
        let out = resample_linear(&x, 48_000);
        assert_eq!(&out[..4], &[0.0, 2.0, 5.0, 8.0]);
    }

    #[test]
    fn sixteen_khz_input_passes_through() {
        let x = noise(1000, 3);
        assert_eq!(resample_linear(&x, SAMPLE_RATE), x);
    }

    #[rstest]
    #[case::empty(0, 1)]
    #[case::one(1, 1)]
    #[case::half_frame(256, 1)]
    #[case::just_over(257, 2)]
    #[case::two_hops(512, 2)]
    #[case::second(16_000, 63)]
    fn frames_are_centred_on_zero(#[case] samples: usize, #[case] frames: usize) {
        assert_eq!(frame_count(samples), frames);
        assert_eq!(MelFrontend::new().compute(&noise(samples, 1)).len(), frames);
    }

    #[test]
    fn streaming_mel_matches_one_push() {
        let frontend = MelFrontend::new();
        let x = noise(16_000 * 3 + 123, 11);
        let whole = frontend.compute(&x);
        for chunk in [1, 100, 255, 256, 257, 5000] {
            let mut stream = MelStream::new(&frontend);
            for piece in x.chunks(chunk) {
                stream.push(piece);
            }
            assert_eq!(stream.finish(), whole, "chunk {chunk}");
        }
    }

    #[test]
    fn silence_is_zero_mel() {
        let mel = MelFrontend::new().compute(&vec![0.0; 4000]);
        assert!(mel.iter().flatten().all(|&v| v == 0.0));
    }

    #[test]
    fn filters_stay_inside_the_spectrum() {
        let frontend = MelFrontend::new();
        assert_eq!(frontend.filters.len(), BANDS);
        for (first, weights) in &frontend.filters {
            assert!(first + weights.len() <= FRAME / 2 + 1);
            assert!(weights.iter().all(|w| *w >= 0.0));
        }
    }

    #[rstest]
    #[case::short(1, vec![0])]
    #[case::exactly_one(128, vec![0])]
    #[case::one_more_frame(129, vec![0])]
    #[case::second_patch(190, vec![0, 62])]
    #[case::last_partial_dropped(251, vec![0, 62])]
    fn patches_hop_62_frames(#[case] frames: usize, #[case] starts: Vec<usize>) {
        assert_eq!(patch_starts(frames), starts);
    }

    #[rstest]
    #[case::none(0, vec![])]
    #[case::one(1, vec![0])]
    #[case::thirty_two(32, (0..32).collect())]
    #[case::thirty_three(33, (0..16).chain(17..33).collect())]
    #[case::sixty_four(64, (0..32).map(|i| 2 * i + 1).collect())]
    fn spread_takes_32_evenly_placed_patches(#[case] total: usize, #[case] picked: Vec<usize>) {
        assert_eq!(spread(total), picked);
    }

    #[test]
    fn spread_reaches_both_ends_of_a_long_track() {
        let picked = spread(1000);
        assert_eq!(picked.len(), PATCHES);
        assert_eq!(picked[0], 15);
        assert_eq!(picked[31], 984);
        assert!(picked.windows(2).all(|w| w[0] < w[1]));
    }

    #[test]
    fn a_short_track_gets_one_zero_padded_patch() {
        let mel = vec![[1.0f32; BANDS]; 50];
        let (data, count) = select_patches(&mel);
        assert_eq!(count, 1);
        assert_eq!(data.len(), PATCH * BANDS);
        assert!(data[..50 * BANDS].iter().all(|&v| v == 1.0));
        assert!(data[50 * BANDS..].iter().all(|&v| v == 0.0));
    }

    #[test]
    fn selected_patches_copy_their_frames() {
        let mel: Vec<MelFrame> = (0..400).map(|f| [f as f32; BANDS]).collect();
        let (data, count) = select_patches(&mel);
        assert_eq!(count, 5);
        for (p, patch) in data.as_chunks::<{ PATCH * BANDS }>().0.iter().enumerate() {
            assert_eq!(patch[0], (p * PATCH_HOP) as f32);
            assert_eq!(
                patch[(PATCH - 1) * BANDS],
                (p * PATCH_HOP + PATCH - 1) as f32
            );
        }
    }
}
