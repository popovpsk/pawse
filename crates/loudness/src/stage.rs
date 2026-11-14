use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use audio_common::AudioSamples;

#[derive(Debug, Default)]
pub struct Switch(AtomicBool);

impl Switch {
    pub fn new(on: bool) -> Self {
        Self(AtomicBool::new(on))
    }

    pub fn set(&self, on: bool) {
        self.0.store(on, Ordering::Relaxed);
    }

    pub fn is_on(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}

pub struct GainStage {
    switch: Arc<Switch>,
    track: f32,
    current: f32,
}

impl GainStage {
    pub fn new(switch: Arc<Switch>) -> Self {
        Self {
            switch,
            track: 1.0,
            current: 1.0,
        }
    }

    fn target(&self) -> f32 {
        if self.switch.is_on() { self.track } else { 1.0 }
    }

    pub fn set_track(&mut self, factor: f32, continuous: bool) {
        self.track = if factor.is_finite() && factor > 0.0 {
            factor
        } else {
            1.0
        };
        if !continuous {
            self.current = self.target();
        }
    }

    pub fn process(&mut self, samples: &mut AudioSamples, channels: usize) {
        let target = self.target();
        if target == 1.0 && self.current == 1.0 {
            return;
        }
        let channels = channels.max(1);
        let frames = samples.len() / channels;
        if frames == 0 {
            return;
        }
        let data = samples.f32_mut();
        if self.current == target {
            for sample in data.iter_mut() {
                *sample *= target;
            }
            return;
        }
        let start = self.current;
        let step = (target - start) / frames as f32;
        for (index, frame) in data.chunks_exact_mut(channels).enumerate() {
            let gain = start + step * (index + 1) as f32;
            for sample in frame {
                *sample *= gain;
            }
        }
        self.current = target;
    }

    pub fn is_active(&self) -> bool {
        self.current != 1.0
    }
}
