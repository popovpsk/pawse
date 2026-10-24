use std::f64::consts::PI;

use crate::band::{Band, Kind, MIN_FREQ};

const NYQUIST_GUARD: f64 = 0.49;
const PEAK_STEPS_PER_OCTAVE: f64 = 48.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Coefficients {
    pub b0: f64,
    pub b1: f64,
    pub b2: f64,
    pub a1: f64,
    pub a2: f64,
}

impl Coefficients {
    pub fn design(band: &Band, rate: f64) -> Self {
        let band = band.clamped();
        let freq = band.freq.min(rate * NYQUIST_GUARD);
        let gain = if band.kind.has_gain() { band.gain } else { 0.0 };
        let a = 10f64.powf(gain / 40.0);
        let (sin, cos) = (2.0 * PI * freq / rate).sin_cos();
        let alpha = sin / (2.0 * band.q);
        let shelf = 2.0 * a.sqrt() * alpha;
        let [b0, b1, b2, a0, a1, a2] = match band.kind {
            Kind::Peak => [
                1.0 + alpha * a,
                -2.0 * cos,
                1.0 - alpha * a,
                1.0 + alpha / a,
                -2.0 * cos,
                1.0 - alpha / a,
            ],
            Kind::LowShelf => [
                a * ((a + 1.0) - (a - 1.0) * cos + shelf),
                2.0 * a * ((a - 1.0) - (a + 1.0) * cos),
                a * ((a + 1.0) - (a - 1.0) * cos - shelf),
                (a + 1.0) + (a - 1.0) * cos + shelf,
                -2.0 * ((a - 1.0) + (a + 1.0) * cos),
                (a + 1.0) + (a - 1.0) * cos - shelf,
            ],
            Kind::HighShelf => [
                a * ((a + 1.0) + (a - 1.0) * cos + shelf),
                -2.0 * a * ((a - 1.0) + (a + 1.0) * cos),
                a * ((a + 1.0) + (a - 1.0) * cos - shelf),
                (a + 1.0) - (a - 1.0) * cos + shelf,
                2.0 * ((a - 1.0) - (a + 1.0) * cos),
                (a + 1.0) - (a - 1.0) * cos - shelf,
            ],
            Kind::LowPass => [
                (1.0 - cos) / 2.0,
                1.0 - cos,
                (1.0 - cos) / 2.0,
                1.0 + alpha,
                -2.0 * cos,
                1.0 - alpha,
            ],
            Kind::HighPass => [
                (1.0 + cos) / 2.0,
                -(1.0 + cos),
                (1.0 + cos) / 2.0,
                1.0 + alpha,
                -2.0 * cos,
                1.0 - alpha,
            ],
        };
        Self {
            b0: b0 / a0,
            b1: b1 / a0,
            b2: b2 / a0,
            a1: a1 / a0,
            a2: a2 / a0,
        }
    }

    pub fn magnitude_db(&self, freq: f64, rate: f64) -> f64 {
        let w = 2.0 * PI * freq / rate;
        let (s1, c1) = w.sin_cos();
        let (s2, c2) = (2.0 * w).sin_cos();
        let (nr, ni) = (
            self.b0 + self.b1 * c1 + self.b2 * c2,
            self.b1 * s1 + self.b2 * s2,
        );
        let (dr, di) = (
            1.0 + self.a1 * c1 + self.a2 * c2,
            self.a1 * s1 + self.a2 * s2,
        );
        10.0 * ((nr * nr + ni * ni) / (dr * dr + di * di)).log10()
    }
}

pub struct Response {
    filters: Vec<(f64, Coefficients)>,
    rate: f64,
}

impl Response {
    pub fn new(bands: &[Band], rate: f64) -> Self {
        let filters = bands
            .iter()
            .filter(|band| !band.is_flat())
            .map(|band| (band.clamped().freq, Coefficients::design(band, rate)))
            .collect();
        Self { filters, rate }
    }

    pub fn db(&self, freq: f64) -> f64 {
        self.filters
            .iter()
            .map(|(_, c)| c.magnitude_db(freq, self.rate))
            .sum()
    }

    pub fn peak_db(&self) -> f64 {
        if self.filters.is_empty() {
            return 0.0;
        }
        let top = self.rate * NYQUIST_GUARD;
        let steps = ((top / MIN_FREQ).log2() * PEAK_STEPS_PER_OCTAVE).ceil() as usize;
        let grid = (0..=steps).map(|i| MIN_FREQ * (top / MIN_FREQ).powf(i as f64 / steps as f64));
        let centers = self.filters.iter().map(|(freq, _)| freq.min(top));
        grid.chain(centers)
            .map(|freq| self.db(freq))
            .fold(f64::NEG_INFINITY, f64::max)
    }
}

pub fn preamp_db(bands: &[Band], rate: f64) -> f64 {
    -Response::new(bands, rate).peak_db().max(0.0)
}
