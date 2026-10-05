use std::io;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4, TcpListener, UdpSocket};
use std::ops::RangeInclusive;

use network_interface::{Addr, NetworkInterface, NetworkInterfaceConfig};
use socket2::{Domain, Protocol, Socket, Type};

const MULTICAST_TTL: u32 = 255;

pub(crate) const PORTS: RangeInclusive<u16> = 39_831..=39_840;

pub(crate) fn listen(what: &str) -> io::Result<TcpListener> {
    listen_in(PORTS, what)
}

fn listen_in(ports: RangeInclusive<u16>, what: &str) -> io::Result<TcpListener> {
    let listener = ports
        .clone()
        .chain([0])
        .find_map(listen_both)
        .or_else(|| {
            log::warn!("cast: {what} takes IPv4 only");
            ports.clone().chain([0]).find_map(listen_v4)
        })
        .ok_or_else(|| io::Error::other(format!("{what} has no port to listen on")))?;
    let port = listener.local_addr()?.port();
    if !ports.contains(&port) {
        log::warn!(
            "cast: TCP ports {}-{} are taken, {what} listens on {port}",
            ports.start(),
            ports.end()
        );
    }
    Ok(listener)
}

fn listen_both(port: u16) -> Option<TcpListener> {
    let socket = Socket::new(Domain::IPV6, Type::STREAM, Some(Protocol::TCP)).ok()?;
    socket.set_only_v6(false).ok()?;
    #[cfg(not(windows))]
    socket.set_reuse_address(true).ok()?;
    socket
        .bind(&SocketAddr::from((Ipv6Addr::UNSPECIFIED, port)).into())
        .ok()?;
    socket.listen(16).ok()?;
    Some(socket.into())
}

fn listen_v4(port: u16) -> Option<TcpListener> {
    TcpListener::bind((Ipv4Addr::UNSPECIFIED, port)).ok()
}

pub fn local_ip_for(peer: IpAddr) -> io::Result<IpAddr> {
    let unspecified = match peer {
        IpAddr::V4(_) => IpAddr::V4(Ipv4Addr::UNSPECIFIED),
        IpAddr::V6(_) => IpAddr::V6(Ipv6Addr::UNSPECIFIED),
    };
    let socket = UdpSocket::bind(SocketAddr::new(unspecified, 0))?;
    socket.connect(SocketAddr::new(peer, 9))?;
    let local = socket.local_addr()?.ip();
    if local.is_unspecified() {
        return Err(io::Error::other(format!("no route to {peer}")));
    }
    Ok(local)
}

pub fn ipv4_interfaces() -> Vec<Ipv4Addr> {
    let mut addresses: Vec<Ipv4Addr> = NetworkInterface::show()
        .unwrap_or_else(|e| {
            log::warn!("cast: listing network interfaces failed: {e}");
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

pub fn multicast_socket(interface: Ipv4Addr) -> io::Result<UdpSocket> {
    let socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
    socket.set_multicast_if_v4(&interface)?;
    socket.set_multicast_ttl_v4(MULTICAST_TTL)?;
    socket.bind(&SocketAddrV4::new(interface, 0).into())?;
    socket.set_nonblocking(true)?;
    Ok(socket.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpStream;

    fn free_tcp_ports(count: u16) -> RangeInclusive<u16> {
        loop {
            let first = TcpListener::bind("127.0.0.1:0")
                .unwrap()
                .local_addr()
                .unwrap()
                .port();
            let Some(last) = first.checked_add(count - 1) else {
                continue;
            };
            if (first..=last).all(|port| listen_both(port).is_some()) {
                return first..=last;
            }
        }
    }

    #[test]
    fn listeners_take_free_ports_of_the_range() {
        let ports = free_tcp_ports(3);
        let first = listen_in(ports.clone(), "test").unwrap();
        let second = listen_in(ports.clone(), "test").unwrap();
        let (first, second) = (
            first.local_addr().unwrap().port(),
            second.local_addr().unwrap().port(),
        );
        assert!(ports.contains(&first), "{first} in {ports:?}");
        assert!(ports.contains(&second), "{second} in {ports:?}");
        assert_ne!(first, second);
    }

    #[test]
    fn a_listener_takes_ipv4_and_ipv6_connections() {
        let listener = listen_both(0).unwrap();
        let port = listener.local_addr().unwrap().port();
        let mut addresses = vec![IpAddr::from(Ipv4Addr::LOCALHOST)];
        if TcpListener::bind((Ipv6Addr::LOCALHOST, 0)).is_ok() {
            addresses.push(IpAddr::from(Ipv6Addr::LOCALHOST));
        }
        for address in addresses {
            TcpStream::connect((address, port)).unwrap();
            listener.accept().unwrap();
        }
    }

    #[test]
    fn loopback_is_reached_from_loopback() {
        assert_eq!(
            local_ip_for(IpAddr::V4(Ipv4Addr::LOCALHOST)).unwrap(),
            IpAddr::V4(Ipv4Addr::LOCALHOST)
        );
    }
}
