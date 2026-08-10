use std::f64::consts::PI;

/// Windowed-sinc halfband lowpass (cutoff at fs/4), Blackman-windowed.
/// Halfband property: every tap at an even offset from the center is
/// exactly zero (the ideal sinc has a zero there); we don't bother
/// skipping them in the convolution since the total cost is trivial at
/// these sample rates, but the property is why a single filter this small
/// gives a clean decimate-by-2 stopband.
fn design_halfband(num_taps: usize) -> Vec<f32> {
    assert!(num_taps % 2 == 1, "halfband filter needs an odd tap count");
    let center = (num_taps - 1) as f64 / 2.0;
    let mut taps = vec![0f64; num_taps];

    for (n, tap) in taps.iter_mut().enumerate() {
        let k = n as f64 - center;
        let ideal = if k == 0.0 {
            0.5
        } else if (k as i64) % 2 != 0 {
            let x = 0.5 * k;
            (PI * x).sin() / (PI * x) * 0.5
        } else {
            0.0
        };
        let window = 0.42 - 0.5 * (2.0 * PI * n as f64 / (num_taps - 1) as f64).cos()
            + 0.08 * (4.0 * PI * n as f64 / (num_taps - 1) as f64).cos();
        *tap = ideal * window;
    }

    let dc_gain: f64 = taps.iter().sum();
    taps.into_iter().map(|t| (t / dc_gain) as f32).collect()
}

/// Stateful decimate-by-2 halfband lowpass for one channel. Emits one
/// output sample for every two input samples, carrying both filter history
/// and decimation phase across calls so streaming input in arbitrary-sized
/// chunks doesn't glitch or drift at chunk boundaries.
pub struct HalfbandDecimator {
    taps: Vec<f32>,
    ring: Vec<f32>,
    pos: usize,
    emit: bool,
}

impl HalfbandDecimator {
    pub fn new(num_taps: usize) -> Self {
        Self {
            taps: design_halfband(num_taps),
            ring: vec![0.0; num_taps],
            pos: 0,
            emit: true,
        }
    }

    pub fn reset(&mut self) {
        self.ring.iter_mut().for_each(|s| *s = 0.0);
        self.pos = 0;
        self.emit = true;
    }

    pub fn process(&mut self, input: &[f32], output: &mut Vec<f32>) {
        let n = self.taps.len();
        for &sample in input {
            self.ring[self.pos] = sample;
            self.pos = (self.pos + 1) % n;

            if self.emit {
                let mut acc = 0f32;
                for (i, &coeff) in self.taps.iter().enumerate() {
                    let idx = (self.pos + n - 1 - i) % n;
                    acc += coeff * self.ring[idx];
                }
                output.push(acc);
            }
            self.emit = !self.emit;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TAPS: usize = 31;

    #[test]
    fn halves_output_length() {
        let mut d = HalfbandDecimator::new(TAPS);
        let mut out = Vec::new();
        d.process(&vec![0.3f32; 200], &mut out);
        assert_eq!(out.len(), 100);
    }

    #[test]
    fn output_length_stays_consistent_across_odd_sized_chunks() {
        let mut d = HalfbandDecimator::new(TAPS);
        let mut out = Vec::new();
        for chunk_len in [7, 13, 5, 41, 3] {
            d.process(&vec![0.1f32; chunk_len], &mut out);
        }
        // `emit` starts `true`, so the very first processed sample always
        // emits — an odd running total rounds *up*, not down.
        let total: usize = [7, 13, 5, 41, 3].iter().sum();
        assert_eq!(out.len(), total.div_ceil(2));
    }

    #[test]
    fn dc_signal_passes_through_near_unity_gain() {
        let mut d = HalfbandDecimator::new(TAPS);
        let mut out = Vec::new();
        d.process(&vec![0.5f32; 400], &mut out);
        for &s in &out[100..] {
            assert!((s - 0.5).abs() < 1e-3, "expected ~0.5, got {s}");
        }
    }

    #[test]
    fn near_nyquist_tone_is_attenuated() {
        // Alternating +1/-1 at the *input* sample rate is right at the old
        // Nyquist, well above the new one (fs/4) — must be crushed.
        let mut d = HalfbandDecimator::new(TAPS);
        let input: Vec<f32> = (0..400).map(|i| if i % 2 == 0 { 1.0 } else { -1.0 }).collect();
        let mut out = Vec::new();
        d.process(&input, &mut out);
        for &s in &out[50..] {
            assert!(s.abs() < 1e-2, "expected strong attenuation, got {s}");
        }
    }

    #[test]
    fn cascading_twice_quarters_the_rate() {
        let mut stage1 = HalfbandDecimator::new(TAPS);
        let mut stage2 = HalfbandDecimator::new(TAPS);
        let mut mid = Vec::new();
        stage1.process(&vec![0.2f32; 400], &mut mid);
        let mut out = Vec::new();
        stage2.process(&mid, &mut out);
        assert_eq!(out.len(), 100);
    }
}
