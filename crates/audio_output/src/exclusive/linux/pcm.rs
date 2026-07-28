use std::ffi::OsString;

use alsa::Direction;
use alsa::ValueOr;
use alsa::pcm::{Access, Format, HwParams, PCM};
use audio_common::AudioError;

use crate::cpal_stream::OutputConfig;

const PROPS_KEY: &str = "PIPEWIRE_PROPS";
const FORCE_RATE_PROPS: &str = "{ node.force-rate = 0 }";

pub(super) struct DeviceFormat {
    pub(super) period_frames: usize,
}

pub(super) fn pcm_name(uid: &str) -> String {
    match uid.strip_prefix("pw:").map(str::trim) {
        Some(node) if !node.is_empty() => format!("pipewire:NODE={}", node),
        _ => "pipewire".to_string(),
    }
}

struct ForceRate {
    previous: Option<OsString>,
}

impl ForceRate {
    fn set() -> Self {
        let previous = std::env::var_os(PROPS_KEY);
        unsafe { std::env::set_var(PROPS_KEY, FORCE_RATE_PROPS) };
        Self { previous }
    }
}

impl Drop for ForceRate {
    fn drop(&mut self) {
        unsafe {
            match self.previous.take() {
                Some(props) => std::env::set_var(PROPS_KEY, props),
                None => std::env::remove_var(PROPS_KEY),
            }
        }
    }
}

pub(super) fn open(uid: &str, config: &OutputConfig) -> Result<(PCM, DeviceFormat), AudioError> {
    let name = pcm_name(uid);
    let _force_rate = ForceRate::set();

    let pcm = PCM::new(&name, Direction::Playback, false)
        .map_err(|e| AudioError::DeviceNotFound(format!("open '{}': {}", name, e)))?;
    let fmt = configure(&pcm, config)?;

    Ok((pcm, fmt))
}

fn configure(pcm: &PCM, config: &OutputConfig) -> Result<DeviceFormat, AudioError> {
    let hwp =
        HwParams::any(pcm).map_err(|e| AudioError::Output(format!("HwParams::any: {}", e)))?;

    hwp.set_access(Access::RWInterleaved)
        .map_err(|e| AudioError::Output(format!("set_access: {}", e)))?;
    hwp.set_channels(config.channels as u32)
        .map_err(|e| AudioError::UnsupportedFormat(format!("set_channels: {}", e)))?;
    hwp.set_rate(config.sample_rate, ValueOr::Nearest)
        .map_err(|e| AudioError::UnsupportedFormat(format!("set_rate: {}", e)))?;
    hwp.set_format(Format::FloatLE)
        .map_err(|e| AudioError::UnsupportedFormat(format!("set_format f32: {}", e)))?;
    hwp.set_period_size_near(1024, ValueOr::Nearest)
        .map_err(|e| AudioError::Output(format!("set_period_size_near: {}", e)))?;
    hwp.set_buffer_size_near(4096)
        .map_err(|e| AudioError::Output(format!("set_buffer_size_near: {}", e)))?;

    pcm.hw_params(&hwp)
        .map_err(|e| AudioError::Output(format!("hw_params commit: {}", e)))?;

    let actual_rate = hwp
        .get_rate()
        .map_err(|e| AudioError::Output(format!("get_rate: {}", e)))?;
    if actual_rate != config.sample_rate {
        return Err(AudioError::UnsupportedFormat(format!(
            "PipeWire negotiated {} Hz but source is {} Hz",
            actual_rate, config.sample_rate
        )));
    }

    let period_frames = hwp
        .get_period_size()
        .map_err(|e| AudioError::Output(format!("get_period_size: {}", e)))?
        .max(1) as usize;

    pcm.prepare()
        .map_err(|e| AudioError::Output(format!("pcm.prepare: {}", e)))?;

    Ok(DeviceFormat { period_frames })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pw_uid_becomes_a_targeted_pipewire_pcm() {
        assert_eq!(
            pcm_name("pw:alsa_output.usb-Topping_D50_III-00.HiFi__Headphones__sink"),
            "pipewire:NODE=alsa_output.usb-Topping_D50_III-00.HiFi__Headphones__sink"
        );
    }

    #[test]
    fn empty_uid_follows_the_default_sink() {
        assert_eq!(pcm_name(""), "pipewire");
    }

    #[test]
    fn legacy_alsa_uids_follow_the_default_sink() {
        assert_eq!(pcm_name("plughw:CARD=III,DEV=0"), "pipewire");
        assert_eq!(pcm_name("hw:0,0"), "pipewire");
        assert_eq!(pcm_name("default"), "pipewire");
    }

    #[test]
    fn blank_node_falls_back_instead_of_building_an_empty_target() {
        assert_eq!(pcm_name("pw:"), "pipewire");
        assert_eq!(pcm_name("pw:   "), "pipewire");
    }
}
