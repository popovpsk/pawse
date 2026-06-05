use std::sync::Arc;

use audio_common::AudioError;
use cpal::traits::{DeviceTrait, HostTrait};

#[cfg(target_os = "linux")]
use std::collections::{BTreeMap, HashMap};
#[cfg(target_os = "linux")]
use std::process::Command;

#[cfg(target_os = "linux")]
use serde::Deserialize;

#[derive(Clone, Debug)]
pub struct OutputDeviceInfo {
    pub name: String,
    pub uid: String,
    pub is_default: bool,
}

/// Tracks which audio device the user wants to use.
///
/// Identity is a UID string (CoreAudio device UID on macOS, cpal's `Device::id().1`
/// elsewhere) — *not* a cached `cpal::Device` handle, because those go stale when
/// the underlying hardware disconnects or is re-enumerated.
///
/// A `selected_uid` of `None` means "follow the system default device" — handy as
/// a recovery state when a previously-selected device disappears.
///
/// Every `resolve_*` call re-queries cpal's host. If the selected UID can't be
/// found (device unplugged, UID format changed, etc.) the resolver falls back to
/// the system default device and surfaces what happened to the caller via the
/// returned device's UID.
pub struct DeviceManager {
    host: Arc<cpal::Host>,
    selected_uid: Option<String>,
    cached_name: String,
}

impl DeviceManager {
    pub fn from_host(host: &Arc<cpal::Host>) -> Result<Self, AudioError> {
        let default = host
            .default_output_device()
            .ok_or_else(|| AudioError::DeviceNotFound("No default output device".to_string()))?;
        let name = device_display_name(&default);

        Ok(Self {
            host: host.clone(),
            selected_uid: None, // follow system default initially
            cached_name: name,
        })
    }

    pub fn headless(host: &Arc<cpal::Host>) -> Self {
        Self {
            host: host.clone(),
            selected_uid: None,
            cached_name: "(no output device)".to_string(),
        }
    }

    /// Live enumeration. Always re-queries the host so newly attached devices
    /// appear immediately without a manual refresh step.
    pub fn output_devices(&self) -> Result<Vec<OutputDeviceInfo>, AudioError> {
        // On Linux, prefer the PipeWire/PulseAudio sink list so the picker
        // matches what the user sees in their system settings. Critically, this
        // avoids cpal's raw ALSA enumeration entirely in the common case, which
        // otherwise probes dmix PCMs and spams "unable to open slave" errors.
        #[cfg(target_os = "linux")]
        {
            if let Some(sinks) = enum_pulse_sinks() {
                let list = build_pulse_list(&sinks, pulse_default_sink().as_deref());
                if !list.is_empty() {
                    return Ok(list);
                }
            }
            // No PipeWire/Pulse: fall back to grouping cpal's ALSA PCMs by card.
            let (raw, default_uid) = self.enumerate_cpal()?;
            let longnames = alsa_card_longnames();
            Ok(curate_alsa_cards(raw, default_uid.as_deref(), &longnames))
        }

        #[cfg(not(target_os = "linux"))]
        {
            let (devices, _default_uid) = self.enumerate_cpal()?;
            Ok(devices)
        }
    }

    /// Raw cpal enumeration: every output device the host exposes, plus the
    /// current default device's UID (for marking `is_default`).
    fn enumerate_cpal(&self) -> Result<(Vec<OutputDeviceInfo>, Option<String>), AudioError> {
        let default_uid = self
            .host
            .default_output_device()
            .and_then(|d| device_uid(&d).ok());

        let devices: Vec<OutputDeviceInfo> = self
            .host
            .output_devices()
            .map_err(|e| AudioError::Output(e.to_string()))?
            .map(|d| {
                let name = device_display_name(&d);
                let uid = device_uid(&d).unwrap_or_default();
                let is_default = default_uid.as_deref() == Some(uid.as_str());
                OutputDeviceInfo {
                    name,
                    uid,
                    is_default,
                }
            })
            .collect();

        Ok((devices, default_uid))
    }

    /// The most recently confirmed display name for the selected device.
    /// May be stale if the device just disconnected; in that case `resolve_device`
    /// will refresh it on the next call.
    pub fn selected_device_name(&self) -> &str {
        &self.cached_name
    }

    /// `Some(uid)` for a user-selected device; `None` means "follow system default".
    pub fn selected_uid(&self) -> Option<&str> {
        self.selected_uid.as_deref()
    }

    /// Resolves the current selection to a live `cpal::Device` handle.
    ///
    /// If `selected_uid` is `None`, returns the system default device.
    /// If `selected_uid` is `Some(uid)` but no device with that UID exists right
    /// now, the selection is cleared (so the app stops pointing at a ghost) and
    /// the system default is returned instead. The caller can detect the
    /// fallback by comparing the returned device's UID against the prior
    /// `selected_uid`.
    ///
    /// On Linux, a `pw:<node>` selection (a PipeWire/PulseAudio sink) routes
    /// through PipeWire: we open the host *default* device (the PipeWire ALSA
    /// plugin) and set `PIPEWIRE_NODE` so the stream lands on that sink. This
    /// avoids grabbing the hardware card directly — which under PipeWire causes
    /// dmix failures, stutter, and busy errors. Direct ALSA-PCM selections (the
    /// non-PipeWire fallback) still open their device by UID.
    pub fn resolve_device(&mut self) -> Result<Arc<cpal::Device>, AudioError> {
        #[cfg(target_os = "linux")]
        {
            if let Some(uid) = self.selected_uid.clone() {
                if let Some(node) = uid.strip_prefix("pw:") {
                    // Validate the sink still exists; if it vanished (e.g. a USB
                    // DAC was unplugged) clear the pin and fall through to default
                    // rather than keep targeting a ghost node. If enumeration
                    // fails we assume it's present (don't disrupt on a transient
                    // pactl hiccup).
                    let present = enum_pulse_sinks()
                        .map(|sinks| sinks.iter().any(|s| s.node_name == node))
                        .unwrap_or(true);
                    if present {
                        set_pipewire_node(Some(node));
                        return self.default_device();
                    }
                    self.selected_uid = None;
                } else {
                    // Non-PipeWire fallback: a direct ALSA PCM. Open it by UID.
                    set_pipewire_node(None);
                    if let Some(device) = self.find_by_uid(&uid)? {
                        self.cached_name = device_display_name(&device);
                        return Ok(Arc::new(device));
                    }
                    // Stale selection: drop it and fall through to default.
                    self.selected_uid = None;
                }
            }
            // Follow the system default sink, but target it *explicitly* so
            // PipeWire's stream-restore can't silently reroute us to a sink the
            // app happened to use before (which otherwise plays to the wrong
            // device — "no sound" from where the user is actually listening).
            set_pipewire_node(pulse_default_sink().as_deref());
            self.default_device()
        }

        #[cfg(not(target_os = "linux"))]
        {
            if let Some(uid) = self.selected_uid.clone()
                && let Some(device) = self.find_by_uid(&uid)?
            {
                self.cached_name = device_display_name(&device);
                return Ok(Arc::new(device));
            }

            // Fallback: clear stale selection and use system default.
            if self.selected_uid.is_some() {
                self.selected_uid = None;
            }
            let default = self.host.default_output_device().ok_or_else(|| {
                AudioError::DeviceNotFound("No default output device".to_string())
            })?;
            self.cached_name = device_display_name(&default);
            Ok(Arc::new(default))
        }
    }

    /// Opens the host default output device (on Linux, the PipeWire ALSA plugin).
    #[cfg(target_os = "linux")]
    fn default_device(&self) -> Result<Arc<cpal::Device>, AudioError> {
        let default = self
            .host
            .default_output_device()
            .ok_or_else(|| AudioError::DeviceNotFound("No default output device".to_string()))?;
        Ok(Arc::new(default))
    }

    /// Returns the UID string to use for exclusive-mode lookups. For "follow
    /// system default" selections, queries the host for the current default
    /// device's UID, so that exclusive mode tracks the system default if the
    /// user hasn't pinned a device.
    pub fn resolve_uid(&mut self) -> Result<String, AudioError> {
        let device = self.resolve_device()?;
        device_uid(&device)
    }

    /// Returns the index of the selected device within the current enumeration
    /// order. Returns `None` if the selection is "follow default" or the
    /// selected UID is no longer present in the enumeration (e.g. unplugged).
    pub fn selected_device_index(&self) -> Option<usize> {
        let uid = self.selected_uid.as_deref()?;
        let devices = self.output_devices().ok()?;
        devices.iter().position(|d| d.uid == uid)
    }

    /// Selects a device by its index in the current enumeration. The captured
    /// UID is what gets stored — index-based selection is only used for the UI
    /// dropdown's "user clicked row N" event.
    pub fn select_device(&mut self, index: usize) -> Result<(), AudioError> {
        let devices = self.output_devices()?;
        let chosen = devices.into_iter().nth(index).ok_or_else(|| {
            AudioError::DeviceNotFound(format!("Device index {} out of range", index))
        })?;
        self.cached_name = chosen.name;
        self.selected_uid = if chosen.uid.is_empty() {
            None
        } else {
            Some(chosen.uid)
        };
        Ok(())
    }

    /// Pins `uid` as the selected device without going through the index-based
    /// UI path. Used when entering exclusive mode to ensure the shared-mode
    /// fallback on exit lands on the same physical device.
    pub fn set_selected_uid(&mut self, uid: String) {
        if let Ok(Some(d)) = self.find_by_uid(&uid) {
            self.cached_name = device_display_name(&d);
        }
        self.selected_uid = Some(uid);
    }

    /// Clears the explicit selection so the manager follows the system default.
    pub fn select_default(&mut self) {
        self.selected_uid = None;
        if let Some(d) = self.host.default_output_device() {
            self.cached_name = device_display_name(&d);
        }
    }

    fn find_by_uid(&self, uid: &str) -> Result<Option<cpal::Device>, AudioError> {
        let devices = self
            .host
            .output_devices()
            .map_err(|e| AudioError::Output(e.to_string()))?;
        for d in devices {
            if let Ok(this_uid) = device_uid(&d)
                && this_uid == uid
            {
                return Ok(Some(d));
            }
        }
        Ok(None)
    }
}

fn device_display_name(d: &cpal::Device) -> String {
    d.description()
        .map(|desc| desc.name().to_string())
        .unwrap_or_else(|_| "Unknown".to_string())
}

fn device_uid(d: &cpal::Device) -> Result<String, AudioError> {
    d.id()
        .map(|id| id.1)
        .map_err(|e| AudioError::DeviceNotFound(format!("Could not read device UID: {}", e)))
}

#[cfg(target_os = "linux")]
fn extract_card(uid: &str) -> Option<String> {
    let after = uid.split("CARD=").nth(1)?;
    let card = after.split([',', ':']).next()?.trim();
    if card.is_empty() {
        None
    } else {
        Some(card.to_string())
    }
}

#[cfg(target_os = "linux")]
fn alsa_card_longnames() -> HashMap<String, String> {
    let mut map = HashMap::new();
    for card in alsa::card::Iter::new().flatten() {
        if let Ok(ctl) = alsa::Ctl::from_card(&card, false)
            && let Ok(info) = ctl.card_info()
            && let (Ok(id), Ok(name)) = (info.get_id(), info.get_name())
        {
            map.insert(id.to_string(), name.to_string());
        }
    }
    map
}

#[cfg(target_os = "linux")]
fn pick_best_uid(devices: &[OutputDeviceInfo]) -> String {
    let uids: Vec<&str> = devices.iter().map(|d| d.uid.as_str()).collect();

    for prefix in &["plughw:", "hw:", "sysdefault:", "front:"] {
        if let Some(uid) = uids.iter().find(|u| u.starts_with(prefix)) {
            return uid.to_string();
        }
    }

    uids.first().map(|s| s.to_string()).unwrap_or_default()
}

/// The always-present "follow system default" entry. Empty UID maps to
/// `selected_uid = None`, so cpal opens the host default device — which on a
/// PipeWire/PulseAudio system routes through the sound server. This is the
/// safety net: it works even if every curated device below fails to open.
#[cfg(target_os = "linux")]
fn system_default_entry() -> OutputDeviceInfo {
    OutputDeviceInfo {
        name: "System Default".to_string(),
        uid: String::new(),
        is_default: true,
    }
}

/// Sets (or clears) the `PIPEWIRE_NODE` env var that the PipeWire ALSA plugin
/// reads at stream-connect time to pick a target sink. Cleared (`None`) means
/// "follow the system default sink".
///
/// Caveat: `set_var`/`remove_var` are `unsafe` in edition 2024 because they race
/// with concurrent `getenv` from other threads, and cpal/PipeWire run their own
/// worker threads. Writers here are serialized (every caller holds the
/// `DeviceManager` write lock and sets this right before building a stream), but
/// that does NOT exclude a library reading the environment concurrently during
/// connect. So this is best-effort routing — it has been observed not to take
/// effect reliably inside the full GUI app — and a non-env mechanism would be
/// more correct (see HANDOFF-linux-exclusive-dac.md). The realtime callback
/// never touches the environment.
#[cfg(target_os = "linux")]
pub(crate) fn set_pipewire_node(node: Option<&str>) {
    unsafe {
        match node {
            Some(n) if !n.is_empty() => std::env::set_var("PIPEWIRE_NODE", n),
            _ => std::env::remove_var("PIPEWIRE_NODE"),
        }
    }
}

/// A PipeWire/PulseAudio output sink, distilled to the fields we need.
#[cfg(target_os = "linux")]
#[derive(Debug, Clone)]
struct PulseSink {
    /// Human-readable name shown by the OS (e.g. "D50 III Headphones").
    description: String,
    /// PipeWire node name, e.g. "alsa_output.usb-Topping_D50_III-00...sink".
    /// Used verbatim as the `PIPEWIRE_NODE` routing target.
    node_name: String,
}

/// Shape of one entry in `pactl -f json list sinks`.
#[cfg(target_os = "linux")]
#[derive(Deserialize)]
struct RawPulseSink {
    name: String,
    description: String,
}

/// Runs `pactl` to enumerate sinks. Returns `None` on any failure (binary
/// missing, non-zero exit, unparseable output) so the caller degrades to the
/// ALSA-card path.
#[cfg(target_os = "linux")]
fn enum_pulse_sinks() -> Option<Vec<PulseSink>> {
    let output = Command::new("pactl")
        .args(["-f", "json", "list", "sinks"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let json = String::from_utf8(output.stdout).ok()?;
    parse_pulse_sinks(&json)
}

/// The current default sink's node name (`pactl get-default-sink`), used to mark
/// one device as the default and to target it explicitly when following default.
/// `None` on any failure.
#[cfg(target_os = "linux")]
pub(crate) fn pulse_default_sink() -> Option<String> {
    let output = Command::new("pactl")
        .args(["get-default-sink"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let name = String::from_utf8(output.stdout).ok()?.trim().to_string();
    (!name.is_empty()).then_some(name)
}

/// Pure parser for `pactl -f json list sinks` output. Split out from
/// `enum_pulse_sinks` so it can be unit-tested against a captured fixture.
#[cfg(target_os = "linux")]
fn parse_pulse_sinks(json: &str) -> Option<Vec<PulseSink>> {
    let raw: Vec<RawPulseSink> = serde_json::from_str(json).ok()?;
    let sinks = raw
        .into_iter()
        .map(|s| PulseSink {
            description: s.description,
            node_name: s.name,
        })
        .collect();
    Some(sinks)
}

/// Builds one entry per sink (no synthetic "System Default" — the sinks already
/// are the system's devices). Each UID is `pw:<node>`, which `resolve_device`
/// routes to via `PIPEWIRE_NODE`. The sink matching `default_node` is flagged
/// `is_default`, so the UI marks it and highlights it while following default.
#[cfg(target_os = "linux")]
fn build_pulse_list(sinks: &[PulseSink], default_node: Option<&str>) -> Vec<OutputDeviceInfo> {
    sinks
        .iter()
        .filter(|s| !s.node_name.is_empty())
        .map(|s| OutputDeviceInfo {
            name: s.description.clone(),
            uid: format!("pw:{}", s.node_name),
            is_default: default_node == Some(s.node_name.as_str()),
        })
        .collect()
}

/// Fallback enumeration: group cpal's raw ALSA PCMs by card, one representative
/// entry per card. `longnames` is passed in (rather than queried) so the pure
/// grouping logic is unit-testable without audio hardware.
#[cfg(target_os = "linux")]
fn curate_alsa_cards(
    raw: Vec<OutputDeviceInfo>,
    _default_uid: Option<&str>,
    longnames: &HashMap<String, String>,
) -> Vec<OutputDeviceInfo> {
    let (card_devices, _non_card): (Vec<_>, Vec<_>) =
        raw.into_iter().partition(|d| d.uid.contains("CARD="));

    let mut groups: BTreeMap<String, Vec<OutputDeviceInfo>> = BTreeMap::new();
    for d in card_devices {
        if let Some(card) = extract_card(&d.uid) {
            groups.entry(card).or_default().push(d);
        }
    }

    let mut result = vec![system_default_entry()];

    for (card_token, devices) in groups {
        let representative = pick_best_uid(&devices);

        let name = longnames
            .get(&card_token)
            .cloned()
            .or_else(|| {
                devices.iter().find_map(|d| {
                    let n = d.name.as_str();
                    if !n.is_empty() && !n.contains("Default") && !n.contains("HDA Intel") {
                        Some(d.name.clone())
                    } else {
                        None
                    }
                })
            })
            .unwrap_or_else(|| card_token.clone());

        result.push(OutputDeviceInfo {
            name,
            uid: representative,
            is_default: false,
        });
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_manager() -> DeviceManager {
        let host = Arc::new(cpal::default_host());
        DeviceManager::from_host(&host).expect("dev host must have a default device")
    }

    #[test]
    fn fresh_manager_follows_default() {
        let m = make_manager();
        assert!(
            m.selected_uid().is_none(),
            "new manager should follow system default (no explicit UID pinned)"
        );
        assert!(
            !m.selected_device_name().is_empty(),
            "default device should have a name"
        );
    }

    #[test]
    fn resolve_device_returns_a_device_when_following_default() {
        let mut m = make_manager();
        let _device = m.resolve_device().expect("default device should resolve");
        // Display name should now match the default device's name (cached during resolve).
        assert!(!m.selected_device_name().is_empty());
    }

    #[test]
    fn resolve_uid_returns_non_empty_string_for_default() {
        let mut m = make_manager();
        let uid = m.resolve_uid().expect("default device should expose a UID");
        assert!(
            !uid.is_empty(),
            "system default device UID must be non-empty"
        );
    }

    #[test]
    fn selected_device_index_is_none_when_following_default() {
        let m = make_manager();
        assert_eq!(m.selected_device_index(), None);
    }

    #[test]
    fn select_default_clears_pinned_uid() {
        let mut m = make_manager();
        // Pretend something was pinned, then clear.
        m.selected_uid = Some("fake-uid-doesnt-matter".to_string());
        m.select_default();
        assert!(m.selected_uid().is_none());
    }

    #[test]
    fn resolve_falls_back_to_default_when_pinned_uid_is_unknown() {
        let mut m = make_manager();
        m.selected_uid = Some("definitely-not-a-real-device-uid-xyz123".to_string());
        let _device = m
            .resolve_device()
            .expect("fallback to default must succeed");
        assert!(
            m.selected_uid().is_none(),
            "stale selection should be cleared after fallback"
        );
    }

    #[test]
    fn output_devices_returns_at_least_one_on_dev_machine() {
        let m = make_manager();
        let devices = m.output_devices().expect("enumeration must succeed");
        assert!(
            !devices.is_empty(),
            "dev machine should expose at least one output device"
        );
        let default_count = devices.iter().filter(|d| d.is_default).count();
        assert!(
            default_count <= 1,
            "at most one device should be marked default"
        );
    }

    #[cfg(target_os = "linux")]
    fn dev(uid: &str, name: &str) -> OutputDeviceInfo {
        OutputDeviceInfo {
            name: name.to_string(),
            uid: uid.to_string(),
            is_default: false,
        }
    }

    /// A cpal-style "flood" enumeration for a 3-card system (PCH analog/digital,
    /// ATI HDMI, D50 III USB), mirroring what `host.output_devices()` yields.
    #[cfg(target_os = "linux")]
    fn cpal_flood() -> Vec<OutputDeviceInfo> {
        vec![
            dev("default", "Default"),
            dev("pipewire", "PipeWire"),
            dev("pulse", "PulseAudio"),
            // PCH
            dev("sysdefault:CARD=PCH", "HDA Intel PCH, Analog"),
            dev("front:CARD=PCH,DEV=0", "Front"),
            dev("surround40:CARD=PCH,DEV=0", "Surround 4.0"),
            dev("iec958:CARD=PCH,DEV=0", "IEC958 Digital"),
            dev("hw:CARD=PCH,DEV=0", "hw PCH"),
            dev("plughw:CARD=PCH,DEV=0", "plughw PCH"),
            // HDMI (ATI), several ports
            dev("hdmi:CARD=HDMI,DEV=0", "HDMI 0"),
            dev("hdmi:CARD=HDMI,DEV=1", "HDMI 1"),
            dev("hw:CARD=HDMI,DEV=3", "hw HDMI"),
            dev("plughw:CARD=HDMI,DEV=3", "plughw HDMI"),
            // D50 III (USB)
            dev("sysdefault:CARD=III", "D50 III"),
            dev("front:CARD=III,DEV=0", "Front"),
            dev("hw:CARD=III,DEV=0", "hw III"),
            dev("plughw:CARD=III,DEV=0", "plughw III"),
        ]
    }

    #[cfg(target_os = "linux")]
    const PACTL_FIXTURE: &str = r#"[
        {"index":53,"name":"alsa_output.pci-0000_00_1f.3.iec958-stereo",
         "description":"Built-in Audio Digital Stereo (IEC958)",
         "properties":{"alsa.id":"PCH","api.alsa.path":"iec958:0"}},
        {"index":52,"name":"alsa_output.usb-Topping_D50_III-00.HiFi__Headphones__sink",
         "description":"D50 III Headphones",
         "properties":{"alsa.id":"III","api.alsa.path":"hw:III,0"}},
        {"index":51,"name":"alsa_output.pci-0000_04_00.1.hdmi-stereo-extra1",
         "description":"Navi 48 HDMI/DP Audio Controller Digital Stereo (HDMI 2)",
         "properties":{"alsa.id":"HDMI","api.alsa.path":"hdmi:1,1"}}
    ]"#;

    #[cfg(target_os = "linux")]
    #[test]
    fn parse_pulse_sinks_extracts_description_and_node_name() {
        let sinks = parse_pulse_sinks(PACTL_FIXTURE).expect("fixture must parse");
        assert_eq!(sinks.len(), 3);
        assert_eq!(
            sinks[0].description,
            "Built-in Audio Digital Stereo (IEC958)"
        );
        assert_eq!(
            sinks[0].node_name,
            "alsa_output.pci-0000_00_1f.3.iec958-stereo"
        );
        assert_eq!(
            sinks[1].node_name,
            "alsa_output.usb-Topping_D50_III-00.HiFi__Headphones__sink"
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn parse_pulse_sinks_rejects_garbage() {
        assert!(parse_pulse_sinks("not json").is_none());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn build_pulse_list_is_pw_routed_sinks_with_one_default() {
        let sinks = parse_pulse_sinks(PACTL_FIXTURE).unwrap();
        let default = "alsa_output.usb-Topping_D50_III-00.HiFi__Headphones__sink";
        let list = build_pulse_list(&sinks, Some(default));

        // No synthetic "System Default" — just the three real sinks.
        assert_eq!(list.len(), 3);
        let names: Vec<&str> = list.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(
            names,
            vec![
                "Built-in Audio Digital Stereo (IEC958)",
                "D50 III Headphones",
                "Navi 48 HDMI/DP Audio Controller Digital Stereo (HDMI 2)",
            ]
        );
        let uids: Vec<&str> = list.iter().map(|d| d.uid.as_str()).collect();
        assert_eq!(
            uids,
            vec![
                "pw:alsa_output.pci-0000_00_1f.3.iec958-stereo",
                "pw:alsa_output.usb-Topping_D50_III-00.HiFi__Headphones__sink",
                "pw:alsa_output.pci-0000_04_00.1.hdmi-stereo-extra1",
            ]
        );
        // Exactly the D50 sink is flagged default.
        let defaults: Vec<&str> = list
            .iter()
            .filter(|d| d.is_default)
            .map(|d| d.name.as_str())
            .collect();
        assert_eq!(defaults, vec!["D50 III Headphones"]);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn build_pulse_list_skips_sinks_without_node_name() {
        let sinks = vec![PulseSink {
            description: "Ghost".into(),
            node_name: String::new(),
        }];
        let list = build_pulse_list(&sinks, None);
        assert!(list.is_empty());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn build_pulse_list_marks_no_default_when_unknown() {
        let sinks = parse_pulse_sinks(PACTL_FIXTURE).unwrap();
        let list = build_pulse_list(&sinks, None);
        assert_eq!(list.len(), 3);
        assert!(list.iter().all(|d| !d.is_default));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn curate_alsa_cards_groups_one_entry_per_card() {
        let raw = cpal_flood();
        let mut longnames = HashMap::new();
        longnames.insert("PCH".to_string(), "HDA Intel PCH".to_string());
        longnames.insert("HDMI".to_string(), "HDA ATI HDMI".to_string());
        longnames.insert("III".to_string(), "D50 III".to_string());

        let list = curate_alsa_cards(raw, None, &longnames);

        assert_eq!(list[0].name, "System Default");
        let named: Vec<(&str, &str)> = list[1..]
            .iter()
            .map(|d| (d.name.as_str(), d.uid.as_str()))
            .collect();
        // BTreeMap order: HDMI, III, PCH. pick_best_uid prefers plughw: > hw:.
        assert_eq!(
            named,
            vec![
                ("HDA ATI HDMI", "plughw:CARD=HDMI,DEV=3"),
                ("D50 III", "plughw:CARD=III,DEV=0"),
                ("HDA Intel PCH", "plughw:CARD=PCH,DEV=0"),
            ]
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn curate_alsa_cards_empty_input_is_just_default() {
        let list = curate_alsa_cards(vec![dev("default", "Default")], None, &HashMap::new());
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].name, "System Default");
    }
}
