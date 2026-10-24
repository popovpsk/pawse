use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use crate::band::{Band, Kind};
use crate::filter::{Coefficients, preamp_db};

const SETTLE_BELOW: f64 = 1e-30;

pub struct Control {
    generation: AtomicU64,
    bands: Mutex<Option<Arc<[Band]>>>,
}

impl Control {
    pub fn new(bands: Option<Vec<Band>>) -> Self {
        Self {
            generation: AtomicU64::new(0),
            bands: Mutex::new(bands.map(Arc::from)),
        }
    }

    pub fn set(&self, bands: Option<Vec<Band>>) {
        let mut current = self.bands.lock().unwrap_or_else(PoisonError::into_inner);
        if current.as_deref() == bands.as_deref() {
            return;
        }
        *current = bands.map(Arc::from);
        self.generation.fetch_add(1, Ordering::Release);
    }

    fn generation(&self) -> u64 {
        self.generation.load(Ordering::Acquire)
    }

    fn snapshot(&self) -> (u64, Option<Arc<[Band]>>) {
        let current = self.bands.lock().unwrap_or_else(PoisonError::into_inner);
        (self.generation.load(Ordering::Acquire), current.clone())
    }
}

pub struct Processor {
    control: Arc<Control>,
    generation: Option<u64>,
    bands: Option<Arc<[Band]>>,
    format: Option<(u32, usize)>,
    chain: Option<Chain>,
    fade_from: Option<Option<Chain>>,
}

impl Processor {
    pub fn new(control: Arc<Control>) -> Self {
        Self {
            control,
            generation: None,
            bands: None,
            format: None,
            chain: None,
            fade_from: None,
        }
    }

    pub fn prepare(&mut self, rate: u32, channels: usize) -> bool {
        let format = (rate, channels.max(1));
        if self.format != Some(format) {
            self.sync();
            self.format = Some(format);
            self.chain = Chain::build(self.bands.as_deref(), format);
            self.fade_from = None;
        } else if self.generation != Some(self.control.generation()) {
            self.sync();
            let mut next = Chain::build(self.bands.as_deref(), format);
            if let (Some(next), Some(previous)) = (next.as_mut(), self.chain.as_ref()) {
                next.inherit(previous);
            }
            let previous = std::mem::replace(&mut self.chain, next);
            if self.fade_from.is_none() && (previous.is_some() || self.chain.is_some()) {
                self.fade_from = Some(previous);
            }
        }
        self.chain.is_some() || self.fade_from.is_some()
    }

    pub fn process(&mut self, samples: &mut [f32]) {
        let Some((_, channels)) = self.format else {
            return;
        };
        match self.fade_from.take() {
            Some(mut from) => crossfade(samples, channels, from.as_mut(), self.chain.as_mut()),
            None => {
                if let Some(chain) = self.chain.as_mut() {
                    chain.run(samples, channels);
                }
            }
        }
        if let Some(chain) = self.chain.as_mut() {
            chain.settle();
        }
    }

    pub fn is_active(&self) -> bool {
        self.chain.is_some()
    }

    fn sync(&mut self) {
        let (generation, bands) = self.control.snapshot();
        self.generation = Some(generation);
        self.bands = bands;
    }
}

fn crossfade(
    samples: &mut [f32],
    channels: usize,
    from: Option<&mut Chain>,
    to: Option<&mut Chain>,
) {
    let frames = samples.len() / channels;
    if frames == 0 {
        return;
    }
    let (mut from, mut to) = (from, to);
    for (i, frame) in samples.chunks_exact_mut(channels).enumerate() {
        let t = (i + 1) as f64 / frames as f64;
        for (channel, sample) in frame.iter_mut().enumerate() {
            let x = f64::from(*sample);
            let a = from
                .as_deref_mut()
                .map_or(x, |chain| chain.tick(x, channel));
            let b = to.as_deref_mut().map_or(x, |chain| chain.tick(x, channel));
            *sample = (a + (b - a) * t) as f32;
        }
    }
}

struct Chain {
    gain: f64,
    sections: Vec<Section>,
}

struct Section {
    band: usize,
    kind: Kind,
    coefficients: Coefficients,
    state: Vec<[f64; 2]>,
}

impl Chain {
    fn build(bands: Option<&[Band]>, (rate, channels): (u32, usize)) -> Option<Self> {
        let rate = f64::from(rate);
        let active: Vec<(usize, Band)> = bands?
            .iter()
            .copied()
            .enumerate()
            .filter(|(_, band)| !band.is_flat())
            .collect();
        if active.is_empty() {
            return None;
        }
        let bands: Vec<Band> = active.iter().map(|&(_, band)| band).collect();
        let gain = 10f64.powf(preamp_db(&bands, rate) / 20.0);
        let sections = active
            .iter()
            .map(|(ix, band)| Section {
                band: *ix,
                kind: band.kind,
                coefficients: Coefficients::design(band, rate),
                state: vec![[0.0; 2]; channels],
            })
            .collect();
        Some(Self { gain, sections })
    }

    fn inherit(&mut self, previous: &Chain) {
        for section in &mut self.sections {
            if let Some(old) = previous
                .sections
                .iter()
                .find(|old| old.band == section.band && old.kind == section.kind)
            {
                section.state.clone_from(&old.state);
            }
        }
    }

    fn tick(&mut self, x: f64, channel: usize) -> f64 {
        let mut y = x;
        for section in &mut self.sections {
            let c = &section.coefficients;
            let z = &mut section.state[channel];
            let out = c.b0 * y + z[0];
            z[0] = c.b1 * y - c.a1 * out + z[1];
            z[1] = c.b2 * y - c.a2 * out;
            y = out;
        }
        y * self.gain
    }

    fn run(&mut self, samples: &mut [f32], channels: usize) {
        for frame in samples.chunks_exact_mut(channels) {
            for (channel, sample) in frame.iter_mut().enumerate() {
                *sample = self.tick(f64::from(*sample), channel) as f32;
            }
        }
    }

    fn settle(&mut self) {
        for z in self
            .sections
            .iter_mut()
            .flat_map(|section| section.state.iter_mut().flatten())
        {
            if !z.is_finite() || z.abs() < SETTLE_BELOW {
                *z = 0.0;
            }
        }
    }
}
