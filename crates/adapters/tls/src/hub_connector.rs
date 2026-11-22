//! The agent's way to its hub's `/sync/v1` (§7.1; §16, decision 120, "The
//! handshake" and "Crates"): a WebSocket over plain TCP on this computer,
//! over TLS checked against the operating system's trust store for a
//! CA-checked address, or over TLS with the pinned certificate, whose
//! `tls-exporter` value the handshake binds to. Frames over
//! [`MAX_FRAME_BYTES`] are refused, and a hub that doesn't answer in time is
//! unreachable.

use std::io;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use api::sync::{AgentFrame, HubFrame, MAX_FRAME_BYTES, PATH};
use app::ports::{HubConnector, HubLink, LinkError};
use async_trait::async_trait;
use domain::fleet::{Binding, Fingerprint, HubRoute, KEY_BYTES, RouteSession};
use futures::{SinkExt as _, StreamExt as _};
use rustls::ClientConfig;
use rustls::client::Resumption;
use rustls::client::danger::ServerCertVerifier;
use rustls::crypto::CryptoProvider;
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

/// What builds the verifier of a CA-checked route's certificate.
type Roots = Box<
    dyn Fn(Arc<CryptoProvider>) -> Result<Arc<dyn ServerCertVerifier>, rustls::Error> + Send + Sync,
>;

/// The agent's [`HubConnector`]: its CA-checked TLS is TLS 1.3 alone, the
/// hybrid key exchange then X25519, no early data and no resumption, and a
/// certificate the operating system's trust store accepts for the address's
/// name (`rustls-platform-verifier`).
pub struct WebSocketHubConnector {
    roots: Roots,
    /// Built when the first CA-checked route opens: a plain or a pinned one
    /// has no use for the trust store.
    ca: OnceLock<TlsConnector>,
}

impl WebSocketHubConnector {
    /// The connector, with the operating system's trust store for its
    /// CA-checked routes.
    pub fn new() -> Self {
        Self::with_verifier(|provider| {
            let verifier = rustls_platform_verifier::Verifier::new(provider)?;
            Ok(Arc::new(verifier))
        })
    }

    /// A connector whose CA-checked routes trust the verifier `roots`
    /// builds, when the first such route is opened and not before. A plain
    /// or a pinned route never asks for it, so a machine with no CA roots
    /// pairs over either.
    pub fn with_verifier(
        roots: impl Fn(Arc<CryptoProvider>) -> Result<Arc<dyn ServerCertVerifier>, rustls::Error>
        + Send
        + Sync
        + 'static,
    ) -> Self {
        Self {
            roots: Box::new(roots),
            ca: OnceLock::new(),
        }
    }

    /// The TLS connector of a CA-checked route. Failing to build it fails
    /// that route alone.
    fn ca_connector(&self) -> Result<TlsConnector, LinkError> {
        if let Some(ca) = self.ca.get() {
            return Ok(ca.clone());
        }
        let refused = |error: rustls::Error| LinkError::TlsRefused {
            detail: error.to_string(),
        };
        let provider = provider(vec![kx_group::X25519MLKEM768, kx_group::X25519]);
        let verifier = (self.roots)(provider.clone()).map_err(refused)?;
        let mut config = ClientConfig::builder_with_provider(provider)
            .with_protocol_versions(&[&rustls::version::TLS13])
            .map_err(refused)?
            .dangerous()
            .with_custom_certificate_verifier(verifier)
            .with_no_client_auth();
        config.enable_early_data = false;
        config.resumption = Resumption::disabled();
        Ok(self
            .ca
            .get_or_init(|| TlsConnector::from(Arc::new(config)))
            .clone())
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
                    .ca_connector()?
                    .connect(name, tcp)
                    .await
                    .map_err(|error| refused(&error, false))?;
                upgrade(tls, "wss", route, None).await
            }
            RouteSession::Pinned(pin) => pinned(tcp, pin, route).await,
        }
    }
}

impl Default for WebSocketHubConnector {
    fn default() -> Self {
        Self::new()
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
    let (socket, _) = client_async_with_config(url, stream, Some(config()))
        .await
        .map_err(|error| match error {
            WsError::Http(response) => LinkError::Unreachable {
                detail: format!("no hub at this address: HTTP {}", response.status()),
            },
            other => LinkError::Unreachable {
                detail: other.to_string(),
            },
        })?;
    Ok(Box::new(WebSocketLink {
        socket,
        exporter,
        within: ANSWER_WITHIN,
    }))
}

/// Writes go out at once, and what a hub that doesn't read leaves queued,
/// its pings' pongs among it, stays bounded.
fn config() -> WebSocketConfig {
    WebSocketConfig::default()
        .max_message_size(Some(MAX_FRAME_BYTES))
        .max_frame_size(Some(MAX_FRAME_BYTES))
        .write_buffer_size(0)
        .max_write_buffer_size(2 * MAX_FRAME_BYTES)
}

struct WebSocketLink<S> {
    socket: WebSocketStream<S>,
    exporter: Option<[u8; KEY_BYTES]>,
    /// How long the hub may take over one frame.
    within: Duration,
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
        // A hub that stops reading can block the write, as its pongs and this
        // frame wait for room: the same deadline as an answer ends the wait.
        tokio::time::timeout(
            self.within,
            self.socket.send(Message::text(frame.to_text())),
        )
        .await
        .map_err(|_| no_answer())?
        .map_err(|_| LinkError::Closed { reason: None })
    }

    async fn receive(&mut self) -> Result<HubFrame, LinkError> {
        // One deadline for the answer, however many pings come before it.
        let deadline = tokio::time::Instant::now() + self.within;
        loop {
            let next = tokio::time::timeout_at(deadline, self.socket.next())
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

#[cfg(test)]
mod tests {
    use tokio_tungstenite::tungstenite::protocol::Role;

    use super::*;

    /// A hub that stops reading, so a frame the agent writes can't leave:
    /// the pipe holds 8 bytes and nothing empties it. The write gives up
    /// within its deadline, with an error that has the caller drop the link.
    #[tokio::test]
    async fn a_frame_a_hub_never_reads_is_given_up_on_within_its_deadline() {
        let (agent, _hub_that_never_reads) = tokio::io::duplex(8);
        let socket = WebSocketStream::from_raw_socket(agent, Role::Client, Some(config())).await;
        let mut link = WebSocketLink {
            socket,
            exporter: None,
            within: Duration::from_millis(200),
        };
        let frame = AgentFrame::Connect {
            agent_id: "00000000-0000-0000-0000-000000000001".to_owned(),
            agent_nonce: "A".repeat(43),
            binding_kind: 0,
        };
        let sent = tokio::time::timeout(Duration::from_secs(2), link.send(&frame)).await;
        assert!(
            matches!(sent, Ok(Err(LinkError::Unreachable { .. }))),
            "the write gave no error within its deadline: {sent:?}"
        );
    }

    /// A hub that keeps the connection open and answers nothing, as a hub
    /// that hangs while the agent waits for an acknowledgement: the read
    /// gives up within its deadline, so the agent's sync loop drops the
    /// link and tries again after its back-off.
    #[tokio::test]
    async fn a_hub_that_never_answers_is_given_up_on_within_its_deadline() {
        let (agent, _silent_hub) = tokio::io::duplex(4096);
        let socket = WebSocketStream::from_raw_socket(agent, Role::Client, Some(config())).await;
        let mut link = WebSocketLink {
            socket,
            exporter: None,
            within: Duration::from_millis(200),
        };
        let received = tokio::time::timeout(Duration::from_secs(2), link.receive()).await;
        assert!(
            matches!(received, Ok(Err(LinkError::Unreachable { .. }))),
            "the read gave no error within its deadline: {received:?}"
        );
    }
}
