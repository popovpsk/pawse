/// Below this volume gap from unity (1.0) we still consider the signal path
/// "bit-perfect". Must match the IOProc's skip-multiply threshold in
/// `exclusive::macos::ioproc::ioproc_callback` — otherwise the indicator and
/// the actual signal-path behaviour disagree at boundary volumes (e.g. vol=0.99
/// where IOProc skips the multiply but the indicator would flag attenuation).
pub const UNITY_VOLUME_TOLERANCE: f32 = 0.02;

#[derive(Debug, Clone)]
pub struct BitPerfectStatus {
    pub issues: Vec<BitPerfectIssue>,
}

impl BitPerfectStatus {
    pub fn is_bit_perfect(&self) -> bool {
        self.issues.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_status_is_bit_perfect() {
        let s = BitPerfectStatus { issues: vec![] };
        assert!(s.is_bit_perfect());
    }

    #[test]
    fn any_issue_breaks_bit_perfect() {
        let s = BitPerfectStatus {
            issues: vec![BitPerfectIssue::NotExclusive],
        };
        assert!(!s.is_bit_perfect());
    }

    #[test]
    fn no_source_status_is_not_bit_perfect() {
        let s = BitPerfectStatus {
            issues: vec![BitPerfectIssue::NoSource],
        };
        assert!(!s.is_bit_perfect());
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum BitPerfectIssue {
    /// Output is shared (going through the OS mixer).
    NotExclusive,
    /// Device hardware volume scalar < 1.0 (system slider attenuates).
    SystemVolumeNotUnity { current: f32 },
    /// Device hardware mute is on.
    SystemMuted,
    /// App-level digital volume < 1.0 (in-app slider attenuates).
    AppVolumeNotUnity { current: f32 },
    /// Device sample rate doesn't match the source — CoreAudio is resampling.
    SampleRateMismatch { source: u32, device: u32 },
    /// Source bit depth exceeds what the f32 transport preserves (24 mantissa bits).
    BitDepthExceedsContainer { source: u8 },
    /// No track loaded — status is undefined.
    NoSource,
}
