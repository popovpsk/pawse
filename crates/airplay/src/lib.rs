use std::net::{IpAddr, SocketAddr};

mod alac;
mod events;
#[cfg(test)]
mod fake;
mod handshake;
mod metadata;
mod pairing;
mod rtp;
mod rtsp;
mod secure;
mod srp;
mod stream;
mod tlv;

pub use metadata::{Cover, NowPlaying, RemoteCommand, dacp_command};
pub use stream::{Render, Stream, StreamEvent};

pub const RAOP_SERVICE_TYPE: &str = "_raop._tcp.local.";
pub const AIRPLAY_SERVICE_TYPE: &str = "_airplay._tcp.local.";
pub const SAMPLE_RATE: u32 = 44_100;
pub const CHANNELS: usize = 2;
pub const FRAMES_PER_PACKET: usize = 352;
pub const LATENCY_FRAMES: u32 = 88_200;

const FEATURE_AUDIO: u32 = 9;
const FEATURE_METADATA_ARTWORK: u32 = 15;
const FEATURE_METADATA_PROGRESS: u32 = 16;
const FEATURE_METADATA_TEXT: u32 = 17;
const FEATURE_UNIFIED_MEDIA_CONTROL: u32 = 38;
const FEATURE_HOMEKIT_PAIRING: u32 = 46;
const FEATURE_COREUTILS_PAIRING: u32 = 48;
const FLAG_PIN_REQUIRED: u64 = 0x8;
const FLAG_PASSWORD: u64 = 0x80;

#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum Error {
    #[error("{0}")]
    Io(String),
    #[error("the device asks for a password")]
    PasswordRequired,
    #[error("the device refused: {0}")]
    Refused(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Protocol {
    Raop,
    AirPlay2,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Shows {
    pub text: bool,
    pub artwork: bool,
    pub progress: bool,
}

impl Shows {
    pub const ALL: Self = Self {
        text: true,
        artwork: true,
        progress: true,
    };
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Device {
    pub id: String,
    pub name: String,
    pub model: Option<String>,
    pub address: SocketAddr,
    pub protocol: Protocol,
    pub shows: Shows,
}

pub(crate) fn random<const N: usize>() -> [u8; N] {
    let mut bytes = [0u8; N];
    if getrandom::fill(&mut bytes).is_err() {
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|since| since.as_nanos())
            .unwrap_or_default();
        for (i, byte) in bytes.iter_mut().enumerate() {
            *byte = (seed >> ((i % 16) * 8)) as u8;
        }
    }
    bytes
}

fn listed(txt: Option<&str>, wanted: &str) -> bool {
    txt.is_none_or(|list| list.split(',').any(|item| item.trim() == wanted))
}

fn preferred_address(addresses: impl IntoIterator<Item = IpAddr>) -> Option<IpAddr> {
    let mut addresses: Vec<IpAddr> = addresses.into_iter().collect();
    addresses.sort_by_key(|address| !address.is_ipv4());
    addresses.first().copied()
}

fn hex_number(text: &str) -> Option<u64> {
    let text = text.trim();
    let digits = text
        .strip_prefix("0x")
        .or_else(|| text.strip_prefix("0X"))
        .unwrap_or(text);
    u64::from_str_radix(digits, 16).ok()
}

fn features(text: &str) -> Option<u64> {
    match text.split_once(',') {
        Some((low, high)) => Some((hex_number(low)? & 0xffff_ffff) | (hex_number(high)? << 32)),
        None => hex_number(text),
    }
}

impl Device {
    pub fn from_service<'a>(
        instance: &str,
        addresses: impl IntoIterator<Item = IpAddr>,
        port: u16,
        txt: impl Fn(&str) -> Option<&'a str>,
    ) -> Option<Self> {
        let address = preferred_address(addresses)?;
        let (id, name) = match instance.split_once('@') {
            Some((id, name)) if !name.trim().is_empty() => (id.trim(), name.trim()),
            _ => (instance.trim(), instance.trim()),
        };
        if id.is_empty() {
            return None;
        }
        let password = txt("pw").is_some_and(|pw| pw.eq_ignore_ascii_case("true"));
        let plain = listed(txt("et"), "0");
        let alac = listed(txt("cn"), "1");
        let udp =
            txt("tp").is_none_or(|transports| transports.to_ascii_uppercase().contains("UDP"));
        let format = txt("sr").is_none_or(|rate| rate.trim() == "44100")
            && txt("ss").is_none_or(|bits| bits.trim() == "16")
            && txt("ch").is_none_or(|channels| channels.trim() == "2");
        if password || !plain || !alac || !udp || !format {
            log::debug!(
                "AirPlay: {name} is not supported (pw {:?}, et {:?}, cn {:?}, tp {:?})",
                txt("pw"),
                txt("et"),
                txt("cn"),
                txt("tp")
            );
            return None;
        }
        let model = txt("am")
            .map(str::trim)
            .filter(|model| !model.is_empty())
            .map(str::to_string);
        let shows = |kind| txt("md").is_some_and(|kinds| listed(Some(kinds), kind));
        Some(Device {
            id: id.to_string(),
            name: name.to_string(),
            model,
            shows: Shows {
                text: shows("0"),
                artwork: shows("1"),
                progress: shows("2"),
            },
            address: SocketAddr::new(address, port),
            protocol: Protocol::Raop,
        })
    }

    pub fn from_airplay_service<'a>(
        instance: &str,
        addresses: impl IntoIterator<Item = IpAddr>,
        port: u16,
        txt: impl Fn(&str) -> Option<&'a str>,
    ) -> Option<Self> {
        let address = preferred_address(addresses)?;
        let name = instance.trim();
        let id: String = txt("deviceid")?
            .chars()
            .filter(char::is_ascii_hexdigit)
            .map(|digit| digit.to_ascii_uppercase())
            .collect();
        if name.is_empty() || id.len() != 12 {
            return None;
        }
        let features = txt("features")
            .or_else(|| txt("ft"))
            .and_then(features)
            .unwrap_or(0);
        let flags = txt("flags")
            .or_else(|| txt("sf"))
            .and_then(hex_number)
            .unwrap_or(0);
        let has = |bit: u32| features & (1 << bit) != 0;
        let audio = has(FEATURE_AUDIO);
        let airplay2 = has(FEATURE_UNIFIED_MEDIA_CONTROL) || has(FEATURE_COREUTILS_PAIRING);
        let pairs = has(FEATURE_HOMEKIT_PAIRING) || has(FEATURE_COREUTILS_PAIRING);
        let code = flags & FLAG_PIN_REQUIRED != 0;
        let password = flags & FLAG_PASSWORD != 0
            || txt("pw").is_some_and(|pw| pw.eq_ignore_ascii_case("true"));
        let restricted = txt("acl").is_some_and(|acl| acl.trim() != "0")
            || txt("act").is_some_and(|act| act.trim() == "2");
        if !audio || !airplay2 || !pairs || code || password || restricted {
            log::debug!(
                "AirPlay: {name} is not supported (features {features:#x}, flags {flags:#x}, acl {:?}, act {:?}, pw {:?})",
                txt("acl"),
                txt("act"),
                txt("pw")
            );
            return None;
        }
        let model = txt("model")
            .map(str::trim)
            .filter(|model| !model.is_empty())
            .map(str::to_string);
        Some(Device {
            id,
            name: name.to_string(),
            model,
            address: SocketAddr::new(address, port),
            protocol: Protocol::AirPlay2,
            shows: Shows {
                text: has(FEATURE_METADATA_TEXT),
                artwork: has(FEATURE_METADATA_ARTWORK),
                progress: has(FEATURE_METADATA_PROGRESS),
            },
        })
    }
}

pub fn dacp_id() -> String {
    handshake::dacp_id()
}

pub fn volume_db(volume: f32) -> f32 {
    if volume <= 0.001 {
        -144.0
    } else {
        -30.0 + 30.0 * volume.clamp(0.0, 1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn device(instance: &str, txt: &[(&str, &str)]) -> Option<Device> {
        let txt: HashMap<String, String> = txt
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        Device::from_service(
            instance,
            ["fe80::1".parse().unwrap(), "192.168.3.22".parse().unwrap()],
            7000,
            |key| txt.get(key).map(String::as_str),
        )
    }

    #[test]
    fn a_shairport_instance_is_a_supported_device() {
        let found = device(
            "2863813C4503@Pi AirPlay",
            &[
                ("et", "0,1"),
                ("cn", "0,1"),
                ("tp", "UDP"),
                ("sr", "44100"),
                ("ss", "16"),
                ("ch", "2"),
                ("am", "ShairportSync"),
                ("md", "0,1,2"),
            ],
        )
        .unwrap();
        assert_eq!(found.id, "2863813C4503");
        assert_eq!(found.name, "Pi AirPlay");
        assert_eq!(found.model.as_deref(), Some("ShairportSync"));
        assert_eq!(found.address, "192.168.3.22:7000".parse().unwrap());
        assert_eq!(found.shows, Shows::ALL);
    }

    #[test]
    fn a_raop_speaker_shows_only_the_metadata_its_md_lists() {
        let shows = |md: Option<&str>| {
            let mut txt = vec![("et", "0")];
            txt.extend(md.map(|md| ("md", md)));
            device("AA@Speaker", &txt).unwrap().shows
        };
        assert_eq!(shows(None), Shows::default());
        assert_eq!(
            shows(Some("0,2")),
            Shows {
                text: true,
                artwork: false,
                progress: true,
            }
        );
    }

    #[test]
    fn devices_that_need_encryption_or_a_password_are_left_out() {
        assert!(device("AA@Speaker", &[("et", "1,3")]).is_none());
        assert!(device("AA@Speaker", &[("pw", "true")]).is_none());
        assert!(device("AA@Speaker", &[("cn", "0,2")]).is_none());
        assert!(device("AA@Speaker", &[("tp", "TCP")]).is_none());
        assert!(device("AA@Speaker", &[("sr", "48000")]).is_none());
        assert!(device("AA@Speaker", &[]).is_some());
    }

    fn airplay(txt: &[(&str, &str)]) -> Option<Device> {
        let txt: HashMap<String, String> = txt
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        Device::from_airplay_service(
            "Guest Room TV",
            ["192.168.3.7".parse().unwrap()],
            7000,
            |key| txt.get(key).map(String::as_str),
        )
    }

    const TV: [(&str, &str); 5] = [
        ("deviceid", "4F:CB:77:B2:06:25"),
        ("features", "0x7F8AD0,0x38BCF46"),
        ("flags", "0x244"),
        ("acl", "0"),
        ("model", "55U7SE"),
    ];

    #[test]
    fn an_airplay_2_tv_is_a_device_with_the_raop_style_id() {
        let tv = airplay(&TV).unwrap();
        assert_eq!(tv.id, "4FCB77B20625");
        assert_eq!(tv.name, "Guest Room TV");
        assert_eq!(tv.model.as_deref(), Some("55U7SE"));
        assert_eq!(tv.protocol, Protocol::AirPlay2);
        assert_eq!(tv.address, "192.168.3.7:7000".parse().unwrap());
        assert_eq!(tv.shows, Shows::ALL);
        let mut quiet = TV;
        quiet[1] = ("features", "0x7C0AD0,0x38BCF46");
        assert_eq!(airplay(&quiet).unwrap().shows, Shows::default());
    }

    #[test]
    fn airplay_2_devices_that_need_a_code_a_password_or_a_home_are_left_out() {
        let with = |key: &'static str, value: &'static str| {
            let mut txt: Vec<(&str, &str)> =
                TV.iter().filter(|(k, _)| *k != key).copied().collect();
            txt.push((key, value));
            airplay(&txt)
        };
        assert!(with("flags", "0x8").is_none());
        assert!(with("flags", "0x80").is_none());
        assert!(with("pw", "true").is_none());
        assert!(with("acl", "1").is_none());
        assert!(with("act", "2").is_none());
        assert!(with("features", "0x5A7FFFF7,0x1E").is_none());
        assert!(with("features", "0x7F88D0,0x38BCF46").is_none());
        assert!(with("deviceid", "nonsense").is_none());
        assert!(with("flags", "0x204").is_some());
    }

    #[test]
    fn features_are_two_32_bit_halves_or_one_number() {
        assert_eq!(features("0x7F8AD0,0x38BCF46"), Some(0x038B_CF46_007F_8AD0));
        assert_eq!(features("0x445F8A00"), Some(0x445F_8A00));
        assert_eq!(features("0x1,zz"), None);
    }

    #[test]
    fn volume_maps_to_the_airplay_decibel_range() {
        assert_eq!(volume_db(1.0), 0.0);
        assert_eq!(volume_db(0.5), -15.0);
        assert_eq!(volume_db(0.0), -144.0);
    }
}
