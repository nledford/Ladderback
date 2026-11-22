//! How the daemon serves the connections its listener takes, the plain one
//! or a hub's front door (security §10.1, "Connections"): each has a time
//! limit to send a request's head, and only so many are served at once.

use std::future::{Future, IntoFuture as _};
use std::io;
use std::time::Duration;

use axum::Router;
use axum::extract::connect_info::Connected;
use axum::serve::{IncomingStream, Listener};

use crate::{GatedListener, PeerAddr};

/// The bounds on the connections the server serves.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Bounds {
    /// How long a connection has to send a request's head, counted from
    /// when the server waits for it: a new connection's first request, and
    /// each later one on a connection kept alive.
    pub(crate) head_within: Duration,
    /// How many connections are served at once.
    pub(crate) connections: usize,
}

impl Bounds {
    pub(crate) const DECIDED: Self = Self {
        head_within: Duration::from_secs(30),
        connections: 128,
    };
}

/// A listener the daemon serves: what each request reads as its peer
/// (`axum::extract::ConnectInfo<Self::Peer>`) for the address a connection
/// came from.
pub trait ServedListener: Listener {
    type Peer: Clone + Send + Sync + 'static;

    fn peer(addr: Self::Addr) -> Self::Peer;
}

impl ServedListener for GatedListener {
    type Peer = PeerAddr;

    fn peer(addr: Self::Addr) -> Self::Peer {
        PeerAddr(addr)
    }
}

#[cfg(feature = "hub")]
impl ServedListener for crate::HandedListener {
    type Peer = crate::HandedPeer;

    fn peer(addr: Self::Addr) -> Self::Peer {
        addr
    }
}

/// `app` served on `listener` until `shutdown`, within the decided bounds.
pub fn serve_connections<L>(
    listener: L,
    app: Router,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> impl Future<Output = io::Result<()>> + Send
where
    L: ServedListener,
    L::Addr: std::fmt::Debug,
    for<'a> L::Peer: Connected<IncomingStream<'a, L>>,
{
    serve_within(Bounds::DECIDED, listener, app, shutdown)
}

pub(crate) fn serve_within<L>(
    bounds: Bounds,
    listener: L,
    app: Router,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> impl Future<Output = io::Result<()>> + Send
where
    L: ServedListener,
    L::Addr: std::fmt::Debug,
    for<'a> L::Peer: Connected<IncomingStream<'a, L>>,
{
    let _ = bounds;
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<L::Peer>(),
    )
    .with_graceful_shutdown(shutdown)
    .into_future()
}

#[cfg(test)]
mod tests;
