use audio_common::{AudioBuffer, AudioError};
use std::any::Any;

pub trait DspProcessor: Send + Any {
    fn process(&mut self, buffer: &mut AudioBuffer) -> Result<(), AudioError>;
    fn reset(&mut self);
}

pub struct VolumeProcessor {
    volume: f32,
    bypass: bool,
}

impl VolumeProcessor {
    pub fn new() -> Self {
        Self {
            volume: 1.0,
            bypass: false,
        }
    }

    pub fn set_volume(&mut self, volume: f32) {
        self.volume = volume.clamp(0.0, 1.0);
    }

    pub fn get_volume(&self) -> f32 {
        self.volume
    }

    pub fn set_bypass(&mut self, bypass: bool) {
        self.bypass = bypass;
    }

    pub fn is_bypassed(&self) -> bool {
        self.bypass
    }
}

impl Default for VolumeProcessor {
    fn default() -> Self {
        Self::new()
    }
}

impl DspProcessor for VolumeProcessor {
    fn process(&mut self, buffer: &mut AudioBuffer) -> Result<(), AudioError> {
        if self.bypass {
            return Ok(());
        }

        if (self.volume - 1.0).abs() < f32::EPSILON {
            return Ok(());
        }

        let samples = buffer.as_slice_mut();
        for sample in samples.iter_mut() {
            *sample *= self.volume;
        }

        Ok(())
    }

    fn reset(&mut self) {
        self.volume = 1.0;
        self.bypass = false;
    }
}

pub struct DSPChain {
    pub processors: Vec<Box<dyn DspProcessor>>,
}

impl DSPChain {
    pub fn new() -> Self {
        Self {
            processors: Vec::new(),
        }
    }

    pub fn add_processor<P: DspProcessor + 'static>(&mut self, processor: P) {
        self.processors.push(Box::new(processor));
    }

    pub fn process(&mut self, buffer: &mut AudioBuffer) -> Result<(), AudioError> {
        for processor in self.processors.iter_mut() {
            processor.process(buffer)?;
        }
        Ok(())
    }

    pub fn reset(&mut self) {
        for processor in self.processors.iter_mut() {
            processor.reset();
        }
    }
}

impl Default for DSPChain {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_volume_processing() {
        let mut processor = VolumeProcessor::new();
        let mut buffer = AudioBuffer::from_f32(vec![1.0, 0.5, 0.25]);

        processor.set_volume(0.5);
        processor.process(&mut buffer).unwrap();

        let samples = buffer.as_f32_slice().unwrap();
        assert!((samples[0] - 0.5).abs() < 0.001);
        assert!((samples[1] - 0.25).abs() < 0.001);
        assert!((samples[2] - 0.125).abs() < 0.001);
    }

    #[test]
    fn test_bypass() {
        let mut processor = VolumeProcessor::new();
        let mut buffer = AudioBuffer::from_f32(vec![1.0, 0.5]);

        processor.set_volume(0.0);
        processor.set_bypass(true);
        processor.process(&mut buffer).unwrap();

        let samples = buffer.as_f32_slice().unwrap();
        assert!((samples[0] - 1.0).abs() < 0.001);
        assert!((samples[1] - 0.5).abs() < 0.001);
    }

    #[test]
    fn test_dsp_chain() {
        let mut chain = DSPChain::new();
        let mut volume = VolumeProcessor::new();
        volume.set_volume(0.5);
        chain.add_processor(volume);

        let mut buffer = AudioBuffer::from_f32(vec![1.0, 0.5]);
        chain.process(&mut buffer).unwrap();

        let samples = buffer.as_f32_slice().unwrap();
        assert!((samples[0] - 0.5).abs() < 0.001);
    }
}
