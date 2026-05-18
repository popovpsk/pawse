use std::mem;
use std::ptr::{self, NonNull};

use audio_common::AudioError;
use objc2_core_audio::{
    kAudioDevicePropertyAvailableNominalSampleRates, kAudioObjectPropertyElementMain,
    kAudioObjectPropertyScopeGlobal, AudioObjectGetPropertyData, AudioObjectGetPropertyDataSize,
    AudioObjectPropertyAddress,
};
use objc2_core_audio_types::AudioValueRange;

pub(super) fn get_available_samplerates(device_id: u32) -> Result<Vec<i32>, AudioError> {
    let addr = AudioObjectPropertyAddress {
        mSelector: kAudioDevicePropertyAvailableNominalSampleRates,
        mScope: kAudioObjectPropertyScopeGlobal,
        mElement: kAudioObjectPropertyElementMain,
    };

    unsafe {
        let mut data_size: u32 = 0;
        let status = AudioObjectGetPropertyDataSize(
            device_id,
            NonNull::from(&addr),
            0,
            ptr::null(),
            NonNull::from(&mut data_size),
        );
        if status != 0 {
            return Err(AudioError::UnsupportedFormat(format!(
                "AudioObjectGetPropertyDataSize kAudioDevicePropertyAvailableNominalSampleRates: {:#x}",
                status
            )));
        }

        if data_size == 0 {
            return Ok(Vec::new());
        }

        let count = data_size as usize / mem::size_of::<AudioValueRange>();
        let mut ranges = vec![AudioValueRange { mMinimum: 0.0, mMaximum: 0.0 }; count];

        let status = AudioObjectGetPropertyData(
            device_id,
            NonNull::from(&addr),
            0,
            ptr::null(),
            NonNull::from(&mut data_size),
            NonNull::new(ranges.as_mut_ptr()).unwrap().cast(),
        );
        if status != 0 {
            return Err(AudioError::UnsupportedFormat(format!(
                "AudioObjectGetPropertyData kAudioDevicePropertyAvailableNominalSampleRates: {:#x}",
                status
            )));
        }

        Ok(ranges.iter().map(|r| r.mMinimum as i32).collect())
    }
}

/// Port of deadbeef's get_best_samplerate().
///
/// Scoring: dist*2 + modulo, with a 100× penalty for downscaling.
/// Prefers exact matches, then integer-multiple upscaling, then downscaling.
pub(super) fn get_best_samplerate(target: u32, available: &[i32]) -> u32 {
    let target = target as i64;
    let mut best_score = i64::MAX;
    let mut best_rate = 0i32;

    for &rate in available {
        let rate_i = rate as i64;
        let dist = (rate_i - target).abs();
        let modulo = if target > rate_i {
            target % rate_i
        } else {
            rate_i % target
        };
        let mut score = dist * 2 + modulo;

        if rate_i < target {
            score = score.saturating_mul(100);
        }

        if score < best_score {
            best_score = score;
            best_rate = rate;
        }
    }

    best_rate as u32
}

#[cfg(test)]
mod tests {
    use super::get_best_samplerate;

    // Device 1: standard DAC with 44.1 kHz and 48 kHz
    const REG: &[i32] = &[44100, 48000];

    #[test]
    fn reg_96k_gives_48k() {
        assert_eq!(get_best_samplerate(96000, REG), 48000);
    }
    #[test]
    fn reg_882k_gives_441k() {
        assert_eq!(get_best_samplerate(88200, REG), 44100);
    }
    #[test]
    fn reg_48k_gives_48k() {
        assert_eq!(get_best_samplerate(48000, REG), 48000);
    }
    #[test]
    fn reg_441k_gives_441k() {
        assert_eq!(get_best_samplerate(44100, REG), 44100);
    }
    #[test]
    fn reg_11025_gives_441k() {
        assert_eq!(get_best_samplerate(11025, REG), 44100);
    }

    // Device 2: high-rate DAC {88.2, 96, 176.4, 192} kHz
    const HIGH: &[i32] = &[88200, 96000, 176400, 192000];

    #[test]
    fn high_96k_gives_96k() {
        assert_eq!(get_best_samplerate(96000, HIGH), 96000);
    }
    #[test]
    fn high_882k_gives_882k() {
        assert_eq!(get_best_samplerate(88200, HIGH), 88200);
    }
    #[test]
    fn high_48k_gives_96k() {
        assert_eq!(get_best_samplerate(48000, HIGH), 96000);
    }
    #[test]
    fn high_441k_gives_882k() {
        assert_eq!(get_best_samplerate(44100, HIGH), 88200);
    }
    #[test]
    fn high_11025_gives_882k() {
        assert_eq!(get_best_samplerate(11025, HIGH), 88200);
    }

    // Device 3: low-rate device {8, 11.025, 16, 22.05, 48} kHz
    const LOW: &[i32] = &[8000, 11025, 16000, 22050, 48000];

    #[test]
    fn low_96k_gives_48k() {
        assert_eq!(get_best_samplerate(96000, LOW), 48000);
    }
    #[test]
    fn low_882k_gives_48k() {
        assert_eq!(get_best_samplerate(88200, LOW), 48000);
    }
    #[test]
    fn low_48k_gives_48k() {
        assert_eq!(get_best_samplerate(48000, LOW), 48000);
    }
    #[test]
    fn low_441k_gives_48k() {
        assert_eq!(get_best_samplerate(44100, LOW), 48000);
    }
    #[test]
    fn low_8k_gives_8k() {
        assert_eq!(get_best_samplerate(8000, LOW), 8000);
    }

    // Device 4: sparse {16, 48} kHz
    const SPARSE: &[i32] = &[16000, 48000];

    #[test]
    fn sparse_32k_gives_48k() {
        assert_eq!(get_best_samplerate(32000, SPARSE), 48000);
    }
    #[test]
    fn sparse_17k_gives_48k() {
        assert_eq!(get_best_samplerate(17000, SPARSE), 48000);
    }
    #[test]
    fn sparse_161k_gives_16k() {
        assert_eq!(get_best_samplerate(16100, SPARSE), 16000);
    }
}
