use std::io;
use std::net::{IpAddr, Ipv4Addr, SocketAddrV4, UdpSocket};
use std::time::{Duration, Instant};

use network_interface::{Addr, NetworkInterface, NetworkInterfaceConfig};
use socket2::{Domain, Protocol, Socket, Type};

const PORT: u16 = 1900;
const GROUP: Ipv4Addr = Ipv4Addr::new(239, 255, 255, 250);
const MULTICAST: SocketAddrV4 = SocketAddrV4::new(GROUP, PORT);
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notification {
    pub alive: bool,
    pub udn: String,
    pub kind: String,
    pub location: Option<String>,
    pub source: IpAddr,
}

pub struct NotifyListener {
    socket: UdpSocket,
}

impl NotifyListener {
    pub fn open() -> io::Result<Self> {
        let socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
        socket.set_reuse_address(true)?;
        #[cfg(unix)]
        socket.set_reuse_port(true)?;
        socket.bind(&SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, PORT).into())?;
        let mut interfaces = interfaces();
        if interfaces.is_empty() {
            interfaces.push(Ipv4Addr::UNSPECIFIED);
        }
        let mut joined = 0;
        for interface in interfaces {
            match socket.join_multicast_v4(&GROUP, &interface) {
                Ok(()) => joined += 1,
                Err(e) => log::debug!("SSDP: listening on {interface} failed: {e}"),
            }
        }
        if joined == 0 {
            return Err(io::Error::other("no interface joined the SSDP group"));
        }
        Ok(Self {
            socket: socket.into(),
        })
    }

    pub fn next(&self, timeout: Duration) -> Option<Notification> {
        let deadline = Instant::now() + timeout;
        let mut buffer = [0u8; 4096];
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() || self.socket.set_read_timeout(Some(left)).is_err() {
                return None;
            }
            let (len, from) = self.socket.recv_from(&mut buffer).ok()?;
            if let Some(notification) =
                parse_notify(&String::from_utf8_lossy(&buffer[..len]), from.ip())
            {
                return Some(notification);
            }
        }
    }
}

pub(crate) fn search(
    targets: &[&str],
    hosts: &[Ipv4Addr],
    timeout: Duration,
    wanted: Option<&str>,
) -> Vec<Reply> {
    let mut sockets = sockets();
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
    if !hosts.is_empty() {
        match open(Ipv4Addr::UNSPECIFIED) {
            Ok(unicast) => {
                for host in hosts {
                    for target in targets {
                        if let Err(e) = unicast
                            .send_to(request(target).as_bytes(), SocketAddrV4::new(*host, PORT))
                        {
                            log::debug!("SSDP: M-SEARCH to {host} failed: {e}");
                        }
                    }
                }
                sockets.push(unicast);
            }
            Err(e) => log::debug!("SSDP: no socket for searching known hosts: {e}"),
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

fn interfaces() -> Vec<Ipv4Addr> {
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
    addresses
}

fn sockets() -> Vec<UdpSocket> {
    let mut addresses = interfaces();
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

fn header<'a>(text: &'a str, wanted: &str) -> Option<&'a str> {
    text.lines().skip(1).find_map(|line| {
        let (name, value) = line.split_once(':')?;
        name.trim()
            .eq_ignore_ascii_case(wanted)
            .then_some(value.trim())
    })
}

fn location_of(text: &str) -> Option<&str> {
    header(text, "location").filter(|location| location.contains("://"))
}

fn udn_of(text: &str) -> Option<&str> {
    let udn = header(text, "usn")?.split("::").next()?.trim();
    udn.get(..5)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("uuid:"))
        .then_some(udn)
}

pub(crate) fn parse_reply(text: &str) -> Option<Reply> {
    let status = text.lines().next()?;
    if !status.starts_with("HTTP/") || status.split_whitespace().nth(1) != Some("200") {
        return None;
    }
    Some(Reply {
        location: location_of(text)?.to_string(),
        udn: udn_of(text)?.to_string(),
    })
}

pub(crate) fn parse_notify(text: &str, source: IpAddr) -> Option<Notification> {
    let start = text.lines().next()?;
    if !start
        .get(..7)
        .is_some_and(|method| method.eq_ignore_ascii_case("NOTIFY "))
    {
        return None;
    }
    let alive = match header(text, "nts")?.to_ascii_lowercase().as_str() {
        "ssdp:alive" | "ssdp:update" => true,
        "ssdp:byebye" => false,
        _ => return None,
    };
    let location = location_of(text).map(str::to_string);
    if alive && location.is_none() {
        return None;
    }
    Some(Notification {
        alive,
        udn: udn_of(text)?.to_string(),
        kind: header(text, "nt").unwrap_or_default().to_string(),
        location,
        source,
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

    fn source() -> IpAddr {
        "192.168.3.22".parse().unwrap()
    }

    #[test]
    fn an_alive_notification_names_the_device_and_where_it_is() {
        let text = "NOTIFY * HTTP/1.1\r\nHOST: 239.255.255.250:1900\r\nCACHE-CONTROL: max-age=1800\r\n\
LOCATION: http://192.168.3.22:49494/description.xml\r\nNT: urn:schemas-upnp-org:device:MediaRenderer:1\r\n\
NTS: ssdp:alive\r\nSERVER: Linux, UPnP/1.0, Portable SDK for UPnP devices/17.2.0\r\n\
USN: uuid:5ab1d1bb-ed20-6c05-a9c5-cac2d1b3f8a1::urn:schemas-upnp-org:device:MediaRenderer:1\r\n\r\n";
        assert_eq!(
            parse_notify(text, source()),
            Some(Notification {
                alive: true,
                udn: "uuid:5ab1d1bb-ed20-6c05-a9c5-cac2d1b3f8a1".into(),
                kind: "urn:schemas-upnp-org:device:MediaRenderer:1".into(),
                location: Some("http://192.168.3.22:49494/description.xml".into()),
                source: source(),
            })
        );
    }

    #[test]
    fn a_byebye_needs_no_location_but_an_alive_does() {
        let byebye = parse_notify(
            "notify * HTTP/1.1\nNT: upnp:rootdevice\nNTS: ssdp:byebye\nUSN: uuid:abc::upnp:rootdevice\n\n",
            source(),
        )
        .unwrap();
        assert!(!byebye.alive);
        assert_eq!(byebye.udn, "uuid:abc");
        assert_eq!(byebye.location, None);
        assert_eq!(
            parse_notify(
                "NOTIFY * HTTP/1.1\r\nNTS: ssdp:alive\r\nUSN: uuid:abc\r\n\r\n",
                source()
            ),
            None
        );
    }

    #[test]
    fn searches_replies_and_odd_notifications_are_not_notifications() {
        assert_eq!(
            parse_notify(
                "M-SEARCH * HTTP/1.1\r\nST: ssdp:all\r\nMAN: \"ssdp:discover\"\r\n\r\n",
                source()
            ),
            None
        );
        assert_eq!(
            parse_notify(
                "HTTP/1.1 200 OK\r\nLOCATION: http://a/d.xml\r\nUSN: uuid:x\r\n\r\n",
                source()
            ),
            None
        );
        assert_eq!(
            parse_notify(
                "NOTIFY * HTTP/1.1\r\nNTS: upnp:propchange\r\nLOCATION: http://a/d.xml\r\nUSN: uuid:x\r\n\r\n",
                source()
            ),
            None
        );
        assert_eq!(
            parse_notify(
                "NOTIFY * HTTP/1.1\r\nNTS: ssdp:alive\r\nLOCATION: http://a/d.xml\r\nUSN: upnp:rootdevice\r\n\r\n",
                source()
            ),
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
