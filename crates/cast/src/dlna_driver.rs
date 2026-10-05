use std::net::IpAddr;
use std::time::Duration;

use audio_decoder::Codec;
use dlna::{Renderer, TrackMetadata, TransportState};

use crate::media::{Accepts, Probe, dlna_mimes};
use crate::pcm::Container;
use crate::session::{Driver, Loading, State, Status};

const VOLUME_EVERY: u32 = 5;

pub(crate) struct DlnaDriver {
    renderer: Renderer,
    peer: IpAddr,
    sinks: Vec<String>,
    polls: u32,
}

fn mime_of(protocol_info: &str) -> Option<String> {
    let mime = protocol_info.split(':').nth(2)?.trim();
    Some(
        mime.split(';')
            .next()
            .unwrap_or(mime)
            .trim()
            .to_ascii_lowercase(),
    )
}

impl DlnaDriver {
    pub fn connect(location: &str) -> Result<Self, String> {
        let renderer = Renderer::connect(location).map_err(|e| e.to_string())?;
        let peer = url_host(location).ok_or_else(|| format!("{location} has no host"))?;
        let sinks = match renderer.sink_protocols() {
            Ok(sinks) => sinks,
            Err(e) => {
                log::debug!("DLNA: GetProtocolInfo failed, assuming common formats: {e}");
                Vec::new()
            }
        };
        log::info!(
            "DLNA: {} accepts {} formats",
            renderer.device().name,
            sinks.len()
        );
        Ok(Self {
            renderer,
            peer,
            sinks,
            polls: 0,
        })
    }

    fn sink_mimes(&self) -> impl Iterator<Item = String> + '_ {
        self.sinks.iter().filter_map(|sink| {
            let protocol = sink.split(':').next().unwrap_or_default();
            (protocol.eq_ignore_ascii_case("http-get") || protocol == "*")
                .then(|| mime_of(sink))
                .flatten()
        })
    }

    fn takes(&self, mime: &str) -> bool {
        if self.sinks.is_empty() {
            return matches!(mime, "audio/mpeg" | "audio/flac" | "audio/wav");
        }
        self.sink_mimes()
            .any(|sink| sink == "*" || sink == mime || sink == "audio/*")
    }
}

fn url_host(location: &str) -> Option<IpAddr> {
    let rest = location.split("://").nth(1)?;
    let authority = rest.split('/').next()?;
    let host = match authority.strip_prefix('[') {
        Some(v6) => v6.split(']').next()?,
        None => authority
            .rsplit_once(':')
            .map_or(authority, |(host, _)| host),
    };
    if let Ok(ip) = host.parse() {
        return Some(ip);
    }
    use std::net::ToSocketAddrs;
    (host, 80)
        .to_socket_addrs()
        .ok()?
        .map(|address| address.ip())
        .find(IpAddr::is_ipv4)
}

impl Accepts for DlnaDriver {
    fn original(&self, codec: Codec, extension: &str, _probe: &Probe) -> Option<String> {
        dlna_mimes(codec, extension)
            .iter()
            .find(|mime| self.takes(mime))
            .map(|mime| mime.to_string())
    }

    fn pcm(&self) -> Container {
        let wav = ["audio/wav", "audio/x-wav", "audio/wave"]
            .iter()
            .any(|mime| self.takes(mime));
        if wav || !self.takes("audio/l16") {
            Container::Wav
        } else {
            Container::L16
        }
    }

    fn takes_pcm(&self) -> bool {
        ["audio/wav", "audio/x-wav", "audio/wave", "audio/l16"]
            .iter()
            .any(|mime| self.takes(mime))
    }
    fn pcm_limits(&self) -> (u32, u16) {
        (192_000, 8)
    }
}

fn protocol_info(mime: &str, features: &str) -> String {
    format!("http-get:*:{mime}:{features}")
}

impl Driver for DlnaDriver {
    fn peer(&self) -> IpAddr {
        self.peer
    }

    fn load(&mut self, loading: &Loading) -> Result<(), String> {
        let metadata = dlna::track_didl(
            loading.url,
            &protocol_info(loading.mime, loading.features),
            &TrackMetadata {
                title: loading.info.title.clone(),
                artist: loading.info.artist.clone(),
                album: loading.info.album.clone(),
                cover_url: loading.cover_url.map(str::to_string),
                duration: loading.duration,
                size: loading.size,
            },
        );
        if let Err(first) = self.renderer.set_uri(loading.url, &metadata) {
            log::debug!("DLNA: SetAVTransportURI failed ({first}), stopping and retrying");
            let _ = self.renderer.stop();
            self.renderer
                .set_uri(loading.url, &metadata)
                .map_err(|e| e.to_string())?;
        }
        if loading.autoplay {
            self.renderer.play().map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    fn play(&mut self) -> Result<(), String> {
        self.renderer.play().map_err(|e| e.to_string())
    }

    fn pause(&mut self) -> Result<(), String> {
        self.renderer.pause().map_err(|e| e.to_string())
    }

    fn seek(&mut self, position: Duration) -> Result<(), String> {
        self.renderer.seek(position).map_err(|e| e.to_string())
    }

    fn stop(&mut self) -> Result<(), String> {
        self.renderer.stop().map_err(|e| e.to_string())
    }

    fn set_volume(&mut self, volume: f32) -> Result<(), String> {
        self.renderer
            .set_volume((volume.clamp(0.0, 1.0) * 100.0).round() as u8)
            .map_err(|e| e.to_string())
    }

    fn status(&mut self) -> Result<Status, String> {
        let transport = self.renderer.transport_state().map_err(|e| e.to_string())?;
        let position = self.renderer.position().unwrap_or_else(|e| {
            log::debug!("DLNA: GetPositionInfo failed: {e}");
            dlna::PositionInfo::default()
        });
        self.polls = self.polls.wrapping_add(1);
        let volume = if self.polls.is_multiple_of(VOLUME_EVERY) {
            self.volume()
        } else {
            None
        };
        let state = match transport {
            TransportState::Playing => Some(State::Playing),
            TransportState::Paused => Some(State::Paused),
            TransportState::Transitioning => Some(State::Buffering),
            TransportState::Stopped | TransportState::NoMedia => Some(State::Idle),
            TransportState::Other(_) => None,
        };
        Ok(Status {
            state,
            position: position.position,
            duration: position.duration,
            volume,
            error: None,
        })
    }

    fn lost(&mut self) -> Option<String> {
        None
    }

    fn volume(&mut self) -> Option<f32> {
        self.renderer
            .volume()
            .ok()
            .flatten()
            .map(|volume| f32::from(volume) / 100.0)
    }

    fn seeks_on_load(&self) -> bool {
        false
    }

    fn close(&mut self) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_mime_is_the_third_protocol_info_field() {
        assert_eq!(
            mime_of("http-get:*:audio/L16;rate=44100;channels=2:DLNA.ORG_PN=LPCM"),
            Some("audio/l16".into())
        );
        assert_eq!(
            mime_of("http-get:*:audio/flac:*"),
            Some("audio/flac".into())
        );
        assert_eq!(mime_of("garbage"), None);
    }

    #[test]
    fn the_renderer_host_comes_from_its_description_url() {
        assert_eq!(
            url_host("http://192.168.3.22:49494/description.xml"),
            Some("192.168.3.22".parse().unwrap())
        );
        assert_eq!(
            url_host("http://[fe80::1]:8080/d.xml"),
            Some("fe80::1".parse().unwrap())
        );
    }
}
