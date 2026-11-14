use audio_common::ReplayGain;

use crate::meter::Measurement;

pub const REFERENCE_LUFS: f32 = -18.0;
pub const HEADROOM_DB: f32 = 3.0;
const MIN_GAIN_DB: f32 = -30.0;
const MAX_GAIN_DB: f32 = 20.0;

impl Measurement {
    pub fn replay_gain(&self) -> ReplayGain {
        ReplayGain {
            gain_db: REFERENCE_LUFS - self.lufs,
            peak: Some(self.peak),
        }
    }
}

pub fn applied_db(gain: ReplayGain) -> f32 {
    if !gain.gain_db.is_finite() {
        return 0.0;
    }
    let db = gain.gain_db.clamp(MIN_GAIN_DB, MAX_GAIN_DB);
    if db <= 0.0 {
        return db;
    }
    let ceiling = match gain.peak {
        Some(peak) if peak.is_finite() && peak > 0.0 => {
            (-20.0 * peak.log10() - HEADROOM_DB).max(0.0)
        }
        _ => 0.0,
    };
    db.min(ceiling)
}

pub fn factor(gain: Option<ReplayGain>) -> f32 {
    let Some(gain) = gain else {
        return 1.0;
    };
    let db = applied_db(gain);
    if db.abs() < 0.005 {
        1.0
    } else {
        10f32.powf(db / 20.0)
    }
}
