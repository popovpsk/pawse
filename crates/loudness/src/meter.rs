const ABSOLUTE_GATE_LUFS: f64 = -70.0;
const RELATIVE_GATE_LU: f64 = 10.0;
const LOUDNESS_OFFSET: f64 = -0.691;
const SUB_BLOCKS: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Measurement {
    pub lufs: f32,
    pub peak: f32,
}

#[derive(Clone, Copy, Default)]
struct Biquad {
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
    z1: f64,
    z2: f64,
}

impl Biquad {
    fn run(&mut self, x: f64) -> f64 {
        let y = self.b0 * x + self.z1;
        self.z1 = self.b1 * x - self.a1 * y + self.z2;
        self.z2 = self.b2 * x - self.a2 * y;
        y
    }

    fn reset(&mut self) {
        self.z1 = 0.0;
        self.z2 = 0.0;
    }
}

#[derive(Clone, Copy)]
struct KWeighting {
    shelf: Biquad,
    high_pass: Biquad,
}

impl KWeighting {
    fn new(sample_rate: u32) -> Self {
        let rate = f64::from(sample_rate);

        let f0 = 1681.974450955533;
        let gain_db = 3.999843853973347;
        let q = 0.7071752369554196;
        let k = (std::f64::consts::PI * f0 / rate).tan();
        let vh = 10f64.powf(gain_db / 20.0);
        let vb = vh.powf(0.4996667741545416);
        let a0 = 1.0 + k / q + k * k;
        let shelf = Biquad {
            b0: (vh + vb * k / q + k * k) / a0,
            b1: 2.0 * (k * k - vh) / a0,
            b2: (vh - vb * k / q + k * k) / a0,
            a1: 2.0 * (k * k - 1.0) / a0,
            a2: (1.0 - k / q + k * k) / a0,
            ..Biquad::default()
        };

        let f0 = 38.13547087602444;
        let q = 0.5003270373238773;
        let k = (std::f64::consts::PI * f0 / rate).tan();
        let a0 = 1.0 + k / q + k * k;
        let high_pass = Biquad {
            b0: 1.0,
            b1: -2.0,
            b2: 1.0,
            a1: 2.0 * (k * k - 1.0) / a0,
            a2: (1.0 - k / q + k * k) / a0,
            ..Biquad::default()
        };

        Self { shelf, high_pass }
    }

    fn run(&mut self, x: f64) -> f64 {
        self.high_pass.run(self.shelf.run(x))
    }

    fn reset(&mut self) {
        self.shelf.reset();
        self.high_pass.reset();
    }
}

fn channel_weights(channels: usize) -> Vec<f64> {
    match channels {
        1 => vec![2.0],
        5 => vec![1.0, 1.0, 1.0, 1.41, 1.41],
        6 => vec![1.0, 1.0, 1.0, 0.0, 1.41, 1.41],
        8 => vec![1.0, 1.0, 1.0, 0.0, 1.41, 1.41, 1.41, 1.41],
        n => vec![1.0; n],
    }
}

pub struct Meter {
    sample_rate: u32,
    weights: Vec<f64>,
    filters: Vec<KWeighting>,
    step: usize,
    warm_up: usize,
    sub_energy: f64,
    sub_frames: usize,
    recent: [f64; SUB_BLOCKS],
    recent_len: usize,
    recent_next: usize,
    blocks: Vec<f64>,
    peak: f32,
    frames: u64,
}

impl Meter {
    pub fn new(sample_rate: u32, channels: usize) -> Self {
        let sample_rate = sample_rate.max(1);
        let channels = channels.max(1);
        Self {
            sample_rate,
            weights: channel_weights(channels),
            filters: vec![KWeighting::new(sample_rate); channels],
            step: (sample_rate as usize / 10).max(1),
            warm_up: 0,
            sub_energy: 0.0,
            sub_frames: 0,
            recent: [0.0; SUB_BLOCKS],
            recent_len: 0,
            recent_next: 0,
            blocks: Vec::new(),
            peak: 0.0,
            frames: 0,
        }
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    pub fn channels(&self) -> usize {
        self.weights.len()
    }

    pub fn frames(&self) -> u64 {
        self.frames
    }

    pub fn start_window(&mut self) {
        for filter in &mut self.filters {
            filter.reset();
        }
        self.warm_up = self.step;
        self.sub_energy = 0.0;
        self.sub_frames = 0;
        self.recent_len = 0;
        self.recent_next = 0;
    }

    pub fn push(&mut self, interleaved: &[f32]) {
        let channels = self.weights.len();
        for frame in interleaved.chunks_exact(channels) {
            let mut energy = 0.0;
            for ((sample, filter), weight) in frame
                .iter()
                .zip(self.filters.iter_mut())
                .zip(self.weights.iter())
            {
                let magnitude = sample.abs();
                if magnitude > self.peak && magnitude.is_finite() {
                    self.peak = magnitude;
                }
                let filtered = filter.run(f64::from(*sample));
                energy += weight * filtered * filtered;
            }
            self.frames += 1;
            if self.warm_up > 0 {
                self.warm_up -= 1;
                continue;
            }
            self.sub_energy += energy;
            self.sub_frames += 1;
            if self.sub_frames == self.step {
                self.close_sub_block();
            }
        }
        for filter in &mut self.filters {
            for biquad in [&mut filter.shelf, &mut filter.high_pass] {
                if !biquad.z1.is_finite() || !biquad.z2.is_finite() {
                    biquad.reset();
                }
            }
        }
    }

    fn close_sub_block(&mut self) {
        self.recent[self.recent_next] = self.sub_energy;
        self.recent_next = (self.recent_next + 1) % SUB_BLOCKS;
        self.recent_len = (self.recent_len + 1).min(SUB_BLOCKS);
        self.sub_energy = 0.0;
        self.sub_frames = 0;
        if self.recent_len == SUB_BLOCKS {
            let sum: f64 = self.recent.iter().sum();
            let mean = sum / (SUB_BLOCKS * self.step) as f64;
            if mean.is_finite() {
                self.blocks.push(mean);
            }
        }
    }

    pub fn blocks(&self) -> usize {
        self.blocks.len()
    }

    pub fn finish(&self) -> Option<Measurement> {
        let absolute = energy_of(ABSOLUTE_GATE_LUFS);
        let (sum, count) = mean_above(&self.blocks, absolute);
        if count == 0 {
            return None;
        }
        let relative = energy_of(loudness_of(sum / count as f64) - RELATIVE_GATE_LU);
        let (sum, count) = mean_above(&self.blocks, absolute.max(relative));
        if count == 0 {
            return None;
        }
        Some(Measurement {
            lufs: loudness_of(sum / count as f64) as f32,
            peak: self.peak,
        })
    }
}

fn mean_above(blocks: &[f64], threshold: f64) -> (f64, usize) {
    blocks
        .iter()
        .filter(|&&energy| energy > threshold)
        .fold((0.0, 0), |(sum, count), energy| (sum + energy, count + 1))
}

fn loudness_of(energy: f64) -> f64 {
    LOUDNESS_OFFSET + 10.0 * energy.log10()
}

fn energy_of(lufs: f64) -> f64 {
    10f64.powf((lufs - LOUDNESS_OFFSET) / 10.0)
}
