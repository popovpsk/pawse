use std::net::{Ipv4Addr, SocketAddrV4, UdpSocket};
use std::time::{Duration, Instant};

use network_interface::{Addr, NetworkInterface, NetworkInterfaceConfig};
use socket2::{Domain, Protocol, Socket, Type};

const MULTICAST: SocketAddrV4 = SocketAddrV4::new(Ipv4Addr::new(239, 255, 255, 250), 1900);
pub(crate) const TARGETS: [&str; 2] = [
    "urn:schemas-upnp-org:service:ContentDirectory:1",
    "urn:schemas-upnp-org:device:MediaServer:1",
];
pub(crate) const RENDERER_TARGETS: [&str; 2] = [
    "urn:schemas-upnp-org:service:AVTransport:1",
    "urn:schemas-upnp-org:device:MediaRenderer:1",
];
const POLL: Duration = Duration::from_millis(50);
const TTL: u32 = 4;
const SENDS: usize = 2;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Reply {
    pub location: String,
    pub udn: String,
}

pub(crate) fn search(targets: &[&str], timeout: Duration, wanted: Option<&str>) -> Vec<Reply> {
    let sockets = sockets();
    if sockets.is_empty() {
        log::warn!("SSDP: no IPv4 interface to search on");
        return Vec::new();
    }
    for socket in &sockets {
        for target in targets {
            for _ in 0..SENDS {
                if let Err(e) = socket.send_to(request(target).as_bytes(), MULTICAST) {
                    log::debug!("SSDP: M-SEARCH on {:?} failed: {e}", socket.local_addr());
                }
            }
        }
    }
    let deadline = Instant::now() + timeout;
    let mut replies: Vec<Reply> = Vec::new();
    let mut buffer = [0u8; 4096];
    while Instant::now() < deadline {
        let mut received = false;
        for socket in &sockets {
            while let Ok((len, _)) = socket.recv_from(&mut buffer) {
                received = true;
                let Some(reply) = parse_reply(&String::from_utf8_lossy(&buffer[..len])) else {
                    continue;
                };
                let done = wanted.is_some_and(|udn| reply.udn.eq_ignore_ascii_case(udn));
                if !replies.contains(&reply) {
                    replies.push(reply);
                }
                if done {
                    return replies;
                }
            }
        }
        if !received {
            std::thread::sleep(POLL);
        }
    }
    replies
}

fn request(target: &str) -> String {
    format!(
        "M-SEARCH * HTTP/1.1\r\nHOST: 239.255.255.250:1900\r\nMAN: \"ssdp:discover\"\r\nMX: 2\r\nST: {target}\r\nUSER-AGENT: pawse UPnP/1.1\r\n\r\n"
    )
}

fn sockets() -> Vec<UdpSocket> {
    let mut addresses: Vec<Ipv4Addr> = NetworkInterface::show()
        .unwrap_or_else(|e| {
            log::warn!("SSDP: listing network interfaces failed: {e}");
            Vec::new()
        })
        .into_iter()
        .filter(|interface| !interface.internal)
        .flat_map(|interface| interface.addr)
        .filter_map(|addr| match addr {
            Addr::V4(v4) if !v4.ip.is_loopback() && !v4.ip.is_unspecified() => Some(v4.ip),
            _ => None,
        })
        .collect();
    addresses.sort();
    addresses.dedup();
    if addresses.is_empty() {
        addresses.push(Ipv4Addr::UNSPECIFIED);
    }
    addresses
        .into_iter()
        .filter_map(|address| {
            open(address)
                .inspect_err(|e| log::debug!("SSDP: no socket on {address}: {e}"))
                .ok()
        })
        .collect()
}

fn open(interface: Ipv4Addr) -> std::io::Result<UdpSocket> {
    let socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
    if !interface.is_unspecified() {
        socket.set_multicast_if_v4(&interface)?;
    }
    socket.set_multicast_ttl_v4(TTL)?;
    socket.bind(&SocketAddrV4::new(interface, 0).into())?;
    socket.set_nonblocking(true)?;
    Ok(socket.into())
}

pub(crate) fn parse_reply(text: &str) -> Option<Reply> {
    let mut lines = text.lines();
    let status = lines.next()?;
    if !status.starts_with("HTTP/") || status.split_whitespace().nth(1) != Some("200") {
        return None;
    }
    let mut location = None;
    let mut usn = None;
    for line in lines {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        let name = name.trim();
        if name.eq_ignore_ascii_case("location") {
            location = Some(value.trim());
        } else if name.eq_ignore_ascii_case("usn") {
            usn = Some(value.trim());
        }
    }
    let location = location.filter(|location| location.contains("://"))?;
    let udn = usn?.split("::").next()?.trim();
    if !udn
        .get(..5)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("uuid:"))
    {
        return None;
    }
    Some(Reply {
        location: location.to_string(),
        udn: udn.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_search_reply_gives_the_location_and_the_device_udn() {
        let reply = "HTTP/1.1 200 OK\r\nCACHE-CONTROL: max-age=1810\r\nEXT:\r\n\
location: http://192.168.1.5:8200/rootDesc.xml\r\nSERVER: Debian/12 DLNADOC/1.50 UPnP/1.0 MiniDLNA/1.3.3\r\n\
ST: urn:schemas-upnp-org:service:ContentDirectory:1\r\n\
USN: uuid:4d696e69-444c-164e-9d41-000000000001::urn:schemas-upnp-org:service:ContentDirectory:1\r\n\r\n";
        assert_eq!(
            parse_reply(reply),
            Some(Reply {
                location: "http://192.168.1.5:8200/rootDesc.xml".into(),
                udn: "uuid:4d696e69-444c-164e-9d41-000000000001".into(),
            })
        );
    }

    #[test]
    fn header_spelling_and_line_endings_vary_between_servers() {
        let reply = parse_reply(
            "HTTP/1.1 200 OK\nlocation:http://10.0.0.2:49152/description.xml\nusn:UUID:ABC::urn:schemas-upnp-org:device:MediaServer:1\n\n",
        )
        .unwrap();
        assert_eq!(reply.location, "http://10.0.0.2:49152/description.xml");
        assert_eq!(reply.udn, "UUID:ABC");
        assert_eq!(
            parse_reply("HTTP/1.1 200 OK\r\nLOCATION: http://a/d.xml\r\nUSN: uuid:only\r\n\r\n")
                .map(|reply| reply.udn),
            Some("uuid:only".into())
        );
        assert_eq!(
            parse_reply("HTTP/1.1 404 Not Found\r\nLOCATION: http://a/\r\nUSN: uuid:x\r\n"),
            None
        );
        assert_eq!(parse_reply(""), None);
    }

    #[test]
    fn notifications_and_replies_without_a_udn_are_ignored() {
        assert_eq!(
            parse_reply("NOTIFY * HTTP/1.1\r\nLOCATION: http://a/\r\nUSN: uuid:x\r\n\r\n"),
            None
        );
        assert_eq!(
            parse_reply("HTTP/1.1 200 OK\r\nLOCATION: http://a/\r\nUSN: upnp:rootdevice\r\n\r\n"),
            None
        );
        assert_eq!(
            parse_reply("HTTP/1.1 200 OK\r\nUSN: uuid:x::upnp:rootdevice\r\n\r\n"),
            None
        );
    }

    #[test]
    fn the_search_request_asks_for_the_target() {
        let request = request(TARGETS[0]);
        assert!(request.starts_with("M-SEARCH * HTTP/1.1\r\n"));
        assert!(request.contains("\r\nST: urn:schemas-upnp-org:service:ContentDirectory:1\r\n"));
        assert!(request.ends_with("\r\n\r\n"));
    }
}
