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
        let provider = provider(vec![kx_group::X25519MLKEM768, kx_group::X25519]);
        let config_error = |error: rustls::Error| ConnectError::Config {
            detail: error.to_string(),
        };
        let verifier =
            rustls_platform_verifier::Verifier::new(provider.clone()).map_err(config_error)?;
        let mut config = ClientConfig::builder_with_provider(provider)
            .with_protocol_versions(&[&rustls::version::TLS13])
            .map_err(config_error)?
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(verifier))
            .with_no_client_auth();
        config.enable_early_data = false;
        config.resumption = Resumption::disabled();
        Ok(Self {
            ca: TlsConnector::from(Arc::new(config)),
        })
    }

    async fn open(&self, route: &HubRoute) -> Result<Box<dyn HubLink>, LinkError> {
        let tcp = TcpStream::connect((route.host.as_str(), route.port))
            .await
            .map_err(unreachable)?;
        match route.session {
            RouteSession::Plain => upgrade(tcp, "ws", route, None).await,
            RouteSession::Ca => {
                let name = server_name(&route.host)?;
                let tls = self
                    .ca
                    .connect(name, tcp)
                    .await
                    .map_err(|error| refused(&error, false))?;
                upgrade(tls, "wss", route, None).await
            }
            RouteSession::Pinned(pin) => pinned(tcp, pin, route).await,
        }
    }
}

impl std::fmt::Debug for WebSocketHubConnector {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WebSocketHubConnector")
            .finish_non_exhaustive()
    }
}

#[async_trait]
impl HubConnector for WebSocketHubConnector {
    async fn connect(&self, route: &HubRoute) -> Result<Box<dyn HubLink>, LinkError> {
        tokio::time::timeout(CONNECT_WITHIN, self.open(route))
            .await
            .map_err(|_| no_answer())?
    }
}

async fn pinned(
    tcp: TcpStream,
    pin: Fingerprint,
    route: &HubRoute,
) -> Result<Box<dyn HubLink>, LinkError> {
    let connector = PinnedConnector::new(pin).map_err(|error| match error {
        ConnectError::Config { detail } => LinkError::TlsRefused { detail },
        other => LinkError::TlsRefused {
            detail: format!("{other:?}"),
        },
    })?;
    let tls = connector
        .connect(&route.host, tcp)
        .await
        .map_err(|error| match error {
            ConnectError::Handshake(error) => refused(&error, true),
            ConnectError::Host => LinkError::Unreachable {
                detail: "the address's host is no name".to_owned(),
            },
            ConnectError::Config { detail } => LinkError::TlsRefused { detail },
            ConnectError::NotPostQuantum => LinkError::TlsRefused {
                detail: "the key exchange wasn't X25519MLKEM768".to_owned(),
            },
        })?;
    let exporter = match tls.binding() {
        Ok(Binding::TlsExporter(value)) => value,
        Ok(Binding::None) => {
            return Err(LinkError::TlsRefused {
                detail: "the session has no exporter".to_owned(),
            });
        }
        Err(error) => {
            return Err(LinkError::TlsRefused {
                detail: error.detail,
            });
        }
    };
    upgrade(tls, "wss", route, Some(exporter)).await
}

fn server_name(host: &str) -> Result<ServerName<'static>, LinkError> {
    ServerName::try_from(host.to_owned()).map_err(|_| LinkError::Unreachable {
        detail: "the address's host is no name".to_owned(),
    })
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
    let host = if route.host.contains(':') {
        format!("[{}]", route.host)
    } else {
        route.host.clone()
    };
    let url = format!("{scheme}://{host}:{}{PATH}", route.port);
    let config = WebSocketConfig::default()
        .max_message_size(Some(MAX_FRAME_BYTES))
        .max_frame_size(Some(MAX_FRAME_BYTES));
    let (socket, _) = client_async_with_config(url, stream, Some(config))
        .await
        .map_err(|error| match error {
            WsError::Http(response) => LinkError::Unreachable {
                detail: format!("no hub at this address: HTTP {}", response.status()),
            },
            other => LinkError::Unreachable {
                detail: other.to_string(),
            },
        })?;
    Ok(Box::new(WebSocketLink { socket, exporter }))
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
        self.exporter
    }

    async fn send(&mut self, frame: &AgentFrame) -> Result<(), LinkError> {
        self.socket
            .send(Message::text(frame.to_text()))
            .await
            .map_err(|_| LinkError::Closed { reason: None })
    }

    async fn receive(&mut self) -> Result<HubFrame, LinkError> {
        loop {
            let next = tokio::time::timeout(ANSWER_WITHIN, self.socket.next())
                .await
                .map_err(|_| no_answer())?;
            return match next {
                Some(Ok(Message::Text(text))) => {
                    HubFrame::from_text(text.as_str()).map_err(|_| LinkError::Unreadable)
                }
                Some(Ok(Message::Ping(_) | Message::Pong(_) | Message::Frame(_))) => continue,
                Some(Ok(Message::Binary(_)) | Err(WsError::Capacity(_))) => {
                    Err(LinkError::Unreadable)
                }
                Some(Ok(Message::Close(frame))) => Err(LinkError::Closed {
                    reason: frame
                        .map(|frame| frame.reason.as_str().to_owned())
                        .filter(|reason| !reason.is_empty()),
                }),
                Some(Err(_)) | None => Err(LinkError::Closed { reason: None }),
            };
        }
    }
}

fn no_answer() -> LinkError {
    LinkError::Unreachable {
        detail: "no answer in time".to_owned(),
    }
}

fn unreachable(error: io::Error) -> LinkError {
    LinkError::Unreachable {
        detail: error.to_string(),
    }
}

/// What a failed TLS handshake means: the certificate refused, as not the
/// pinned one or not one the trust store accepts; TLS itself refused, on its
/// version or its key exchange; or, with no TLS error in it, the connection.
fn refused(error: &io::Error, pinned: bool) -> LinkError {
    let detail = error.to_string();
    match error
        .get_ref()
        .and_then(|inner| inner.downcast_ref::<rustls::Error>())
    {
        Some(rustls::Error::InvalidCertificate(_)) if pinned => LinkError::PinMismatch,
        Some(rustls::Error::InvalidCertificate(_)) => LinkError::Untrusted { detail },
        Some(_) => LinkError::TlsRefused { detail },
        None => LinkError::Unreachable { detail },
    }
}
