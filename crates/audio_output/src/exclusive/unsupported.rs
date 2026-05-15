use audio_common::AudioError;
use crate::cpal_stream::{AudioOutput, OutputConfig};
use crate::ring_buffer::AudioRingBuffer;

use std::sync::Arc;

pub struct ExclusiveOutput;

impl ExclusiveOutput {
    pub fn new(
        _buffer: Arc<AudioRingBuffer>,
        _config: OutputConfig,
        _audio_device_id: u32,
    ) -> Result<Self, AudioError> {
        Err(AudioError::UnsupportedFormat(
            "Exclusive mode is not supported on this platform".to_string(),
        ))
    }
}

impl AudioOutput for ExclusiveOutput {
    fn write(&self, _samples: &audio_common::AudioBatch) -> usize {
        0
    }
    fn clear(&self) {}
    fn pause(&self) {}
    fn resume(&self) {}
    fn is_playing(&self) -> bool {
        false
    }
    fn set_volume(&self, _volume: f32) {}
}