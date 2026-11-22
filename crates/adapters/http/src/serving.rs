//! How the daemon serves the connections its listener takes, the plain one
//! or a hub's front door (security §10.1, "Connections"): each has a time
//! limit to send a request's head, and only so many are served at once.
//!
//! axum's own `serve` sets hyper no timer and takes no configuration, so it
//! can't bound how long a head takes; this loop is that `serve` with the
//! bounds, over HTTP/1.1 only.

use std::io;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::serve::Listener;
use hyper::body::Incoming;
use hyper::server::conn::http1;
use hyper_util::rt::{TokioIo, TokioTimer};
use hyper_util::service::TowerToHyperService;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::sync::{OwnedSemaphorePermit, Semaphore, watch};
use tower::ServiceExt as _;

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
    /// 30 s is hyper's own default for the head, ample for any client to
    /// send a few KiB, and the time an idle connection is kept alive. 128
    /// connections is far past what a person's tabs or a hub's agents hold,
    /// and half the 256 descriptors macOS gives a process by default, so
    /// the store, the workers' pipes and the repository files keep theirs.
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
///
/// At `shutdown` it drops the listener, so it takes no connection more,
/// asks each connection to end once its request in flight is answered,
/// and ends once they have. A connection upgraded, as a WebSocket is, has
/// left the server by then and isn't waited for.
pub fn serve_connections<L: ServedListener>(
    listener: L,
    app: Router,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> impl Future<Output = io::Result<()>> + Send {
    serve_within(Bounds::DECIDED, listener, app, shutdown)
}

pub(crate) async fn serve_within<L: ServedListener>(
    bounds: Bounds,
    mut listener: L,
    app: Router,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> io::Result<()> {
    let places = Arc::new(Semaphore::new(bounds.connections));
    let (stop, stopping) = watch::channel(());
    // Each connection's task holds a receiver; once none is left, all have
    // ended.
    let (open, served) = watch::channel(());
    tokio::pin!(shutdown);
    loop {
        // A place first, so a connection past the bound waits in the
        // listening socket's queue, unaccepted, rather than in the server.
        let place = tokio::select! {
            place = Arc::clone(&places).acquire_owned() => place,
            () = &mut shutdown => break,
        };
        // The semaphore is never closed.
        let Ok(place) = place else { break };
        let (io, addr) = tokio::select! {
            accepted = listener.accept() => accepted,
            () = &mut shutdown => break,
        };
        tokio::spawn(connection(
            bounds,
            Placed { io, _place: place },
            L::peer(addr),
            app.clone(),
            stopping.clone(),
            served.clone(),
        ));
    }
    drop(listener);
    // No receiver is an error only once every connection has ended.
    let _ = stop.send(());
    drop(served);
    open.closed().await;
    Ok(())
}

/// One connection served until it ends, or, once `stopping` changes, until
/// its request in flight is answered. `_open` tells the server it still
/// runs.
async fn connection<I, P>(
    bounds: Bounds,
    io: Placed<I>,
    peer: P,
    app: Router,
    mut stopping: watch::Receiver<()>,
    _open: watch::Receiver<()>,
) where
    I: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    P: Clone + Send + Sync + 'static,
{
    let service = app.map_request(move |request: hyper::Request<Incoming>| {
        let mut request = request.map(Body::new);
        request.extensions_mut().insert(ConnectInfo(peer.clone()));
        request
    });
    let mut builder = http1::Builder::new();
    builder
        .timer(TokioTimer::new())
        .header_read_timeout(bounds.head_within);
    let served = builder
        .serve_connection(TokioIo::new(io), TowerToHyperService::new(service))
        .with_upgrades();
    tokio::pin!(served);
    let ended = tokio::select! {
        ended = served.as_mut() => ended,
        // Changed, or the server gone: either way it stops.
        _ = stopping.changed() => {
            served.as_mut().graceful_shutdown();
            served.await
        }
    };
    if let Err(error) = ended {
        log::debug!("a connection ended: {error}");
    }
}

/// A connection with its place among those served: the place is let go
/// when the connection closes, an upgraded one's included, since its bytes
/// travel on with it.
struct Placed<I> {
    io: I,
    _place: OwnedSemaphorePermit,
}

impl<I: AsyncRead + Unpin> AsyncRead for Placed<I> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.io).poll_read(cx, buf)
    }
}

impl<I: AsyncWrite + Unpin> AsyncWrite for Placed<I> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.io).poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.io).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.io).poll_shutdown(cx)
    }

    fn poll_write_vectored(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &[io::IoSlice<'_>],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.io).poll_write_vectored(cx, bufs)
    }

    fn is_write_vectored(&self) -> bool {
        self.io.is_write_vectored()
    }
}

#[cfg(test)]
mod tests;
