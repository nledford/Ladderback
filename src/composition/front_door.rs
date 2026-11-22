//! The hub's front door (decision 120, "Binding to the TLS session"): one
//! listener for the web UI, the API and `/sync/v1`, where a connection that
//! opens with a TLS handshake gets the hub's built-in TLS 1.3, and one that
//! opens in plain HTTP is served only from loopback, as a local proxy's or
//! a browser's on the hub's own machine is. A hub whose TLS key can't serve
//! keeps the door without the TLS ("The hub key"): it serves its own
//! machine on loopback and closes every other connection.

use std::collections::VecDeque;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use domain::access::Client;
use domain::fleet::Binding;
use http_adapter::{BuiltInTls, GatedListener, Handed, HandedListener, HandedPeer};
use tls_adapter::{HubListener, SessionBinding as _};
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio::task::AbortHandle;

/// The first byte of a TLS record that carries a handshake, as a
/// `ClientHello` opens a connection with.
const HANDSHAKE: u8 = 0x16;

/// How long a connection has to send its first byte, and then to finish
/// the TLS handshake, before it is closed.
const FIRST_BYTE_WITHIN: Duration = Duration::from_secs(10);
const HANDSHAKE_WITHIN: Duration = Duration::from_secs(10);

/// How many connections let in may wait, handed on, for the server to take
/// them. One: a connection let in while the server has no place for it
/// waits at the door instead, where it counts in the door's bound and its
/// address's share, rather than in the hand-off, where nothing would.
const WAITING: usize = 1;

/// How many connections may be at the door at once: waiting to send their
/// first byte, in their TLS handshake, or waiting to be handed to the
/// server. A newcomer past it closes the oldest of the address that holds
/// the most there, so a flood of connections that send nothing can't hold
/// the door shut, nor push out a lone connection from elsewhere (decision
/// 120, "At most 64 connections at the door").
const AT_THE_DOOR: usize = 64;

/// How many connections at the door one address beyond loopback may hold:
/// an IPv4 address, or the /64 network of an IPv6 address, which one host
/// can fill with addresses of its own. A newcomer past it closes that
/// address's oldest, as the `/sync/v1` table does, so one address can't
/// take the door's whole bound (decision 120, "8 per address at the door").
const PER_ADDRESS: usize = 8;

/// A connection at the door, as the door holds it.
pub(crate) trait AtDoor {
    /// Closes it.
    fn close(&self);
    /// Whether it has left the door: closed, or handed to the server.
    fn is_gone(&self) -> bool;
}

/// The connections at the door, oldest first, by the address each came
/// from. Dropped, as when the door stops, it closes every one still there.
pub(crate) struct AtTheDoor<C: AtDoor> {
    open: VecDeque<AtTheDoorFrom<C>>,
}

struct AtTheDoorFrom<C> {
    addr: IpAddr,
    /// Whose share it counts in: `None` for loopback, which has none.
    client: Option<Client>,
    connection: C,
}

impl<C: AtDoor> AtTheDoor<C> {
    pub(crate) fn new() -> Self {
        Self {
            open: VecDeque::with_capacity(AT_THE_DOOR),
        }
    }

    /// Takes in `connection` from `from`, closing first the oldest of its
    /// address when that holds [`PER_ADDRESS`] and isn't loopback, then,
    /// when the door holds [`AT_THE_DOOR`], the oldest of the address that
    /// holds the most, loopback counted as one.
    pub(crate) fn admit(&mut self, from: IpAddr, connection: C) {
        self.open.retain(|at| !at.connection.is_gone());
        // A mapped IPv4 address is that address, loopback's included.
        let addr = from.to_canonical();
        let client = (!addr.is_loopback()).then(|| Client::of(addr));
        // Each admission keeps the share, so it is at most full here.
        if client.is_some()
            && self.held_by(client) >= PER_ADDRESS
            && let Some(closed) = self.close_oldest_of(client)
        {
            log::debug!(
                "closed the oldest connection at the hub's door from {closed}: one address may hold {PER_ADDRESS} there"
            );
        }
        if self.open.len() >= AT_THE_DOOR {
            let most = self
                .open
                .iter()
                .map(|at| self.held_by(at.client))
                .max()
                .unwrap_or(0);
            let busiest = self
                .open
                .iter()
                .find(|at| self.held_by(at.client) == most)
                .map(|at| at.client);
            if let Some(busiest) = busiest
                && let Some(closed) = self.close_oldest_of(busiest)
            {
                log::debug!(
                    "closed the oldest connection at the hub's door from {closed}: the door holds {AT_THE_DOOR}, and its address the most"
                );
            }
        }
        self.open.push_back(AtTheDoorFrom {
            addr,
            client,
            connection,
        });
    }

    fn held_by(&self, client: Option<Client>) -> usize {
        self.open.iter().filter(|at| at.client == client).count()
    }

    /// Closes the oldest connection of `client`, and says where it came
    /// from.
    fn close_oldest_of(&mut self, client: Option<Client>) -> Option<IpAddr> {
        let oldest = self.open.iter().position(|at| at.client == client)?;
        let oldest = self.open.remove(oldest)?;
        oldest.connection.close();
        Some(oldest.addr)
    }
}

impl<C: AtDoor> Drop for AtTheDoor<C> {
    fn drop(&mut self) {
        for at in &self.open {
            at.connection.close();
        }
    }
}

impl AtDoor for AbortHandle {
    fn close(&self) {
        self.abort();
    }

    fn is_gone(&self) -> bool {
        self.is_finished()
    }
}

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
/// server once it is let in. At most [`AT_THE_DOOR`] are at the door at
/// once, and [`PER_ADDRESS`] from one address beyond loopback, as
/// [`AtTheDoor`] keeps them. Once the server drops its end, as at
/// shutdown, the door closes its listener and every connection still at
/// it.
pub(crate) fn open(
    listener: GatedListener,
    local: SocketAddr,
    tls: Option<HubListener>,
) -> HandedListener {
    let (hand, connections) = mpsc::channel(WAITING);
    let tls = tls.map(Arc::new);
    let said = Arc::new(AtomicBool::new(false));
    let accepting = tokio::spawn(async move {
        let mut listener = listener;
        let mut at_the_door = AtTheDoor::new();
        loop {
            let (stream, peer) = axum::serve::Listener::accept(&mut listener).await;
            let hand = hand.clone();
            let tls = tls.clone();
            let said = Arc::clone(&said);
            let task = tokio::spawn(async move {
                if let Some(handed) = let_in(stream, peer, tls.as_deref(), &said).await {
                    // The server is gone when this fails, and the
                    // connection closes with it.
                    let _ = hand.send(handed).await;
                }
            });
            at_the_door.admit(peer.ip(), task.abort_handle());
        }
    });
    HandedListener::new(connections, local, accepting)
}

/// The connection from `peer`, let in through its door, or `None` once it
/// is closed: it sent nothing in time, plain HTTP from beyond loopback, or
/// a TLS handshake that failed, took too long or found no TLS served. The
/// first plain close is logged as a warning, since a proxy that forwards to
/// `http://` from another machine or a container's gateway meets it, and
/// `said` keeps the rest to debug.
async fn let_in(
    stream: TcpStream,
    peer: SocketAddr,
    tls: Option<&HubListener>,
    said: &AtomicBool,
) -> Option<Handed> {
    let mut first = [0; 1];
    let peeked = tokio::time::timeout(FIRST_BYTE_WITHIN, stream.peek(&mut first)).await;
    if !matches!(peeked, Ok(Ok(1))) {
        return None;
    }
    match door(first[0], peer) {
        Door::Closed => {
            if said.swap(true, Ordering::Relaxed) {
                log::debug!("closed a plain HTTP connection from {peer}, beyond loopback");
            } else {
                log::warn!(
                    "closed a plain HTTP connection from {peer}: the hub serves plain HTTP only to its own machine, over loopback. Reach it at its https:// address, or have a proxy forward to that; later closes are logged at debug level"
                );
            }
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
    use std::time::Duration;

    use http_adapter::{GatedListener, HandedListener};
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
    use tokio::net::TcpStream;
    use tokio::time::Instant;

    use super::{AT_THE_DOOR, AtDoor, AtTheDoor, Door, FIRST_BYTE_WITHIN, PER_ADDRESS, door, open};

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

    /// A connection at the door that has sent nothing yet.
    async fn silent(at: SocketAddr) -> TcpStream {
        TcpStream::connect(at).await.unwrap()
    }

    /// A plain request from loopback, let in and handed to the server.
    async fn handed(at: SocketAddr, listener: &mut HandedListener) {
        let mut stream = TcpStream::connect(at).await.unwrap();
        stream.write_all(b"GET / HTTP/1.1\r\n\r\n").await.unwrap();
        let (_, peer) = axum::serve::Listener::accept(listener).await;
        assert_eq!(peer.addr, stream.local_addr().unwrap());
    }

    /// Whether the door closed `stream`, connected at `connected`, well
    /// before its own time limit for a first byte would have.
    async fn closed_soon(stream: &mut TcpStream, connected: Instant) -> bool {
        let mut byte = [0; 1];
        let soon = connected + FIRST_BYTE_WITHIN / 2;
        let read = tokio::time::timeout_at(soon, stream.read(&mut byte)).await;
        matches!(read, Ok(Ok(0) | Err(_)))
    }

    /// Whether `stream` is still open: nothing is read from it for a while.
    /// A connection the door closed is read as closed within it.
    async fn still_open(stream: &mut TcpStream) -> bool {
        let mut byte = [0; 1];
        let read = tokio::time::timeout(Duration::from_millis(200), stream.read(&mut byte)).await;
        read.is_err()
    }

    /// A flood of connections that send nothing can't hold the door: one
    /// past `AT_THE_DOOR` closes the oldest still there and is let in, and
    /// one already handed on no longer counts.
    #[tokio::test]
    async fn a_connection_past_the_door_s_bound_closes_the_oldest_still_at_it() {
        let listener = GatedListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))
            .await
            .unwrap();
        let at = listener.local_addr().unwrap();
        let mut server = open(listener, at, None);
        let connected = Instant::now();
        let mut oldest = silent(at).await;
        handed(at, &mut server).await;
        let mut others = Vec::new();
        for _ in 0..AT_THE_DOOR - 2 {
            others.push(silent(at).await);
        }
        // With the one handed on gone, this is the last the door holds.
        handed(at, &mut server).await;
        assert!(still_open(&mut oldest).await, "closed within the bound");
        others.push(silent(at).await);
        handed(at, &mut server).await;
        assert!(
            closed_soon(&mut oldest, connected).await,
            "the oldest is still at the door"
        );
        assert!(
            still_open(&mut others[0]).await,
            "closed another than the oldest"
        );
    }

    /// Stopping the door, as the server does at shutdown, closes the
    /// connections still at it at once, not each at its own time limit.
    #[tokio::test]
    async fn stopping_the_door_closes_the_connections_still_at_it() {
        let listener = GatedListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))
            .await
            .unwrap();
        let at = listener.local_addr().unwrap();
        let mut server = open(listener, at, None);
        let connected = Instant::now();
        let mut waiting = silent(at).await;
        // Accepted after it, so it is at the door by now.
        handed(at, &mut server).await;
        drop(server);
        assert!(
            closed_soon(&mut waiting, connected).await,
            "still at the door"
        );
    }

    /// A connection at the door in a test: closed is all it records.
    #[derive(Clone, Default)]
    struct Probe(std::rc::Rc<std::cell::Cell<bool>>);

    impl AtDoor for Probe {
        fn close(&self) {
            self.0.set(true);
        }

        fn is_gone(&self) -> bool {
            self.0.get()
        }
    }

    /// `count` connections from `from` taken in at `door`.
    fn admitted(door: &mut AtTheDoor<Probe>, from: IpAddr, count: usize) -> Vec<Probe> {
        (0..count)
            .map(|_| {
                let probe = Probe::default();
                door.admit(from, probe.clone());
                probe
            })
            .collect()
    }

    fn closed(probes: &[Probe]) -> Vec<bool> {
        probes.iter().map(AtDoor::is_gone).collect()
    }

    /// An address beyond loopback past its share closes its own oldest, and
    /// no other address's.
    #[test]
    fn an_address_past_its_share_closes_its_own_oldest() {
        let mut at_the_door = AtTheDoor::new();
        let one = IpAddr::V4(Ipv4Addr::new(192, 0, 2, 7));
        let other = IpAddr::V6(Ipv6Addr::new(0x2001, 0xdb8, 0, 0, 0, 0, 0, 7));
        let other_s = admitted(&mut at_the_door, other, 1);
        let first = admitted(&mut at_the_door, one, PER_ADDRESS);
        assert!(!closed(&first).contains(&true), "closed within the share");
        let newcomer = admitted(&mut at_the_door, one, 1);
        let mut expected = vec![false; PER_ADDRESS];
        expected[0] = true;
        assert_eq!(closed(&first), expected);
        assert_eq!(closed(&other_s), [false]);
        assert_eq!(closed(&newcomer), [false]);
    }

    /// A connection that has left the door no longer counts in its
    /// address's share.
    #[test]
    fn a_connection_gone_from_the_door_leaves_its_share() {
        let mut at_the_door = AtTheDoor::new();
        let one = IpAddr::V4(Ipv4Addr::new(192, 0, 2, 7));
        let first = admitted(&mut at_the_door, one, PER_ADDRESS);
        first[3].close();
        admitted(&mut at_the_door, one, 1);
        let mut expected = vec![false; PER_ADDRESS];
        expected[3] = true;
        assert_eq!(closed(&first), expected);
    }

    /// Loopback, where a local proxy brings every client in from one
    /// address, has no share of its own: only the door's whole bound holds
    /// it, where it counts as one address.
    #[test]
    fn loopback_has_no_share_only_the_door_s_bound() {
        let mapped = IpAddr::V6(Ipv4Addr::LOCALHOST.to_ipv6_mapped());
        for from in [
            IpAddr::V4(Ipv4Addr::LOCALHOST),
            IpAddr::V6(Ipv6Addr::LOCALHOST),
            mapped,
        ] {
            let mut at_the_door = AtTheDoor::new();
            let beyond = admitted(&mut at_the_door, IpAddr::V4(Ipv4Addr::new(192, 0, 2, 7)), 1);
            let local = admitted(&mut at_the_door, from, AT_THE_DOOR - 1);
            assert!(
                !closed(&local).contains(&true),
                "{from}: closed within the bound"
            );
            admitted(&mut at_the_door, from, 1);
            assert_eq!(closed(&beyond), [false], "{from}: closed the lone one");
            let mut expected = vec![false; AT_THE_DOOR - 1];
            expected[0] = true;
            assert_eq!(closed(&local), expected, "{from}");
        }
    }

    /// A full door closes the oldest of the address that holds the most
    /// there, so a flood from many addresses can't push out a lone
    /// connection that has waited longest.
    #[test]
    fn a_full_door_closes_the_oldest_of_the_busiest_address() {
        let mut at_the_door = AtTheDoor::new();
        let lone = admitted(
            &mut at_the_door,
            IpAddr::V4(Ipv4Addr::new(198, 51, 100, 1)),
            1,
        );
        let busy: Vec<Vec<Probe>> = (1..=7)
            .map(|n| {
                admitted(
                    &mut at_the_door,
                    IpAddr::V4(Ipv4Addr::new(192, 0, 2, n)),
                    PER_ADDRESS,
                )
            })
            .collect();
        for n in 1..=7 {
            admitted(
                &mut at_the_door,
                IpAddr::V4(Ipv4Addr::new(203, 0, 113, n)),
                1,
            );
        }
        admitted(
            &mut at_the_door,
            IpAddr::V4(Ipv4Addr::new(203, 0, 113, 100)),
            1,
        );
        assert_eq!(closed(&lone), [false], "closed the lone one");
        let mut expected = vec![false; PER_ADDRESS];
        expected[0] = true;
        assert_eq!(closed(&busy[0]), expected);
        assert!(
            !closed(&busy[1]).contains(&true),
            "closed another address's"
        );
    }

    /// The addresses of one IPv6 /64 network, which one host can fill,
    /// share one share; another network's are its own.
    #[test]
    fn an_ipv6_network_holds_one_share() {
        let mut at_the_door = AtTheDoor::new();
        let host =
            |network: u16, n: u16| IpAddr::V6(Ipv6Addr::new(0x2001, 0xdb8, 0, network, 0, 0, 0, n));
        let other = admitted(&mut at_the_door, host(1, 1), 1);
        let first: Vec<Probe> = (1u16..)
            .take(PER_ADDRESS)
            .flat_map(|n| admitted(&mut at_the_door, host(0, n), 1))
            .collect();
        admitted(&mut at_the_door, host(0, 0xffff), 1);
        let mut expected = vec![false; PER_ADDRESS];
        expected[0] = true;
        assert_eq!(closed(&first), expected);
        assert_eq!(closed(&other), [false]);
    }

    /// An IPv4 address mapped into IPv6 counts in that address's share.
    #[test]
    fn a_mapped_address_counts_as_its_ipv4_address() {
        let mut at_the_door = AtTheDoor::new();
        let v4 = Ipv4Addr::new(192, 0, 2, 7);
        let first = admitted(&mut at_the_door, IpAddr::V4(v4), PER_ADDRESS);
        admitted(&mut at_the_door, IpAddr::V6(v4.to_ipv6_mapped()), 1);
        let mut expected = vec![false; PER_ADDRESS];
        expected[0] = true;
        assert_eq!(closed(&first), expected);
    }
}
