//! Connections a hub's front door hands the server: each one plain, from
//! loopback, or decrypted from the hub's built-in TLS, with that session's
//! binding (decision 120, "Binding to the TLS session").

use std::fmt;
use std::io;
use std::net::SocketAddr;

use axum::Router;
use axum::extract::{ConnectInfo, Request};
use axum::middleware::{self, Next};
use axum::response::Response;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use crate::PeerAddr;

/// A request that came over the hub's built-in TLS: the session's
/// `tls-exporter` value when it negotiated the hybrid key exchange, which a
/// kind-1 `Connect` binds to, and `None` otherwise.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct BuiltInTls {
    pub exporter: Option<[u8; 32]>,
}

impl fmt::Debug for BuiltInTls {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BuiltInTls")
            .field("post_quantum", &self.exporter.is_some())
            .finish_non_exhaustive()
    }
}

/// A connection's bytes, plain or decrypted.
pub trait HandedIo: AsyncRead + AsyncWrite + Unpin + Send + 'static {}

impl<T: AsyncRead + AsyncWrite + Unpin + Send + 'static> HandedIo for T {}

/// Where a handed connection came from, and its built-in TLS session when
/// it came over one (`axum::extract::ConnectInfo<HandedPeer>`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HandedPeer {
    pub addr: SocketAddr,
    pub tls: Option<BuiltInTls>,
}

/// A connection the front door let in.
pub struct Handed {
    pub io: Box<dyn HandedIo>,
    pub peer: HandedPeer,
}

/// The server's end of the front door, for `serve_connections`: the connections
/// it lets in, in the order it lets them in. Dropped, it stops the door.
pub struct HandedListener {
    connections: mpsc::Receiver<Handed>,
    local: SocketAddr,
    door: JoinHandle<()>,
}

impl HandedListener {
    /// The connections `door`, the task that accepts on `local`, sends.
    pub fn new(
        connections: mpsc::Receiver<Handed>,
        local: SocketAddr,
        door: JoinHandle<()>,
    ) -> Self {
        Self {
            connections,
            local,
            door,
        }
    }
}

impl Drop for HandedListener {
    fn drop(&mut self) {
        self.door.abort();
    }
}

impl fmt::Debug for HandedListener {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HandedListener")
            .field("local", &self.local)
            .finish_non_exhaustive()
    }
}

impl axum::serve::Listener for HandedListener {
    type Io = Box<dyn HandedIo>;
    type Addr = HandedPeer;

    /// The next connection the door lets in. A door that has stopped lets
    /// none in again, so the server waits for its stop.
    async fn accept(&mut self) -> (Self::Io, Self::Addr) {
        match self.connections.recv().await {
            Some(handed) => (handed.io, handed.peer),
            None => std::future::pending().await,
        }
    }

    /// The listening address, with no session of its own.
    fn local_addr(&self) -> io::Result<Self::Addr> {
        Ok(HandedPeer {
            addr: self.local,
            tls: None,
        })
    }
}

/// `router` served through [`HandedListener`]: each request carries its
/// peer as every route reads it, `ConnectInfo<PeerAddr>`, and a
/// [`BuiltInTls`] extension when it came over the built-in TLS. The layer
/// is the outermost, so every check and route sees both.
pub fn handed(router: Router) -> Router {
    router.layer(middleware::from_fn(hand_on))
}

async fn hand_on(
    ConnectInfo(peer): ConnectInfo<HandedPeer>,
    mut request: Request,
    next: Next,
) -> Response {
    request
        .extensions_mut()
        .insert(ConnectInfo(PeerAddr(peer.addr)));
    if let Some(tls) = peer.tls {
        request.extensions_mut().insert(tls);
    }
    next.run(request).await
}
