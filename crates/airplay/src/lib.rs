use std::net::{IpAddr, SocketAddr};

mod alac;
mod rtp;
mod rtsp;
mod stream;

pub use stream::{Render, Stream, StreamEvent};

pub const SERVICE_TYPE: &str = "_raop._tcp.local.";
pub const SAMPLE_RATE: u32 = 44_100;
pub const CHANNELS: usize = 2;
pub const FRAMES_PER_PACKET: usize = 352;
pub const LATENCY_FRAMES: u32 = 88_200;

#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum Error {
    #[error("{0}")]
    Io(String),
    #[error("the device asks for a password")]
    PasswordRequired,
    #[error("the device refused: {0}")]
    Refused(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Device {
    pub id: String,
    pub name: String,
    pub model: Option<String>,
    pub address: SocketAddr,
}

fn listed(txt: Option<&str>, wanted: &str) -> bool {
    txt.is_none_or(|list| list.split(',').any(|item| item.trim() == wanted))
}

impl Device {
    pub fn from_service<'a>(
        instance: &str,
        addresses: impl IntoIterator<Item = IpAddr>,
        port: u16,
        txt: impl Fn(&str) -> Option<&'a str>,
    ) -> Option<Self> {
        let mut addresses: Vec<IpAddr> = addresses.into_iter().collect();
        addresses.sort_by_key(|address| !address.is_ipv4());
        let address = *addresses.first()?;
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
        Some(Device {
            id: id.to_string(),
            name: name.to_string(),
            model,
            address: SocketAddr::new(address, port),
        })
    }
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
            ],
        )
        .unwrap();
        assert_eq!(found.id, "2863813C4503");
        assert_eq!(found.name, "Pi AirPlay");
        assert_eq!(found.model.as_deref(), Some("ShairportSync"));
        assert_eq!(found.address, "192.168.3.22:7000".parse().unwrap());
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

    #[test]
    fn volume_maps_to_the_airplay_decibel_range() {
        assert_eq!(volume_db(1.0), 0.0);
        assert_eq!(volume_db(0.5), -15.0);
        assert_eq!(volume_db(0.0), -144.0);
    }
}
