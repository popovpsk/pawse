use std::io;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4, UdpSocket};

use network_interface::{Addr, NetworkInterface, NetworkInterfaceConfig};
use socket2::{Domain, Protocol, Socket, Type};

const MULTICAST_TTL: u32 = 255;

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

    #[test]
    fn loopback_is_reached_from_loopback() {
        assert_eq!(
            local_ip_for(IpAddr::V4(Ipv4Addr::LOCALHOST)).unwrap(),
            IpAddr::V4(Ipv4Addr::LOCALHOST)
        );
    }
}
