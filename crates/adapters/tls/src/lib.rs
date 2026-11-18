//! The hub's built-in TLS (§16, decision 120, "Binding to the TLS session",
//! "The hub key" and "Crates"): the listener, its self-signed certificate
//! made with `rcgen` from the hub's TLS key, the agent's client pinned to
//! that key, and the session's `tls-exporter` value the handshake binds to.
//!
//! Every config is TLS 1.3 alone, from one crypto provider, aws-lc-rs, with
//! its cipher suites and key exchange groups named here rather than taken
//! from the provider's defaults, and with early data off. No process default
//! provider is installed or read.

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
use domain::fleet::{Binding, Fingerprint, TlsKey};
use domain::{SignedDuration, TimeZone, Timestamp};
use rcgen::{
    CertificateParams, DistinguishedName, DnType, ExtendedKeyUsagePurpose, KeyPair,
    KeyUsagePurpose, PKCS_ECDSA_P256_SHA256, PublicKeyData as _,
};
use rustls::client::Resumption;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::aws_lc_rs::{self, kx_group};
use rustls::crypto::{CryptoProvider, SupportedKxGroup, verify_tls13_signature};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer, ServerName, UnixTime};
use rustls::server::ParsedCertificate;
use rustls::{
    CertificateError, ClientConfig, DigitallySignedStruct, NamedGroup, ServerConfig,
    SignatureScheme,
};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_rustls::{TlsAcceptor, TlsConnector, client, server};

/// The label of the `tls-exporter` channel binding (RFC 9266), with an
/// empty context and 32 bytes.
pub const EXPORTER_LABEL: &[u8] = b"EXPORTER-Channel-Binding";

/// How long a certificate is valid from `now`: the 397 days browsers take
/// at most. The agent ignores it; a browser that was told to take the
/// certificate may not.
const VALID_FOR_DAYS: i64 = 397;

/// How far before `now` a certificate is valid from, for a peer whose clock
/// runs behind.
const BACKDATED_DAYS: i64 = 1;

/// The one signature scheme the hub's key signs with, and so the only one
/// the agent's verifier takes.
const HUB_SCHEME: SignatureScheme = SignatureScheme::ECDSA_NISTP256_SHA256;

/// The hub's TLS key: ECDSA P-256, made and read with `rcgen` over
/// aws-lc-rs.
#[derive(Clone, Copy, Debug, Default)]
pub struct RcgenTlsKeys;

impl TlsKeys for RcgenTlsKeys {
    fn generate(&self) -> Result<TlsKey, TlsKeyError> {
        let pair = KeyPair::generate_for(&PKCS_ECDSA_P256_SHA256).map_err(unavailable)?;
        TlsKey::from_der(pair.serialize_der()).map_err(|refused| TlsKeyError::Unavailable {
            detail: format!("{refused:?}"),
        })
    }

    fn fingerprint(&self, key: &TlsKey) -> Result<Fingerprint, TlsKeyError> {
        Ok(Fingerprint::of_bytes(
            &key_pair(key)?.subject_public_key_info(),
        ))
    }
}

/// `key` as an ECDSA P-256 key pair, or [`TlsKeyError::NotAKey`].
fn key_pair(key: &TlsKey) -> Result<KeyPair, TlsKeyError> {
    let der = PrivatePkcs8KeyDer::from(key.der());
    KeyPair::from_pkcs8_der_and_sign_algo(&der, &PKCS_ECDSA_P256_SHA256)
        .map_err(|_| TlsKeyError::NotAKey)
}

/// rcgen's own message: it names what failed, never a key's bytes.
fn unavailable(error: rcgen::Error) -> TlsKeyError {
    TlsKeyError::Unavailable {
        detail: error.to_string(),
    }
}

/// The hub's self-signed certificate for `key`, naming `names` (host names
/// or addresses), valid from a day before `now` for 397 days.
pub fn certificate(
    key: &TlsKey,
    names: &[String],
    now: Timestamp,
) -> Result<CertificateDer<'static>, ListenerError> {
    let pair = key_pair(key).map_err(ListenerError::Key)?;
    let mut params = CertificateParams::new(names.to_vec()).map_err(certificate_error)?;
    let mut name = DistinguishedName::new();
    name.push(DnType::CommonName, "Ladderback hub");
    params.distinguished_name = name;
    let (year, month, day) = date(now, -BACKDATED_DAYS)?;
    params.not_before = rcgen::date_time_ymd(year, month, day);
    let (year, month, day) = date(now, VALID_FOR_DAYS)?;
    params.not_after = rcgen::date_time_ymd(year, month, day);
    params.key_usages = vec![KeyUsagePurpose::DigitalSignature];
    params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
    let certificate = params.self_signed(&pair).map_err(certificate_error)?;
    Ok(certificate.der().clone())
}

/// The year, month and day, in UTC, `days` after `now`'s; the certificate
/// counts from their midnight.
fn date(now: Timestamp, days: i64) -> Result<(i32, u8, u8), ListenerError> {
    let out_of_range = || ListenerError::Certificate {
        detail: format!("a certificate can't be valid {days} days from now"),
    };
    let day = now
        .checked_add(SignedDuration::from_hours(days.saturating_mul(24)))
        .map_err(|_| out_of_range())?
        .to_zoned(TimeZone::UTC)
        .date();
    let month = u8::try_from(day.month()).map_err(|_| out_of_range())?;
    let day_of_month = u8::try_from(day.day()).map_err(|_| out_of_range())?;
    Ok((i32::from(day.year()), month, day_of_month))
}

fn certificate_error(error: rcgen::Error) -> ListenerError {
    ListenerError::Certificate {
        detail: error.to_string(),
    }
}

/// aws-lc-rs with its TLS 1.3 cipher suites alone and `groups` alone.
fn provider(groups: Vec<&'static dyn SupportedKxGroup>) -> Arc<CryptoProvider> {
    let base = aws_lc_rs::default_provider();
    Arc::new(CryptoProvider {
        cipher_suites: base
            .cipher_suites
            .iter()
            .copied()
            .filter(|suite| suite.tls13().is_some())
            .collect(),
        kx_groups: groups,
        ..base
    })
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
    pub fn new(key: &TlsKey, names: &[String], now: Timestamp) -> Result<Self, ListenerError> {
        let chain = vec![certificate(key, names, now)?];
        let private = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key.der().to_vec()));
        let mut config = ServerConfig::builder_with_provider(provider(vec![
            kx_group::X25519MLKEM768,
            kx_group::X25519,
        ]))
        .with_protocol_versions(&[&rustls::version::TLS13])
        .map_err(config_error)?
        .with_no_client_auth()
        .with_single_cert(chain, private)
        .map_err(config_error)?;
        config.max_early_data_size = 0;
        let config = Arc::new(config);
        Ok(Self {
            acceptor: TlsAcceptor::from(config.clone()),
            config,
        })
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

fn config_error(error: rustls::Error) -> ListenerError {
    ListenerError::Config {
        detail: error.to_string(),
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
    pub fn new(pin: Fingerprint) -> Result<Self, ConnectError> {
        let provider = provider(vec![kx_group::X25519MLKEM768]);
        let verifier = PinnedVerifier {
            pin,
            provider: provider.clone(),
        };
        let mut config = ClientConfig::builder_with_provider(provider)
            .with_protocol_versions(&[&rustls::version::TLS13])
            .map_err(|error| ConnectError::Config {
                detail: error.to_string(),
            })?
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(verifier))
            .with_no_client_auth();
        config.enable_early_data = false;
        config.resumption = Resumption::disabled();
        let config = Arc::new(config);
        Ok(Self {
            connector: TlsConnector::from(config.clone()),
            config,
        })
    }

    pub fn config(&self) -> &Arc<ClientConfig> {
        &self.config
    }

    /// The TLS session over `io` with the hub at `host`, once its handshake
    /// is done on the hybrid key exchange.
    pub async fn connect<IO>(
        &self,
        host: &str,
        io: IO,
    ) -> Result<client::TlsStream<IO>, ConnectError>
    where
        IO: AsyncRead + AsyncWrite + Unpin,
    {
        let name = ServerName::try_from(host.to_owned()).map_err(|_| ConnectError::Host)?;
        let session = self
            .connector
            .connect(name, io)
            .await
            .map_err(ConnectError::Handshake)?;
        if session.is_post_quantum() {
            Ok(session)
        } else {
            Err(ConnectError::NotPostQuantum)
        }
    }
}

impl fmt::Debug for PinnedConnector {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PinnedConnector").finish_non_exhaustive()
    }
}

/// The agent's check of its hub's certificate: its public key's fingerprint
/// is the pin, and the hub signed the handshake with that key. The
/// certificate's dates, names and issuer are ignored, so a lapsed
/// certificate, or a hub reached by another name, still verifies.
#[derive(Debug)]
struct PinnedVerifier {
    pin: Fingerprint,
    provider: Arc<CryptoProvider>,
}

/// Refused: not the hub's certificate.
fn not_the_hub() -> rustls::Error {
    rustls::Error::InvalidCertificate(CertificateError::ApplicationVerificationFailure)
}

impl ServerCertVerifier for PinnedVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        // The hub shows its own certificate alone.
        if !intermediates.is_empty() {
            return Err(not_the_hub());
        }
        let parsed = ParsedCertificate::try_from(end_entity)?;
        let presented = Fingerprint::of_bytes(parsed.subject_public_key_info().as_ref());
        if presented == self.pin {
            Ok(ServerCertVerified::assertion())
        } else {
            Err(not_the_hub())
        }
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        // Unreached: every config is TLS 1.3 alone.
        Err(not_the_hub())
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        if dss.scheme != HUB_SCHEME {
            return Err(rustls::Error::PeerMisbehaved(
                rustls::PeerMisbehaved::SignedHandshakeWithUnadvertisedSigScheme,
            ));
        }
        verify_tls13_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        vec![HUB_SCHEME]
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

/// The exporter value of the session `connection` is one end of.
fn exporter<Data>(connection: &rustls::ConnectionCommon<Data>) -> Result<Binding, BindingError> {
    connection
        .export_keying_material([0; 32], EXPORTER_LABEL, Some(&[]))
        .map(Binding::TlsExporter)
        .map_err(|error| BindingError {
            detail: error.to_string(),
        })
}

fn hybrid<Data>(connection: &rustls::ConnectionCommon<Data>) -> bool {
    connection
        .negotiated_key_exchange_group()
        .is_some_and(|group| group.name() == NamedGroup::X25519MLKEM768)
}

impl<IO> SessionBinding for client::TlsStream<IO> {
    fn binding(&self) -> Result<Binding, BindingError> {
        exporter(self.get_ref().1)
    }

    fn is_post_quantum(&self) -> bool {
        hybrid(self.get_ref().1)
    }
}

impl<IO> SessionBinding for server::TlsStream<IO> {
    fn binding(&self) -> Result<Binding, BindingError> {
        exporter(self.get_ref().1)
    }

    fn is_post_quantum(&self) -> bool {
        hybrid(self.get_ref().1)
    }
}

/// Why the listener couldn't be made. None carries any of the key (I4).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ListenerError {
    /// The key isn't one the hub can serve.
    Key(TlsKeyError),
    /// The certificate couldn't be made: a name it can't carry, a date out
    /// of its range, or the library's refusal; `detail` is its message.
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

#[cfg(test)]
mod tests {
    use super::*;

    /// No TLS 1.2 handshake reaches the verifier, since every config is
    /// TLS 1.3 alone; were one to, on a config the tests alone can build,
    /// the hub's own key signing, it is refused.
    #[tokio::test]
    async fn the_pinned_verifier_refuses_any_tls12_handshake() {
        let key = RcgenTlsKeys.generate().unwrap();
        let tls12 = Arc::new(CryptoProvider {
            kx_groups: vec![kx_group::X25519],
            ..aws_lc_rs::default_provider()
        });
        let verifier = PinnedVerifier {
            pin: RcgenTlsKeys.fingerprint(&key).unwrap(),
            provider: tls12.clone(),
        };
        let client = ClientConfig::builder_with_provider(tls12.clone())
            .with_protocol_versions(&[&rustls::version::TLS12])
            .unwrap()
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(verifier))
            .with_no_client_auth();
        let server = ServerConfig::builder_with_provider(tls12)
            .with_protocol_versions(&[&rustls::version::TLS12])
            .unwrap()
            .with_no_client_auth()
            .with_single_cert(
                vec![certificate(&key, &[], Timestamp::UNIX_EPOCH).unwrap()],
                PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key.der().to_vec())),
            )
            .unwrap();
        let (hub, agent) = tokio::io::duplex(1 << 16);
        let name = ServerName::try_from("hub.example").unwrap();
        let (_, connected) = tokio::join!(
            TlsAcceptor::from(Arc::new(server)).accept(hub),
            TlsConnector::from(Arc::new(client)).connect(name, agent)
        );
        let refused = connected.unwrap_err().into_inner().unwrap();
        assert_eq!(
            refused.downcast_ref::<rustls::Error>(),
            Some(&not_the_hub())
        );
    }
}
