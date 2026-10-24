use serde::{Deserialize, Serialize};

pub const MIN_FREQ: f64 = 10.0;
pub const MAX_FREQ: f64 = 22_000.0;
pub const MIN_Q: f64 = 0.1;
pub const MAX_Q: f64 = 20.0;
pub const MAX_GAIN: f64 = 30.0;
pub const MAX_BANDS: usize = 32;
pub const DEFAULT_Q: f64 = std::f64::consts::FRAC_1_SQRT_2;

const FLAT_GAIN: f64 = 0.005;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Peak,
    LowShelf,
    HighShelf,
    LowPass,
    HighPass,
}

impl Kind {
    pub const ALL: [Kind; 5] = [
        Kind::Peak,
        Kind::LowShelf,
        Kind::HighShelf,
        Kind::LowPass,
        Kind::HighPass,
    ];

    pub fn has_gain(self) -> bool {
        !matches!(self, Kind::LowPass | Kind::HighPass)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Band {
    pub kind: Kind,
    pub freq: f64,
    #[serde(default)]
    pub gain: f64,
    pub q: f64,
    #[serde(default = "enabled")]
    pub enabled: bool,
}

fn enabled() -> bool {
    true
}

impl Band {
    pub fn new(kind: Kind, freq: f64, gain: f64, q: f64) -> Self {
        Self {
            kind,
            freq,
            gain,
            q,
            enabled: true,
        }
    }

    pub fn clamped(self) -> Self {
        Self {
            freq: finite_or(self.freq, 1000.0).clamp(MIN_FREQ, MAX_FREQ),
            gain: finite_or(self.gain, 0.0).clamp(-MAX_GAIN, MAX_GAIN),
            q: finite_or(self.q, DEFAULT_Q).clamp(MIN_Q, MAX_Q),
            ..self
        }
    }

    pub fn is_flat(&self) -> bool {
        !self.enabled
            || (self.kind.has_gain() && (self.gain.is_nan() || self.gain.abs() < FLAT_GAIN))
    }
}

fn finite_or(value: f64, fallback: f64) -> f64 {
    if value.is_finite() { value } else { fallback }
}
