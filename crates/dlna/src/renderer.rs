use std::net::Ipv4Addr;
use std::time::Duration;

use crate::device::{RendererDescription, Service};
use crate::soap::{self, Failure};
use crate::{Device, Error, didl, read_text, ssdp, xml};

const CONTROL_TIMEOUT: Duration = Duration::from_secs(5);
const DESCRIBE_TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransportState {
    Stopped,
    Playing,
    Paused,
    Transitioning,
    NoMedia,
    Other(String),
}

impl TransportState {
    fn parse(text: &str) -> Self {
        match text.trim() {
            "STOPPED" => TransportState::Stopped,
            "PLAYING" => TransportState::Playing,
            "PAUSED_PLAYBACK" | "PAUSED_RECORDING" => TransportState::Paused,
            "TRANSITIONING" => TransportState::Transitioning,
            "NO_MEDIA_PRESENT" => TransportState::NoMedia,
            other => TransportState::Other(other.to_string()),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PositionInfo {
    pub uri: Option<String>,
    pub position: Option<Duration>,
    pub duration: Option<Duration>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TrackMetadata {
    pub title: String,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub cover_url: Option<String>,
    pub duration: Option<Duration>,
    pub size: Option<u64>,
}

pub struct Renderer {
    agent: ureq::Agent,
    description: RendererDescription,
}

fn agent(timeout: Duration) -> ureq::Agent {
    ureq::Agent::new_with_config(
        ureq::Agent::config_builder()
            .timeout_global(Some(timeout))
            .http_status_as_error(false)
            .build(),
    )
}

fn fetch_renderer(agent: &ureq::Agent, location: &str) -> Result<RendererDescription, Error> {
    let response = agent
        .get(location)
        .call()
        .map_err(|e| Error::Transient(e.to_string()))?;
    let status = response.status().as_u16();
    if !(200..300).contains(&status) {
        return Err(Error::Server(format!("HTTP {status}")));
    }
    let text = read_text(response.into_body())?;
    crate::device::parse_renderer(location, &text).map_err(Error::Server)
}

pub fn describe_renderer(location: &str) -> Result<Device, Error> {
    fetch_renderer(&agent(DESCRIBE_TIMEOUT), location).map(|description| description.device)
}

pub fn discover_renderers(hosts: &[Ipv4Addr], timeout: Duration) -> Vec<Device> {
    let agent = agent(DESCRIBE_TIMEOUT);
    let mut devices: Vec<Device> = Vec::new();
    for reply in ssdp::search(&ssdp::RENDERER_TARGETS, hosts, timeout, None) {
        if devices
            .iter()
            .any(|device| device.udn.eq_ignore_ascii_case(&reply.udn))
        {
            continue;
        }
        match fetch_renderer(&agent, &reply.location) {
            Ok(description)
                if !devices
                    .iter()
                    .any(|device| device.udn.eq_ignore_ascii_case(&description.device.udn)) =>
            {
                devices.push(description.device);
            }
            Ok(_) => {}
            Err(e) => log::debug!(
                "SSDP: renderer {} did not describe itself: {e}",
                reply.location
            ),
        }
    }
    devices
}

fn time(position: Duration) -> String {
    let total = position.as_secs();
    format!(
        "{}:{:02}:{:02}",
        total / 3600,
        (total / 60) % 60,
        total % 60
    )
}

fn precise_time(position: Duration) -> String {
    format!("{}.{:03}", time(position), position.subsec_millis())
}

fn parse_time(text: Option<&String>) -> Option<Duration> {
    let text = text?.trim();
    if text.is_empty() || text.eq_ignore_ascii_case("NOT_IMPLEMENTED") {
        return None;
    }
    didl::parse_duration(text).map(Duration::from_millis)
}

pub fn track_didl(uri: &str, protocol_info: &str, metadata: &TrackMetadata) -> String {
    let mut item = String::new();
    item.push_str(&format!(
        "<dc:title>{}</dc:title>",
        xml::escape(&metadata.title)
    ));
    if let Some(artist) = &metadata.artist {
        let artist = xml::escape(artist);
        item.push_str(&format!(
            "<upnp:artist>{artist}</upnp:artist><dc:creator>{artist}</dc:creator>"
        ));
    }
    if let Some(album) = &metadata.album {
        item.push_str(&format!("<upnp:album>{}</upnp:album>", xml::escape(album)));
    }
    if let Some(cover) = &metadata.cover_url {
        item.push_str(&format!(
            "<upnp:albumArtURI>{}</upnp:albumArtURI>",
            xml::escape(cover)
        ));
    }
    item.push_str("<upnp:class>object.item.audioItem.musicTrack</upnp:class>");
    let mut res = format!("<res protocolInfo=\"{}\"", xml::escape(protocol_info));
    if let Some(duration) = metadata.duration {
        res.push_str(&format!(" duration=\"{}\"", precise_time(duration)));
    }
    if let Some(size) = metadata.size {
        res.push_str(&format!(" size=\"{size}\""));
    }
    res.push_str(&format!(">{}</res>", xml::escape(uri)));
    format!(
        "<DIDL-Lite xmlns=\"urn:schemas-upnp-org:metadata-1-0/DIDL-Lite/\" \
xmlns:dc=\"http://purl.org/dc/elements/1.1/\" \
xmlns:upnp=\"urn:schemas-upnp-org:metadata-1-0/upnp/\" \
xmlns:dlna=\"urn:schemas-dlna-org:metadata-1-0/\">\
<item id=\"pawse\" parentID=\"0\" restricted=\"1\">{item}{res}</item></DIDL-Lite>"
    )
}

impl Renderer {
    pub fn connect(location: &str) -> Result<Self, Error> {
        let description = fetch_renderer(&agent(DESCRIBE_TIMEOUT), location)?;
        Ok(Self {
            agent: agent(CONTROL_TIMEOUT),
            description,
        })
    }

    pub fn device(&self) -> &Device {
        &self.description.device
    }

    fn call(
        &self,
        service: &Service,
        action: &str,
        args: &[(&str, &str)],
    ) -> Result<soap::Args, Error> {
        soap::call(
            &self.agent,
            &service.control,
            &service.service_type,
            action,
            args,
        )
        .map_err(Failure::into_error)
    }

    fn transport(&self, action: &str, args: &[(&str, &str)]) -> Result<soap::Args, Error> {
        let mut all = vec![("InstanceID", "0")];
        all.extend_from_slice(args);
        self.call(&self.description.av_transport, action, &all)
    }

    pub fn sink_protocols(&self) -> Result<Vec<String>, Error> {
        let Some(service) = &self.description.connection_manager else {
            return Ok(Vec::new());
        };
        let args = self.call(service, "GetProtocolInfo", &[])?;
        Ok(args
            .get("Sink")
            .map(|sink| {
                sink.split(',')
                    .map(str::trim)
                    .filter(|entry| !entry.is_empty())
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default())
    }

    pub fn set_uri(&self, uri: &str, metadata: &str) -> Result<(), Error> {
        self.transport(
            "SetAVTransportURI",
            &[("CurrentURI", uri), ("CurrentURIMetaData", metadata)],
        )
        .map(|_| ())
    }

    pub fn play(&self) -> Result<(), Error> {
        self.transport("Play", &[("Speed", "1")]).map(|_| ())
    }

    pub fn pause(&self) -> Result<(), Error> {
        self.transport("Pause", &[]).map(|_| ())
    }

    pub fn stop(&self) -> Result<(), Error> {
        self.transport("Stop", &[]).map(|_| ())
    }

    pub fn seek(&self, position: Duration) -> Result<(), Error> {
        let target = time(position);
        self.transport("Seek", &[("Unit", "REL_TIME"), ("Target", &target)])
            .map(|_| ())
    }

    pub fn transport_state(&self) -> Result<TransportState, Error> {
        let args = self.transport("GetTransportInfo", &[])?;
        Ok(TransportState::parse(
            args.get("CurrentTransportState")
                .map(String::as_str)
                .unwrap_or_default(),
        ))
    }

    pub fn position(&self) -> Result<PositionInfo, Error> {
        let args = self.transport("GetPositionInfo", &[])?;
        Ok(PositionInfo {
            uri: args
                .get("TrackURI")
                .map(|uri| uri.trim().to_string())
                .filter(|uri| !uri.is_empty()),
            position: parse_time(args.get("RelTime")),
            duration: parse_time(args.get("TrackDuration")),
        })
    }

    pub fn volume(&self) -> Result<Option<u8>, Error> {
        let Some(service) = &self.description.rendering_control else {
            return Ok(None);
        };
        let args = self.call(
            service,
            "GetVolume",
            &[("InstanceID", "0"), ("Channel", "Master")],
        )?;
        Ok(args
            .get("CurrentVolume")
            .and_then(|volume| volume.trim().parse::<u32>().ok())
            .map(|volume| volume.min(100) as u8))
    }

    pub fn set_volume(&self, percent: u8) -> Result<(), Error> {
        let Some(service) = &self.description.rendering_control else {
            return Ok(());
        };
        let volume = percent.min(100).to_string();
        self.call(
            service,
            "SetVolume",
            &[
                ("InstanceID", "0"),
                ("Channel", "Master"),
                ("DesiredVolume", &volume),
            ],
        )
        .map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn times_are_written_as_upnp_clock_values() {
        assert_eq!(time(Duration::from_millis(3_723_900)), "1:02:03");
        assert_eq!(time(Duration::ZERO), "0:00:00");
        assert_eq!(precise_time(Duration::from_millis(61_250)), "0:01:01.250");
    }

    #[test]
    fn positions_that_are_not_implemented_are_unknown() {
        assert_eq!(parse_time(Some(&"NOT_IMPLEMENTED".into())), None);
        assert_eq!(parse_time(Some(&"".into())), None);
        assert_eq!(
            parse_time(Some(&"0:03:05.5".into())),
            Some(Duration::from_millis(185_500))
        );
    }

    #[test]
    fn transport_states_map_to_the_known_ones() {
        assert_eq!(
            TransportState::parse("PAUSED_PLAYBACK"),
            TransportState::Paused
        );
        assert_eq!(TransportState::parse(" PLAYING "), TransportState::Playing);
        assert_eq!(
            TransportState::parse("NO_MEDIA_PRESENT"),
            TransportState::NoMedia
        );
        assert_eq!(
            TransportState::parse("CUSTOM"),
            TransportState::Other("CUSTOM".into())
        );
    }

    #[test]
    fn metadata_is_escaped_into_a_music_track_item() {
        let didl = track_didl(
            "http://10.0.0.2:5000/a?b=1&c=2",
            "http-get:*:audio/flac:*",
            &TrackMetadata {
                title: "Rock & Roll".into(),
                artist: Some("AC/DC".into()),
                album: None,
                cover_url: None,
                duration: Some(Duration::from_millis(201_000)),
                size: Some(1234),
            },
        );
        assert!(didl.contains("<dc:title>Rock &amp; Roll</dc:title>"));
        assert!(didl.contains("<upnp:artist>AC/DC</upnp:artist>"));
        assert!(didl.contains("duration=\"0:03:21.000\" size=\"1234\""));
        assert!(didl.contains(">http://10.0.0.2:5000/a?b=1&amp;c=2</res>"));
        assert!(xml::parse(&didl).is_ok());
    }

    #[test]
    fn a_renderer_description_finds_its_services() {
        let text = r#"<?xml version="1.0"?>
<root xmlns="urn:schemas-upnp-org:device-1-0"><device>
<deviceType>urn:schemas-upnp-org:device:MediaRenderer:1</deviceType>
<friendlyName>Pi Renderer</friendlyName><UDN>uuid:pi</UDN>
<serviceList>
<service><serviceType>urn:schemas-upnp-org:service:ConnectionManager:1</serviceType><controlURL>/upnp/control/cm</controlURL></service>
<service><serviceType>urn:schemas-upnp-org:service:AVTransport:1</serviceType><controlURL>/upnp/control/avt</controlURL></service>
<service><serviceType>urn:schemas-upnp-org:service:RenderingControl:1</serviceType><controlURL>/upnp/control/rc</controlURL></service>
</serviceList></device></root>"#;
        let description =
            crate::device::parse_renderer("http://10.0.0.5:49494/description.xml", text).unwrap();
        assert_eq!(description.device.name, "Pi Renderer");
        assert_eq!(
            description.av_transport.control,
            "http://10.0.0.5:49494/upnp/control/avt"
        );
        assert_eq!(
            description.rendering_control.unwrap().control,
            "http://10.0.0.5:49494/upnp/control/rc"
        );
        assert!(description.connection_manager.is_some());
        assert!(crate::device::parse("http://10.0.0.5:49494/description.xml", text).is_err());
    }
}
