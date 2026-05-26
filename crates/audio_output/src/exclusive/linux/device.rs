/// Maps a device UID (cpal's `Device::id().1`, an ALSA PCM name) to a direct
/// hardware PCM name plus the matching control/mixer name.
///
/// For bit-perfect playback we want a raw `hw:` device (no `plughw`/`dmix`/Pulse
/// resampling layer). This is heuristic — the exact shape of cpal's ALSA device
/// id is the main thing to confirm on real hardware:
///
/// - empty            → `hw:0,0`           (first card, first device)
/// - `hw:…`           → used as-is
/// - `plughw:…`       → de-plugged to `hw:…`
/// - `…CARD=name…`    → `hw:CARD=name`
/// - anything else    → used verbatim (best effort)
///
/// The control name (for the mixer) is the PCM name truncated before the device
/// component, e.g. `hw:0,0` → `hw:0`, `hw:CARD=x,DEV=0` → `hw:CARD=x`.
pub(super) fn resolve_names(uid: &str) -> (String, String) {
    let pcm = if uid.is_empty() {
        "hw:0,0".to_string()
    } else if let Some(rest) = uid.strip_prefix("plughw:") {
        format!("hw:{}", rest)
    } else if uid.starts_with("hw:") {
        uid.to_string()
    } else if let Some(card) = extract_card(uid) {
        format!("hw:CARD={}", card)
    } else {
        uid.to_string()
    };

    let ctl = control_name(&pcm);
    (pcm, ctl)
}

fn extract_card(uid: &str) -> Option<String> {
    let after = uid.split("CARD=").nth(1)?;
    let card = after.split([',', ':']).next()?.trim();
    if card.is_empty() {
        None
    } else {
        Some(card.to_string())
    }
}

fn control_name(pcm: &str) -> String {
    if pcm.starts_with("hw:")
        && let Some(idx) = pcm.find(',')
    {
        return pcm[..idx].to_string();
    }
    if pcm.starts_with("hw:") {
        return pcm.to_string();
    }
    "default".to_string()
}
