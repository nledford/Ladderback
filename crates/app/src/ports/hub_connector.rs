use api::sync::{AgentFrame, HubFrame};
use async_trait::async_trait;
use domain::fleet::{HubRoute, KEY_BYTES};

/// The agent's way to its hub's `/sync/v1` (§7.1; §16, decision 120, "The
/// handshake"): one WebSocket connection to a route, over plain TCP on this
/// computer, TLS checked against the operating system's trust store for a
/// CA-checked address, or TLS with the pinned certificate. It carries frames
/// and holds no rule of the handshake; a frame over
/// [`api::sync::MAX_FRAME_BYTES`] ends the connection.
#[async_trait]
pub trait HubConnector: Send + Sync {
    /// The connection to `route`, once its TLS, when it has one, and its
    /// WebSocket upgrade are done.
    async fn connect(&self, route: &HubRoute) -> Result<Box<dyn HubLink>, LinkError>;
}

/// One open connection to the hub.
#[async_trait]
pub trait HubLink: Send {
    /// The session's `tls-exporter` value (RFC 9266) on the hub's own TLS,
    /// a pinned route's; `None` on any other.
    fn exporter(&self) -> Option<[u8; KEY_BYTES]>;

    async fn send(&mut self, frame: &AgentFrame) -> Result<(), LinkError>;

    /// The hub's next frame, within the link's deadline for an answer.
    async fn receive(&mut self) -> Result<HubFrame, LinkError>;

    /// The hub's next frame, whenever it comes, as a command the hub sends
    /// unasked (§16, decision 176): no deadline, and cancel-safe, so a
    /// listen given up loses no frame. A link with no deadline of its own
    /// hears it as [`Self::receive`] does.
    async fn listen(&mut self) -> Result<HubFrame, LinkError> {
        self.receive().await
    }
}

/// Why the connection couldn't be opened or went on no further. None
/// carries anything of the token (I4); `detail` is the system's or the
/// library's own text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LinkError {
    /// No hub answered: the connection was refused, the name didn't
    /// resolve, or nothing answered in time.
    Unreachable { detail: String },
    /// The certificate at a pinned address isn't the one the token names.
    PinMismatch,
    /// The certificate at a CA-checked address isn't one this machine's
    /// trust store accepts for the address's name.
    Untrusted { detail: String },
    /// The TLS handshake failed on its version or its key exchange: the
    /// hub, or a proxy before it, doesn't speak TLS 1.3, or, at a pinned
    /// address, the hybrid key exchange.
    TlsRefused { detail: String },
    /// The hub closed the connection, with the code it gave as the reason
    /// when it gave one.
    Closed { reason: Option<String> },
    /// The hub sent what isn't a frame: binary, too long, or not one of the
    /// messages.
    Unreadable,
}
