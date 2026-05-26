use alsa::mixer::{Mixer, Selem, SelemChannelId, SelemId};

/// Opens the mixer for a control device and resolves a usable playback selem id
/// ("Master" then "PCM"). Returns `None` if no mixer / no matching control.
pub(super) fn open(ctl_name: &str) -> Option<(Mixer, SelemId)> {
    let mixer = Mixer::new(ctl_name, false).ok()?;
    for name in ["Master", "PCM"] {
        let id = SelemId::new(name, 0);
        if mixer.find_selem(&id).is_some() {
            return Some((mixer, id));
        }
    }
    None
}

fn selem<'a>(mixer: &'a Mixer, id: &SelemId) -> Option<Selem<'a>> {
    mixer.find_selem(id)
}

/// Reads the playback volume scalar in [0.0, 1.0]; 1.0 if no volume control.
pub(super) fn read_volume(mixer: &Mixer, id: &SelemId) -> f32 {
    let _ = mixer.handle_events();
    let Some(s) = selem(mixer, id) else {
        return 1.0;
    };
    if !s.has_playback_volume() {
        return 1.0;
    }
    let (min, max) = s.get_playback_volume_range();
    if max <= min {
        return 1.0;
    }
    let v = s
        .get_playback_volume(SelemChannelId::FrontLeft)
        .unwrap_or(max);
    ((v - min) as f32 / (max - min) as f32).clamp(0.0, 1.0)
}

/// Reads the playback mute state; false if no mute switch.
pub(super) fn read_muted(mixer: &Mixer, id: &SelemId) -> bool {
    let _ = mixer.handle_events();
    let Some(s) = selem(mixer, id) else {
        return false;
    };
    if !s.has_playback_switch() {
        return false;
    }
    // The switch reports 1 = on (audible), 0 = muted.
    s.get_playback_switch(SelemChannelId::FrontLeft)
        .map(|on| on == 0)
        .unwrap_or(false)
}

/// Writes the playback volume scalar (0.0–1.0) to all channels. Best-effort.
pub(super) fn set_volume(mixer: &Mixer, id: &SelemId, volume: f32) {
    let Some(s) = selem(mixer, id) else {
        return;
    };
    if !s.has_playback_volume() {
        return;
    }
    let (min, max) = s.get_playback_volume_range();
    let val = min + ((max - min) as f32 * volume.clamp(0.0, 1.0)).round() as i64;
    let _ = s.set_playback_volume_all(val);
}
