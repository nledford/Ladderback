//! The hub's built-in TLS against real peers over an in-memory pipe (§16,
//! decision 120, "Binding to the TLS session", "The hub key" and "Tests that
//! pin it"): the agent accepts only the hub whose certificate's key is the
//! pinned one, and proves it holds that key; it ignores the certificate's
//! dates and names; both ends refuse TLS 1.2; a pinned agent takes only the
//! hybrid key exchange, while the listener also serves X25519 to browsers;
//! and both ends of one session, and only that session, share its binding.

use std::sync::Arc;

use app::ports::{TlsKeyError, TlsKeys};
use domain::Timestamp;
use domain::fleet::{Binding, Fingerprint, TlsKey};
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::aws_lc_rs::{self, kx_group};
use rustls::crypto::{CryptoProvider, SupportedKxGroup};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer, ServerName, UnixTime};
use rustls::server::{ClientHello, ParsedCertificate, ResolvesServerCert};
use rustls::sign::CertifiedKey;
use rustls::{
    ClientConfig, DigitallySignedStruct, NamedGroup, ProtocolVersion, ServerConfig,
    SignatureScheme, SupportedProtocolVersion,
};
use tls_adapter::{
    ConnectError, EXPORTER_LABEL, HubListener, ListenerError, PinnedConnector, RcgenTlsKeys,
    SessionBinding, certificate,
};
use tokio::io::DuplexStream;
use tokio_rustls::{TlsAcceptor, TlsConnector, client, server};

const HUB: &str = "hub.example";

fn now() -> Timestamp {
    Timestamp::from_second(1_800_000_000).unwrap()
}

fn names() -> Vec<String> {
    vec![HUB.to_owned()]
}

fn new_key() -> TlsKey {
    RcgenTlsKeys.generate().unwrap()
}

fn pin(key: &TlsKey) -> Fingerprint {
    RcgenTlsKeys.fingerprint(key).unwrap()
}

fn listener(key: &TlsKey) -> HubListener {
    HubListener::new(key, &names(), now()).unwrap()
}

fn connector(key: &TlsKey) -> PinnedConnector {
    PinnedConnector::new(pin(key)).unwrap()
}

/// A provider of aws-lc-rs with only `groups`.
fn provider(groups: &[&'static dyn SupportedKxGroup]) -> Arc<CryptoProvider> {
    Arc::new(CryptoProvider {
        kx_groups: groups.to_vec(),
        ..aws_lc_rs::default_provider()
    })
}

fn private_key(key: &TlsKey) -> PrivateKeyDer<'static> {
    PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key.der().to_vec()))
}

/// The hub's listener and the agent's client, one handshake between them.
async fn handshake(
    listener: &HubListener,
    connector: &PinnedConnector,
    host: &str,
) -> (
    std::io::Result<server::TlsStream<DuplexStream>>,
    Result<client::TlsStream<DuplexStream>, ConnectError>,
) {
    let (hub, agent) = tokio::io::duplex(1 << 16);
    tokio::join!(listener.accept(hub), connector.connect(host, agent))
}

/// The agent's client against a server that isn't the hub's listener.
async fn connect_to(
    server: ServerConfig,
    connector: &PinnedConnector,
) -> Result<client::TlsStream<DuplexStream>, ConnectError> {
    let (hub, agent) = tokio::io::duplex(1 << 16);
    let acceptor = TlsAcceptor::from(Arc::new(server));
    let (_, connected) = tokio::join!(acceptor.accept(hub), connector.connect(HUB, agent));
    connected
}

/// A client that isn't an agent, a browser say, against the hub's listener.
async fn client_of(
    client: ClientConfig,
    listener: &HubListener,
) -> (
    std::io::Result<server::TlsStream<DuplexStream>>,
    std::io::Result<client::TlsStream<DuplexStream>>,
) {
    let (hub, other) = tokio::io::duplex(1 << 16);
    let connector = TlsConnector::from(Arc::new(client));
    let name = ServerName::try_from(HUB).unwrap();
    tokio::join!(listener.accept(hub), connector.connect(name, other))
}

/// A server that shows `chain` and signs with `signer`, whatever they are.
#[derive(Debug)]
struct Shows(Arc<CertifiedKey>);

impl ResolvesServerCert for Shows {
    fn resolve(&self, _hello: ClientHello<'_>) -> Option<Arc<CertifiedKey>> {
        Some(self.0.clone())
    }
}

fn server_showing(chain: Vec<CertificateDer<'static>>, signer: &TlsKey) -> ServerConfig {
    let provider = provider(&[kx_group::X25519MLKEM768]);
    let signing_key = provider
        .key_provider
        .load_private_key(private_key(signer))
        .unwrap();
    ServerConfig::builder_with_provider(provider)
        .with_protocol_versions(&[&rustls::version::TLS13])
        .unwrap()
        .with_no_client_auth()
        .with_cert_resolver(Arc::new(Shows(Arc::new(CertifiedKey::new(
            chain,
            signing_key,
        )))))
}

fn server_of(
    key: &TlsKey,
    groups: &[&'static dyn SupportedKxGroup],
    versions: &[&'static SupportedProtocolVersion],
) -> ServerConfig {
    ServerConfig::builder_with_provider(provider(groups))
        .with_protocol_versions(versions)
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(
            vec![certificate(key, &names(), now()).unwrap()],
            private_key(key),
        )
        .unwrap()
}

/// A browser's verifier, which a person told to take the hub's certificate.
#[derive(Debug)]
struct TakesAnything;

impl ServerCertVerifier for TakesAnything {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        aws_lc_rs::default_provider()
            .signature_verification_algorithms
            .supported_schemes()
    }
}

fn client_of_kind(
    groups: &[&'static dyn SupportedKxGroup],
    versions: &[&'static SupportedProtocolVersion],
) -> ClientConfig {
    ClientConfig::builder_with_provider(provider(groups))
        .with_protocol_versions(versions)
        .unwrap()
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(TakesAnything))
        .with_no_client_auth()
}

fn exporter_value(binding: Binding) -> [u8; 32] {
    match binding {
        Binding::TlsExporter(value) => value,
        Binding::None => panic!("a TLS session binds as kind 1"),
    }
}

#[tokio::test]
async fn an_agent_pinned_to_the_hubs_key_connects_on_tls13_and_the_hybrid() {
    let key = new_key();
    let (hub, agent) = handshake(&listener(&key), &connector(&key), HUB).await;
    let (hub, agent) = (hub.unwrap(), agent.unwrap());
    let session = agent.get_ref().1;
    assert_eq!(session.protocol_version(), Some(ProtocolVersion::TLSv1_3));
    assert_eq!(
        session
            .negotiated_key_exchange_group()
            .map(|group| group.name()),
        Some(NamedGroup::X25519MLKEM768)
    );
    assert!(agent.is_post_quantum());
    assert!(hub.is_post_quantum());
}

/// The fingerprint agents pin is the SHA-256 of the certificate's
/// SubjectPublicKeyInfo as the listener presents it.
#[tokio::test]
async fn the_pinned_fingerprint_is_that_of_the_certificates_public_key() {
    let key = new_key();
    let (_, agent) = handshake(&listener(&key), &connector(&key), HUB).await;
    let agent = agent.unwrap();
    let presented = agent.get_ref().1.peer_certificates().unwrap();
    assert_eq!(presented.len(), 1, "the hub's certificate alone");
    let spki = ParsedCertificate::try_from(&presented[0])
        .unwrap()
        .subject_public_key_info();
    assert_eq!(Fingerprint::of_bytes(spki.as_ref()), pin(&key));
}

#[tokio::test]
async fn both_ends_of_a_session_share_its_exporter_value() {
    let key = new_key();
    let (hub, agent) = handshake(&listener(&key), &connector(&key), HUB).await;
    let (hub, agent) = (hub.unwrap(), agent.unwrap());
    let agent_binding = agent.binding().unwrap();
    assert_eq!(agent_binding.kind(), 1);
    assert_eq!(hub.binding().unwrap(), agent_binding);
    // RFC 9266: the label `EXPORTER-Channel-Binding`, an empty context
    // (in TLS 1.3 the same value as none) and 32 bytes.
    let by_hand = hub
        .get_ref()
        .1
        .export_keying_material([0; 32], b"EXPORTER-Channel-Binding", None)
        .unwrap();
    assert_eq!(exporter_value(agent_binding), by_hand);
    assert_eq!(EXPORTER_LABEL, b"EXPORTER-Channel-Binding");
}

#[tokio::test]
async fn two_sessions_have_two_exporter_values() {
    let key = new_key();
    let (listener, connector) = (listener(&key), connector(&key));
    let (_, first) = handshake(&listener, &connector, HUB).await;
    let (_, second) = handshake(&listener, &connector, HUB).await;
    assert_ne!(
        first.unwrap().binding().unwrap(),
        second.unwrap().binding().unwrap()
    );
}

#[tokio::test]
async fn an_agent_refuses_a_hub_with_another_tls_key() {
    let (hub_key, pinned_key) = (new_key(), new_key());
    let (hub, agent) = handshake(&listener(&hub_key), &connector(&pinned_key), HUB).await;
    assert!(
        matches!(agent, Err(ConnectError::Handshake(_))),
        "{agent:?}"
    );
    assert!(hub.is_err());
}

/// A certificate made again from the same key, after the old one lapsed or
/// for another name, keeps every agent's pin.
#[tokio::test]
async fn an_agent_takes_a_lapsed_or_otherwise_named_certificate_with_the_pinned_key() {
    let key = new_key();
    let long_ago = Timestamp::from_second(946_684_800).unwrap();
    let lapsed = HubListener::new(&key, &["elsewhere.example".to_owned()], long_ago).unwrap();
    let (hub, agent) = handshake(&lapsed, &connector(&key), HUB).await;
    assert!(agent.is_ok(), "{agent:?}");
    assert!(hub.is_ok());
}

/// A server that shows the pinned certificate but signs with another key
/// fails the `CertificateVerify` check.
#[tokio::test]
async fn an_agent_refuses_the_pinned_certificate_without_its_private_key() {
    let (pinned, other) = (new_key(), new_key());
    let pinned_certificate = certificate(&pinned, &names(), now()).unwrap();
    let server = server_showing(vec![pinned_certificate], &other);
    let agent = connect_to(server, &connector(&pinned)).await;
    assert!(
        matches!(agent, Err(ConnectError::Handshake(_))),
        "{agent:?}"
    );
}

/// Only the certificate the hub is: the pinned one as an intermediate below
/// another key's certificate counts for nothing.
#[tokio::test]
async fn an_agent_refuses_the_pinned_certificate_behind_another() {
    let (pinned, other) = (new_key(), new_key());
    let chain = vec![
        certificate(&other, &names(), now()).unwrap(),
        certificate(&pinned, &names(), now()).unwrap(),
    ];
    let agent = connect_to(server_showing(chain, &other), &connector(&pinned)).await;
    assert!(
        matches!(agent, Err(ConnectError::Handshake(_))),
        "{agent:?}"
    );
}

#[tokio::test]
async fn an_agent_refuses_a_certificate_that_isnt_one() {
    let key = new_key();
    let junk = vec![CertificateDer::from(b"not a certificate".to_vec())];
    let agent = connect_to(server_showing(junk, &key), &connector(&key)).await;
    assert!(
        matches!(agent, Err(ConnectError::Handshake(_))),
        "{agent:?}"
    );
}

#[tokio::test]
async fn an_agent_refuses_a_hub_that_speaks_only_tls12() {
    let key = new_key();
    let server = server_of(
        &key,
        &[kx_group::X25519MLKEM768, kx_group::X25519],
        &[&rustls::version::TLS12],
    );
    let agent = connect_to(server, &connector(&key)).await;
    assert!(
        matches!(agent, Err(ConnectError::Handshake(_))),
        "{agent:?}"
    );
}

#[tokio::test]
async fn an_agent_refuses_a_hub_that_offers_only_x25519() {
    let key = new_key();
    let server = server_of(&key, &[kx_group::X25519], &[&rustls::version::TLS13]);
    let agent = connect_to(server, &connector(&key)).await;
    assert!(
        matches!(agent, Err(ConnectError::Handshake(_))),
        "{agent:?}"
    );
}

#[tokio::test]
async fn the_listener_refuses_a_tls12_client() {
    let key = new_key();
    let client = client_of_kind(
        &[kx_group::X25519MLKEM768, kx_group::X25519],
        &[&rustls::version::TLS12],
    );
    let (hub, other) = client_of(client, &listener(&key)).await;
    assert!(hub.is_err());
    assert!(other.is_err());
}

/// The listener serves browsers too, which may offer X25519 alone; the
/// session says so, for the hub's `binding-unavailable` check.
#[tokio::test]
async fn the_listener_serves_x25519_to_a_browser_and_says_it_isnt_post_quantum() {
    let key = new_key();
    let client = client_of_kind(&[kx_group::X25519], &[&rustls::version::TLS13]);
    let (hub, other) = client_of(client, &listener(&key)).await;
    assert!(other.is_ok(), "{other:?}");
    assert!(!hub.unwrap().is_post_quantum());
}

/// The client's order decides: a browser that prefers the hybrid gets it.
#[tokio::test]
async fn the_listener_takes_the_hybrid_a_browser_prefers() {
    let key = new_key();
    let client = client_of_kind(
        &[kx_group::X25519MLKEM768, kx_group::X25519],
        &[&rustls::version::TLS13],
    );
    let (hub, other) = client_of(client, &listener(&key)).await;
    assert!(other.unwrap().is_post_quantum());
    assert!(hub.unwrap().is_post_quantum());
}

fn group_names(provider: &CryptoProvider) -> Vec<NamedGroup> {
    provider
        .kx_groups
        .iter()
        .map(|group| group.name())
        .collect()
}

/// One provider, aws-lc-rs, with the groups named here, TLS 1.3 suites
/// alone, and early data off on both ends.
#[test]
fn every_config_names_its_groups_and_takes_no_early_data() {
    let key = new_key();
    let hub = listener(&key);
    let hub = hub.config();
    assert_eq!(
        group_names(hub.crypto_provider()),
        [NamedGroup::X25519MLKEM768, NamedGroup::X25519]
    );
    assert_eq!(hub.max_early_data_size, 0);
    let agent = connector(&key);
    let agent = agent.config();
    assert_eq!(
        group_names(agent.crypto_provider()),
        [NamedGroup::X25519MLKEM768]
    );
    assert!(!agent.enable_early_data);
    for config in [hub.crypto_provider(), agent.crypto_provider()] {
        assert!(
            config
                .cipher_suites
                .iter()
                .all(|suite| suite.tls13().is_some()),
            "TLS 1.3 suites alone"
        );
        assert_eq!(
            config.cipher_suites.len(),
            aws_lc_rs::default_provider()
                .cipher_suites
                .iter()
                .filter(|suite| suite.tls13().is_some())
                .count()
        );
    }
}

#[test]
fn a_key_of_another_kind_is_refused() {
    for algorithm in [&rcgen::PKCS_ED25519, &rcgen::PKCS_ECDSA_P384_SHA384] {
        let other = rcgen::KeyPair::generate_for(algorithm).unwrap();
        let key = TlsKey::from_der(other.serialize_der()).unwrap();
        assert_eq!(RcgenTlsKeys.fingerprint(&key), Err(TlsKeyError::NotAKey));
        assert_eq!(
            HubListener::new(&key, &names(), now()).unwrap_err(),
            ListenerError::Key(TlsKeyError::NotAKey)
        );
    }
}

#[test]
fn a_name_a_certificate_cant_carry_is_refused() {
    let refused = certificate(&new_key(), &["hüb.example".to_owned()], now());
    assert!(
        matches!(refused, Err(ListenerError::Certificate { .. })),
        "{refused:?}"
    );
}

#[test]
fn a_new_key_is_ecdsa_p256() {
    let key = new_key();
    let pair = rcgen::KeyPair::try_from(key.der()).unwrap();
    assert!(pair.is_compatible(&rcgen::PKCS_ECDSA_P256_SHA256));
}
