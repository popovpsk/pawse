use std::sync::Arc;

use realfft::num_complex::Complex;
use realfft::{RealFftPlanner, RealToComplex};

pub const MIN_HZ: f32 = 20.0;
pub const MAX_HZ: f32 = 20_000.0;
pub const MAX_WINDOW: usize = 32_768;
pub const SILENCE_DB: f32 = -200.0;

const MIN_WINDOW: usize = 4_096;
const WINDOW_SECONDS: f32 = 0.17;
const ATTACK_SECONDS: f32 = 0.012;
const PEAK_HOLD_SECONDS: f32 = 1.4;
const PEAK_FALL_DB_PER_SECOND: f32 = 14.0;
const ZONE_SECONDS: f32 = 0.35;
const SETTLED_DB: f32 = -120.0;
const SETTLED_ZONE_POWER: f64 = 1e-9;
const MAX_STEP_SECONDS: f32 = 0.25;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Zone {
    pub name: &'static str,
    pub low: f32,
    pub high: f32,
}

pub const ZONES: [Zone; 6] = [
    Zone {
        name: "SUB",
        low: 20.0,
        high: 60.0,
    },
    Zone {
        name: "BASS",
        low: 60.0,
        high: 250.0,
    },
    Zone {
        name: "LOW MID",
        low: 250.0,
        high: 500.0,
    },
    Zone {
        name: "MID",
        low: 500.0,
        high: 2_000.0,
    },
    Zone {
        name: "HIGH MID",
        low: 2_000.0,
        high: 6_000.0,
    },
    Zone {
        name: "AIR",
        low: 6_000.0,
        high: 20_000.0,
    },
];

pub fn position(hz: f32) -> f32 {
    (hz / MIN_HZ).log2() / (MAX_HZ / MIN_HZ).log2()
}

pub fn frequency(position: f32) -> f32 {
    MIN_HZ * (MAX_HZ / MIN_HZ).powf(position)
}

pub fn zone_of(hz: f32) -> &'static Zone {
    ZONES
        .iter()
        .find(|zone| hz < zone.high)
        .unwrap_or(&ZONES[ZONES.len() - 1])
}

pub fn window_len(sample_rate: u32) -> usize {
    ((sample_rate as f32 * WINDOW_SECONDS) as usize)
        .next_power_of_two()
        .clamp(MIN_WINDOW, MAX_WINDOW)
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Settings {
    pub smoothing_octaves: f32,
    pub tilt_db_per_octave: f32,
    pub release_seconds: f32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            smoothing_octaves: 1.0 / 6.0,
            tilt_db_per_octave: 4.5,
            release_seconds: 0.2,
        }
    }
}

struct Transform {
    fft: Arc<dyn RealToComplex<f32>>,
    window: Vec<f32>,
    input: Vec<f32>,
    output: Vec<Complex<f32>>,
    scratch: Vec<Complex<f32>>,
    calibration_db: f32,
    window_power: f64,
}

impl Transform {
    fn new(planner: &mut RealFftPlanner<f32>, len: usize) -> Self {
        let fft = planner.plan_fft_forward(len);
        let window: Vec<f32> = (0..len).map(|n| blackman(n, len)).collect();
        let sum: f64 = window.iter().map(|&w| f64::from(w)).sum();
        let window_power = window
            .iter()
            .map(|&w| f64::from(w) * f64::from(w))
            .sum::<f64>()
            / len as f64;
        Self {
            input: fft.make_input_vec(),
            output: fft.make_output_vec(),
            scratch: fft.make_scratch_vec(),
            calibration_db: (-20.0 * (sum / (2.0 * len as f64)).log10()) as f32,
            window_power,
            window,
            fft,
        }
    }
}

fn blackman(n: usize, len: usize) -> f32 {
    let x = 2.0 * std::f64::consts::PI * n as f64 / len as f64;
    (0.42 - 0.5 * x.cos() + 0.08 * (2.0 * x).cos()) as f32
}

fn to_db(power: f64) -> f32 {
    if power > 0.0 {
        ((10.0 * power.log10()) as f32).max(SILENCE_DB)
    } else {
        SILENCE_DB
    }
}

fn resample(values: &[f32], count: usize, empty: f32) -> Vec<f32> {
    if values.len() < 2 {
        return vec![empty; count];
    }
    let from_last = (values.len() - 1) as f32;
    let to_last = count.saturating_sub(1).max(1) as f32;
    (0..count)
        .map(|i| {
            let x = i as f32 / to_last * from_last;
            let lo = (x as usize).min(values.len() - 2);
            values[lo] + (values[lo + 1] - values[lo]) * (x - lo as f32)
        })
        .collect()
}

fn approach(current: f64, target: f64, dt: f32, seconds: f32) -> f64 {
    current + (target - current) * f64::from(1.0 - (-dt / seconds).exp())
}

pub struct Analyzer {
    planner: RealFftPlanner<f32>,
    transform: Option<Transform>,
    signal: Vec<f32>,
    power: Vec<f64>,
    cumulative: Vec<f64>,
    bin_hz: f64,
    nyquist: f32,
    settings: Settings,
    frequencies: Vec<f32>,
    tilt: Vec<f32>,
    levels: Vec<f32>,
    peaks: Vec<f32>,
    holds: Vec<f32>,
    zones: [f64; ZONES.len()],
}

impl Default for Analyzer {
    fn default() -> Self {
        Self::new()
    }
}

impl Analyzer {
    pub fn new() -> Self {
        Self {
            planner: RealFftPlanner::new(),
            transform: None,
            signal: vec![0.0; MAX_WINDOW],
            power: Vec::new(),
            cumulative: Vec::new(),
            bin_hz: 1.0,
            nyquist: 0.0,
            settings: Settings::default(),
            frequencies: Vec::new(),
            tilt: Vec::new(),
            levels: Vec::new(),
            peaks: Vec::new(),
            holds: Vec::new(),
            zones: [0.0; ZONES.len()],
        }
    }

    pub fn signal_mut(&mut self) -> &mut [f32] {
        &mut self.signal
    }

    pub fn signal_tail_mut(&mut self, sample_rate: u32) -> &mut [f32] {
        let len = window_len(sample_rate);
        &mut self.signal[MAX_WINDOW - len..]
    }

    pub fn set_points(&mut self, count: usize) {
        if count == self.frequencies.len() {
            return;
        }
        let last = count.saturating_sub(1).max(1) as f32;
        self.frequencies = (0..count).map(|i| frequency(i as f32 / last)).collect();
        self.levels = resample(&self.levels, count, SILENCE_DB);
        self.peaks = resample(&self.peaks, count, SILENCE_DB);
        self.holds = resample(&self.holds, count, 0.0);
        self.refresh_tilt();
    }

    pub fn set_settings(&mut self, settings: Settings) {
        if settings != self.settings {
            self.settings = settings;
            self.refresh_tilt();
        }
    }

    fn refresh_tilt(&mut self) {
        let tilt = self.settings.tilt_db_per_octave;
        self.tilt = self
            .frequencies
            .iter()
            .map(|&hz| tilt * (hz / 1_000.0).log2())
            .collect();
    }

    pub fn frequencies(&self) -> &[f32] {
        &self.frequencies
    }

    pub fn levels(&self) -> &[f32] {
        &self.levels
    }

    pub fn peaks(&self) -> &[f32] {
        &self.peaks
    }

    pub fn zone_level(&self, zone: usize) -> f32 {
        self.zones.get(zone).copied().map_or(SILENCE_DB, to_db)
    }

    pub fn is_settled(&self, full: bool) -> bool {
        self.levels.iter().all(|&l| l < SETTLED_DB)
            && (!full
                || (self.peaks.iter().all(|&p| p < SETTLED_DB)
                    && self.zones.iter().all(|&z| z < SETTLED_ZONE_POWER)))
    }

    pub fn reset(&mut self) {
        self.levels.fill(SILENCE_DB);
        self.peaks.fill(SILENCE_DB);
        self.holds.fill(0.0);
        self.zones = [0.0; ZONES.len()];
    }

    pub fn update(&mut self, sample_rate: Option<u32>, dt: f32) {
        let dt = dt.clamp(0.0, MAX_STEP_SECONDS);
        let heard = sample_rate.filter(|&rate| rate > 0 && self.transform(rate));
        let calibration = self.transform.as_ref().map_or(0.0, |t| t.calibration_db);
        let window_power = self.transform.as_ref().map_or(1.0, |t| t.window_power);

        for (z, zone) in ZONES.iter().enumerate() {
            let power = if heard.is_some() {
                let high = zone.high.min(self.nyquist);
                if high > zone.low {
                    let sum = self.integral(f64::from(high) / self.bin_hz)
                        - self.integral(f64::from(zone.low) / self.bin_hz);
                    4.0 * sum.max(0.0) / window_power
                } else {
                    0.0
                }
            } else {
                0.0
            };
            self.zones[z] = approach(self.zones[z], power, dt, ZONE_SECONDS);
        }

        let half = 2f64.powf(f64::from(self.settings.smoothing_octaves) / 2.0);
        let release = self.settings.release_seconds.max(0.001);
        for i in 0..self.frequencies.len() {
            let hz = self.frequencies[i];
            let target = if heard.is_some() && hz <= self.nyquist {
                let low = f64::from(hz) / half / self.bin_hz;
                let high = f64::from(hz) * half / self.bin_hz;
                let power = if high - low < 1.5 {
                    self.power_at(f64::from(hz) / self.bin_hz)
                } else {
                    (self.integral(high) - self.integral(low)) / (high - low)
                };
                to_db(power) + calibration + self.tilt[i]
            } else {
                SILENCE_DB
            };
            let current = self.levels[i];
            let seconds = if target > current {
                ATTACK_SECONDS
            } else {
                release
            };
            let level = approach(f64::from(current), f64::from(target), dt, seconds) as f32;
            self.levels[i] = level;
            if level >= self.peaks[i] {
                self.peaks[i] = level;
                self.holds[i] = PEAK_HOLD_SECONDS;
            } else if self.holds[i] > 0.0 {
                self.holds[i] -= dt;
            } else {
                self.peaks[i] = (self.peaks[i] - PEAK_FALL_DB_PER_SECOND * dt).max(level);
            }
        }
    }

    fn transform(&mut self, sample_rate: u32) -> bool {
        let len = window_len(sample_rate);
        if self.transform.as_ref().map(|t| t.window.len()) != Some(len) {
            self.transform = Some(Transform::new(&mut self.planner, len));
            self.power = vec![0.0; len / 2 + 1];
            self.cumulative = vec![0.0; len / 2 + 2];
        }
        let Some(t) = self.transform.as_mut() else {
            return false;
        };
        let tail = &self.signal[MAX_WINDOW - len..];
        for ((slot, &sample), &w) in t.input.iter_mut().zip(tail).zip(&t.window) {
            *slot = sample * w;
        }
        if t.fft
            .process_with_scratch(&mut t.input, &mut t.output, &mut t.scratch)
            .is_err()
        {
            return false;
        }
        let norm = 1.0 / (len as f64 * len as f64);
        for (power, bin) in self.power.iter_mut().zip(&t.output) {
            let (re, im) = (f64::from(bin.re), f64::from(bin.im));
            *power = (re * re + im * im) * norm;
        }
        self.cumulative[0] = 0.0;
        for k in 0..self.power.len() {
            self.cumulative[k + 1] = self.cumulative[k] + self.power[k];
        }
        self.bin_hz = f64::from(sample_rate) / len as f64;
        self.nyquist = sample_rate as f32 / 2.0;
        true
    }

    fn integral(&self, bin: f64) -> f64 {
        let bins = self.power.len();
        let x = (bin + 0.5).clamp(0.0, bins as f64);
        let k = x.floor() as usize;
        if k >= bins {
            return self.cumulative[bins];
        }
        self.cumulative[k] + (x - k as f64) * self.power[k]
    }

    fn power_at(&self, bin: f64) -> f64 {
        let last = self.power.len().saturating_sub(1);
        let x = bin.clamp(0.0, last as f64);
        let k = (x.floor() as usize).min(last);
        let next = (k + 1).min(last);
        let frac = x - k as f64;
        self.power[k] * (1.0 - frac) + self.power[next] * frac
    }
}

#[cfg(test)]
mod tests;
