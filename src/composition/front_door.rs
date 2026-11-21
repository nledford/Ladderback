//! The hub's front door (decision 120, "Binding to the TLS session"): one
//! listener for the web UI, the API and `/sync/v1`, where a connection that
//! opens with a TLS handshake gets the hub's built-in TLS 1.3, and one that
//! opens in plain HTTP is served only from loopback, as a local proxy's or
//! a browser's on the hub's own machine is.

use std::net::SocketAddr;

/// What the front door does with a connection, by the first byte it sent
/// and the address it came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Door {
    /// It opened a TLS handshake: the built-in TLS takes it.
    Tls,
    /// Plain HTTP from loopback: served as it is.
    Plain,
    /// Plain HTTP from beyond loopback: closed unserved, since nothing it
    /// carries would be encrypted.
    Closed,
}

/// The door for a connection from `peer` whose first byte is `first`.
pub(crate) fn door(first: u8, peer: SocketAddr) -> Door {
    let _ = (first, peer);
    todo!()
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

    use super::{Door, door};

    /// The first byte of a TLS handshake record.
    const HANDSHAKE: u8 = 0x16;

    fn from(ip: IpAddr) -> SocketAddr {
        SocketAddr::new(ip, 50_000)
    }

    #[test]
    fn a_tls_handshake_gets_the_built_in_tls_from_anywhere() {
        for ip in [
            IpAddr::V4(Ipv4Addr::LOCALHOST),
            IpAddr::V4(Ipv4Addr::new(192, 0, 2, 7)),
            IpAddr::V6(Ipv6Addr::new(0x2001, 0xdb8, 0, 0, 0, 0, 0, 7)),
        ] {
            assert_eq!(door(HANDSHAKE, from(ip)), Door::Tls, "{ip}");
        }
    }

    /// Plain HTTP is served from loopback, an IPv4 loopback address mapped
    /// into IPv6 among it, and closed from anywhere else.
    #[test]
    fn plain_http_is_served_from_loopback_only() {
        let mapped = Ipv4Addr::LOCALHOST.to_ipv6_mapped();
        for (ip, expected) in [
            (IpAddr::V4(Ipv4Addr::LOCALHOST), Door::Plain),
            (IpAddr::V4(Ipv4Addr::new(127, 0, 0, 2)), Door::Plain),
            (IpAddr::V6(Ipv6Addr::LOCALHOST), Door::Plain),
            (IpAddr::V6(mapped), Door::Plain),
            (IpAddr::V4(Ipv4Addr::new(192, 0, 2, 7)), Door::Closed),
            (
                IpAddr::V6(Ipv6Addr::new(0x2001, 0xdb8, 0, 0, 0, 0, 0, 7)),
                Door::Closed,
            ),
        ] {
            assert_eq!(door(b'G', from(ip)), expected, "{ip}");
        }
    }
}
