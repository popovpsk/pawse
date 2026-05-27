use std::sync::Arc;

use audio_common::AudioError;
use cpal::traits::{DeviceTrait, HostTrait};

#[cfg(target_os = "linux")]
use std::collections::{BTreeMap, HashMap};

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

    /// Live enumeration. Always re-queries the host so newly attached devices
    /// appear immediately without a manual refresh step.
    pub fn output_devices(&self) -> Result<Vec<OutputDeviceInfo>, AudioError> {
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

        #[cfg(target_os = "linux")]
        {
            return Ok(curate_linux(devices, default_uid.as_deref()));
        }

        #[cfg(not(target_os = "linux"))]
        {
            Ok(devices)
        }
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
    pub fn resolve_device(&mut self) -> Result<Arc<cpal::Device>, AudioError> {
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
        let default = self
            .host
            .default_output_device()
            .ok_or_else(|| AudioError::DeviceNotFound("No default output device".to_string()))?;
        self.cached_name = device_display_name(&default);
        Ok(Arc::new(default))
    }

    /// The device shared-mode streams should open. On Linux this is always the
    /// system default (PipeWire): routing a specific ALSA card in shared mode
    /// bypasses PipeWire and causes stutter/EBUSY, so the per-card selection is
    /// reserved for exclusive mode. macOS/Windows honor the explicit pick.
    pub fn resolve_shared_device(&mut self) -> Result<Arc<cpal::Device>, AudioError> {
        #[cfg(target_os = "linux")]
        {
            let default = self.host.default_output_device().ok_or_else(|| {
                AudioError::DeviceNotFound("No default output device".to_string())
            })?;
            return Ok(Arc::new(default));
        }
        #[cfg(not(target_os = "linux"))]
        {
            self.resolve_device()
        }
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

#[cfg(target_os = "linux")]
fn curate_linux(raw: Vec<OutputDeviceInfo>, _default_uid: Option<&str>) -> Vec<OutputDeviceInfo> {
    let longnames = alsa_card_longnames();

    let (card_devices, _non_card): (Vec<_>, Vec<_>) =
        raw.into_iter().partition(|d| d.uid.contains("CARD="));

    let mut groups: BTreeMap<String, Vec<OutputDeviceInfo>> = BTreeMap::new();
    for d in card_devices {
        if let Some(card) = extract_card(&d.uid) {
            groups.entry(card).or_default().push(d);
        }
    }

    let mut result = Vec::new();

    result.push(OutputDeviceInfo {
        name: "System Default".to_string(),
        uid: String::new(),
        is_default: true,
    });

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
}
