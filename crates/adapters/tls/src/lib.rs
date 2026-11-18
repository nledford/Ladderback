//! The hub's built-in TLS (§16, decision 120, "Binding to the TLS session",
//! "The hub key" and "Crates"): the listener, its self-signed certificate
//! made with `rcgen` from the hub's TLS key, the agent's client pinned to
//! that key, and the session's `tls-exporter` value the handshake binds to.
//!
//! Every config is TLS 1.3 alone, from one crypto provider, aws-lc-rs, with
//! its key exchange groups named here rather than taken from the provider's
//! defaults, and with early data off. No process default provider is
//! installed, and clippy bans the builders that would read one.

#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

use std::fmt;
use std::io;
use std::sync::Arc;

use app::ports::{TlsKeyError, TlsKeys};
use domain::Timestamp;
use domain::fleet::{Binding, Fingerprint, TlsKey};
use rustls::pki_types::CertificateDer;
use rustls::{ClientConfig, ServerConfig};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_rustls::{TlsAcceptor, TlsConnector, client, server};

/// The label of the `tls-exporter` channel binding (RFC 9266), with an
/// empty context and 32 bytes.
pub const EXPORTER_LABEL: &[u8] = b"EXPORTER-Channel-Binding";

/// The hub's TLS key: ECDSA P-256, made and read with `rcgen` over
/// aws-lc-rs.
#[derive(Clone, Copy, Debug, Default)]
pub struct RcgenTlsKeys;

impl TlsKeys for RcgenTlsKeys {
    #[expect(clippy::todo, reason = "the red test: commit's stub")]
    fn generate(&self) -> Result<TlsKey, TlsKeyError> {
        todo!()
    }

    #[expect(clippy::todo, reason = "the red test: commit's stub")]
    fn fingerprint(&self, _key: &TlsKey) -> Result<Fingerprint, TlsKeyError> {
        todo!()
    }
}

/// The hub's self-signed certificate for `key`, naming `names` (host names
/// or addresses), valid from a day before `now` for 397 days.
#[expect(clippy::todo, reason = "the red test: commit's stub")]
pub fn certificate(
    _key: &TlsKey,
    _names: &[String],
    _now: Timestamp,
) -> Result<CertificateDer<'static>, ListenerError> {
    todo!()
}

/// The hub's TLS listener: TLS 1.3 alone, the hybrid key exchange and
/// X25519, since browsers share it, and no early data.
pub struct HubListener {
    config: Arc<ServerConfig>,
    acceptor: TlsAcceptor,
}

impl HubListener {
    /// The listener serving a certificate made from `key` at `now`, naming
    /// `names`.
    #[expect(clippy::todo, reason = "the red test: commit's stub")]
    pub fn new(_key: &TlsKey, _names: &[String], _now: Timestamp) -> Result<Self, ListenerError> {
        todo!()
    }

    pub fn config(&self) -> &Arc<ServerConfig> {
        &self.config
    }

    /// The TLS session over `io`, once its handshake is done.
    pub async fn accept<IO>(&self, io: IO) -> io::Result<server::TlsStream<IO>>
    where
        IO: AsyncRead + AsyncWrite + Unpin,
    {
        self.acceptor.accept(io).await
    }
}

impl fmt::Debug for HubListener {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HubListener").finish_non_exhaustive()
    }
}

/// The agent's client to a pinned address: TLS 1.3 alone, the hybrid key
/// exchange alone, no early data and no resumption, and a hub whose
/// certificate's key is the pinned one.
pub struct PinnedConnector {
    config: Arc<ClientConfig>,
    connector: TlsConnector,
}

impl PinnedConnector {
    /// The client that accepts only the key whose fingerprint is `pin`.
    #[expect(clippy::todo, reason = "the red test: commit's stub")]
    pub fn new(_pin: Fingerprint) -> Result<Self, ConnectError> {
        todo!()
    }

    pub fn config(&self) -> &Arc<ClientConfig> {
        &self.config
    }

    /// The TLS session over `io` with the hub at `host`, once its handshake
    /// is done on the hybrid key exchange.
    #[expect(clippy::todo, reason = "the red test: commit's stub")]
    pub async fn connect<IO>(
        &self,
        _host: &str,
        _io: IO,
    ) -> Result<client::TlsStream<IO>, ConnectError>
    where
        IO: AsyncRead + AsyncWrite + Unpin,
    {
        let _ = &self.connector;
        todo!()
    }
}

impl fmt::Debug for PinnedConnector {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PinnedConnector").finish_non_exhaustive()
    }
}

/// What a TLS session tells the handshake about itself.
pub trait SessionBinding {
    /// The session's `tls-exporter` value (RFC 9266), as both ends compute
    /// it.
    fn binding(&self) -> Result<Binding, BindingError>;

    /// Whether the session's key exchange was the hybrid `X25519MLKEM768`.
    fn is_post_quantum(&self) -> bool;
}

impl<IO> SessionBinding for client::TlsStream<IO> {
    #[expect(clippy::todo, reason = "the red test: commit's stub")]
    fn binding(&self) -> Result<Binding, BindingError> {
        todo!()
    }

    #[expect(clippy::todo, reason = "the red test: commit's stub")]
    fn is_post_quantum(&self) -> bool {
        todo!()
    }
}

impl<IO> SessionBinding for server::TlsStream<IO> {
    #[expect(clippy::todo, reason = "the red test: commit's stub")]
    fn binding(&self) -> Result<Binding, BindingError> {
        todo!()
    }

    #[expect(clippy::todo, reason = "the red test: commit's stub")]
    fn is_post_quantum(&self) -> bool {
        todo!()
    }
}

/// Why the listener couldn't be made. None carries any of the key (I4).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ListenerError {
    /// The key isn't one the hub can serve.
    Key(TlsKeyError),
    /// The certificate couldn't be made: a name it can't carry, or the
    /// library's refusal; `detail` is its message.
    Certificate { detail: String },
    /// rustls refused the config; `detail` is its message.
    Config { detail: String },
}

/// Why the agent couldn't open a session with its hub. None carries any of
/// a key (I4).
#[derive(Debug)]
pub enum ConnectError {
    /// rustls refused the config; `detail` is its message.
    Config { detail: String },
    /// The address's host is neither a name nor an IP address.
    Host,
    /// The handshake failed: another key than the pinned one, TLS 1.2, no
    /// key exchange in common, or the connection itself.
    Handshake(io::Error),
    /// The session's key exchange wasn't the hybrid.
    NotPostQuantum,
}

/// The exporter refused: the session's handshake isn't done.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BindingError {
    pub detail: String,
}
