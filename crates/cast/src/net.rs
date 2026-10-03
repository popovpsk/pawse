use std::io;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, UdpSocket};

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
