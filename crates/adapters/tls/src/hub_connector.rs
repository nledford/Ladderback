//! The agent's way to its hub's `/sync/v1` (§7.1; §16, decision 120, "The
//! handshake" and "Crates"): a WebSocket over plain TCP on this computer,
//! over TLS checked against the operating system's trust store for a
//! CA-checked address, or over TLS with the pinned certificate, whose
//! `tls-exporter` value the handshake binds to. Frames over
//! [`MAX_FRAME_BYTES`] are refused, and a hub that doesn't answer in time is
//! unreachable.

use std::io;
use std::sync::Arc;
use std::time::Duration;

use api::sync::{AgentFrame, HubFrame, MAX_FRAME_BYTES, PATH};
use app::ports::{HubConnector, HubLink, LinkError};
use async_trait::async_trait;
use domain::fleet::{Binding, Fingerprint, HubRoute, KEY_BYTES, RouteSession};
use futures::{SinkExt as _, StreamExt as _};
use rustls::ClientConfig;
use rustls::client::Resumption;
use rustls::crypto::aws_lc_rs::kx_group;
use rustls::pki_types::ServerName;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;
use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;
use tokio_tungstenite::tungstenite::{Error as WsError, Message};
use tokio_tungstenite::{WebSocketStream, client_async_with_config};

use crate::{ConnectError, PinnedConnector, SessionBinding as _, provider};

/// How long the connection, its TLS and its upgrade may take: decision 121's
/// "no answer in 10 seconds".
pub const CONNECT_WITHIN: Duration = Duration::from_secs(10);

/// How long the hub may take to answer a frame: the 15 s the hub gives the
/// whole handshake (decision 120, "Bounds before the proofs").
pub const ANSWER_WITHIN: Duration = Duration::from_secs(15);

/// The agent's [`HubConnector`]: its CA-checked TLS is TLS 1.3 alone, the
/// hybrid key exchange then X25519, no early data and no resumption, and a
/// certificate the operating system's trust store accepts for the address's
/// name (`rustls-platform-verifier`).
pub struct WebSocketHubConnector {
    ca: TlsConnector,
}

impl WebSocketHubConnector {
    /// The connector, with the operating system's trust store.
    pub fn new() -> Result<Self, ConnectError> {
        todo!()
    }

    async fn open(&self, route: &HubRoute) -> Result<Box<dyn HubLink>, LinkError> {
        todo!()
    }
}

impl std::fmt::Debug for WebSocketHubConnector {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        todo!()
    }
}

#[async_trait]
impl HubConnector for WebSocketHubConnector {
    async fn connect(&self, route: &HubRoute) -> Result<Box<dyn HubLink>, LinkError> {
        todo!()
    }
}

async fn pinned(
    tcp: TcpStream,
    pin: Fingerprint,
    route: &HubRoute,
) -> Result<Box<dyn HubLink>, LinkError> {
        todo!()
    }

fn server_name(host: &str) -> Result<ServerName<'static>, LinkError> {
        todo!()
    }

/// The WebSocket upgrade to `/sync/v1` over `stream`.
async fn upgrade<S>(
    stream: S,
    scheme: &str,
    route: &HubRoute,
    exporter: Option<[u8; KEY_BYTES]>,
) -> Result<Box<dyn HubLink>, LinkError>
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
        todo!()
    }

struct WebSocketLink<S> {
    socket: WebSocketStream<S>,
    exporter: Option<[u8; KEY_BYTES]>,
}

#[async_trait]
impl<S> HubLink for WebSocketLink<S>
where
    S: AsyncRead + AsyncWrite + Unpin + Send,
{
    fn exporter(&self) -> Option<[u8; KEY_BYTES]> {
        todo!()
    }

    async fn send(&mut self, frame: &AgentFrame) -> Result<(), LinkError> {
        todo!()
    }

    async fn receive(&mut self) -> Result<HubFrame, LinkError> {
        todo!()
    }
}

fn no_answer() -> LinkError {
        todo!()
    }

fn unreachable(error: io::Error) -> LinkError {
        todo!()
    }

/// What a failed TLS handshake means: the certificate refused, as not the
/// pinned one or not one the trust store accepts; TLS itself refused, on its
/// version or its key exchange; or, with no TLS error in it, the connection.
fn refused(error: &io::Error, pinned: bool) -> LinkError {
        todo!()
    }
