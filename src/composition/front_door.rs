//! The hub's front door (decision 120, "Binding to the TLS session"): one
//! listener for the web UI, the API and `/sync/v1`, where a connection that
//! opens with a TLS handshake gets the hub's built-in TLS 1.3, and one that
//! opens in plain HTTP is served only from loopback, as a local proxy's or
//! a browser's on the hub's own machine is. A hub whose TLS key can't serve
//! keeps the door without the TLS ("The hub key"): it serves its own
//! machine on loopback and closes every other connection.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use domain::fleet::Binding;
use http_adapter::{BuiltInTls, GatedListener, Handed, HandedListener, HandedPeer};
use tls_adapter::{HubListener, SessionBinding as _};
use tokio::net::TcpStream;
use tokio::sync::mpsc;

/// The first byte of a TLS record that carries a handshake, as a
/// `ClientHello` opens a connection with.
const HANDSHAKE: u8 = 0x16;

/// How long a connection has to send its first byte, and then to finish
/// the TLS handshake, before it is closed.
const FIRST_BYTE_WITHIN: Duration = Duration::from_secs(10);
const HANDSHAKE_WITHIN: Duration = Duration::from_secs(10);

/// How many connections let in may wait for the server to take them.
const WAITING: usize = 64;

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
    if first == HANDSHAKE {
        Door::Tls
    } else if peer.ip().to_canonical().is_loopback() {
        Door::Plain
    } else {
        Door::Closed
    }
}

/// The front door on `listener`, bound to `local`, serving `tls` when the
/// hub's key serves one: each connection is taken through its door on a
/// task of its own, so a slow one holds up no other, and handed to the
/// server once it is let in.
pub(crate) fn open(
    listener: GatedListener,
    local: SocketAddr,
    tls: Option<HubListener>,
) -> HandedListener {
    let (hand, connections) = mpsc::channel(WAITING);
    let tls = tls.map(Arc::new);
    let accepting = tokio::spawn(async move {
        let mut listener = listener;
        loop {
            let (stream, peer) = axum::serve::Listener::accept(&mut listener).await;
            let hand = hand.clone();
            let tls = tls.clone();
            tokio::spawn(async move {
                if let Some(handed) = let_in(stream, peer, tls.as_deref()).await {
                    // The server is gone when this fails, and the
                    // connection closes with it.
                    let _ = hand.send(handed).await;
                }
            });
        }
    });
    HandedListener::new(connections, local, accepting)
}

/// The connection from `peer`, let in through its door, or `None` once it
/// is closed: it sent nothing in time, plain HTTP from beyond loopback, or
/// a TLS handshake that failed, took too long or found no TLS served.
async fn let_in(stream: TcpStream, peer: SocketAddr, tls: Option<&HubListener>) -> Option<Handed> {
    let mut first = [0; 1];
    let peeked = tokio::time::timeout(FIRST_BYTE_WITHIN, stream.peek(&mut first)).await;
    if !matches!(peeked, Ok(Ok(1))) {
        return None;
    }
    match door(first[0], peer) {
        Door::Closed => {
            log::debug!("closed a plain HTTP connection from {peer}, beyond loopback");
            None
        }
        Door::Plain => Some(Handed {
            io: Box::new(stream),
            peer: HandedPeer {
                addr: peer,
                tls: None,
            },
        }),
        Door::Tls => {
            let Some(tls) = tls else {
                log::debug!("closed a TLS handshake from {peer}: the hub's TLS key doesn't serve");
                return None;
            };
            let session = match tokio::time::timeout(HANDSHAKE_WITHIN, tls.accept(stream)).await {
                Ok(Ok(session)) => session,
                Ok(Err(error)) => {
                    log::debug!("a TLS handshake from {peer} failed: {error}");
                    return None;
                }
                Err(_) => return None,
            };
            // The binding only a hybrid session carries: a kind-1
            // `Connect` on any other is closed with `binding-unavailable`
            // (decision 120, "Post-quantum key exchange").
            let exporter = match session.binding() {
                Ok(Binding::TlsExporter(value)) if session.is_post_quantum() => Some(value),
                _ => None,
            };
            Some(Handed {
                io: Box::new(session),
                peer: HandedPeer {
                    addr: peer,
                    tls: Some(BuiltInTls { exporter }),
                },
            })
        }
    }
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
