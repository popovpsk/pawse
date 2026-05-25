use crate::exclusive::{Backend, DeviceSnapshot, ExclusiveEvent};
use audio_common::AudioBatch;

pub(super) struct UnsupportedBackend;

impl Backend for UnsupportedBackend {
    fn write(&self, _: &AudioBatch) -> usize {
        0
    }
    fn clear(&self) {}
    fn pause(&self) {}
    fn resume(&self) {}
    fn is_playing(&self) -> bool {
        false
    }
    fn set_volume(&self, _: f32) {}
    fn begin_fade(&self, _: Option<f32>, _: f32, _: u32) {}
    fn take_fade_event(&self) -> Option<crate::FadeEvent> {
        None
    }
    fn reset_fade(&self) {}
    fn set_hw_volume(&self, _: f32) {}
    fn is_alive(&self) -> bool {
        false
    }
    fn take_event(&self) -> Option<ExclusiveEvent> {
        None
    }
    fn original_rate(&self) -> f64 {
        0.0
    }
    fn suppress_cleanup(&self) {}
    fn allow_cleanup(&self) {}
    fn device_snapshot(&self) -> DeviceSnapshot {
        DeviceSnapshot {
            hw_volume: 1.0,
            hw_muted: false,
            device_sample_rate: 0,
            app_volume: 1.0,
        }
    }
}
