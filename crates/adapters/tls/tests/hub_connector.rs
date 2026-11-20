//! The agent's WebSocket connector meets the `HubConnector` contract the
//! loopback fake meets, over plain TCP and over the hub's own TLS, against a
//! hub that serves `/sync/v1` in this test; and a certificate the trust store
//! doesn't accept, or not the pinned one, is refused before anything is sent.

use std::sync::Arc;

use api::sync::{AgentFrame, PATH};
use app::contract;
use app::contract::hub_connector::HubConnectorHarness;
use app::pairing::{HubPairing, HubStep};
use app::ports::{HubConnector, LinkError, TlsKeys};
use async_trait::async_trait;
use domain::Timestamp;
use domain::fleet::{Binding, Fingerprint, HubRoute, RouteSession, TlsKey};
use futures::{SinkExt as _, StreamExt as _};
use tls_adapter::{HubListener, RcgenTlsKeys, SessionBinding as _, WebSocketHubConnector};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::handshake::server::{
    Callback, ErrorResponse, Request, Response,
};
use tokio_tungstenite::tungstenite::protocol::CloseFrame;
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;

fn now() -> Timestamp {
    Timestamp::from_second(1_800_000_000).unwrap()
}

fn key() -> TlsKey {
    RcgenTlsKeys.generate().unwrap()
}

/// A hub on loopback serving `pairing`'s sessions at `/sync/v1`, over the
/// hub's own TLS with `key` when it has one; its port.
async fn serve(pairing: Arc<HubPairing>, key: Option<TlsKey>) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let tls =
        key.map(|key| Arc::new(HubListener::new(&key, &["localhost".to_owned()], now()).unwrap()));
    tokio::spawn(async move {
        while let Ok((tcp, _)) = listener.accept().await {
            let pairing = pairing.clone();
            let tls = tls.clone();
            tokio::spawn(async move {
                match tls {
                    None => converse(tcp, &pairing, None).await,
                    Some(tls) => {
                        let Ok(session) = tls.accept(tcp).await else {
                            return;
                        };
                        let exporter = match session.binding() {
                            Ok(Binding::TlsExporter(value)) => Some(value),
                            _ => None,
                        };
                        converse(session, &pairing, exporter).await;
                    }
                }
            });
        }
    });
    port
}

/// Accepts the upgrade only at `/sync/v1`, and only without an `Origin`:
/// the hub's sync routes refuse a handshake that carries one as a web
/// page's (security §10.1).
struct AtSyncPath;

impl Callback for AtSyncPath {
    fn on_request(self, request: &Request, response: Response) -> Result<Response, ErrorResponse> {
        assert_eq!(request.uri().path(), PATH);
        assert_eq!(
            request.headers().get("origin"),
            None,
            "the agent sent an Origin"
        );
        Ok(response)
    }
}

async fn converse<S>(stream: S, pairing: &HubPairing, exporter: Option<[u8; 32]>)
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let Ok(mut socket) = tokio_tungstenite::accept_hdr_async(stream, AtSyncPath).await else {
        return;
    };
    let mut session = pairing.session(exporter).await.unwrap();
    while let Some(Ok(Message::Text(text))) = socket.next().await {
        let frame = AgentFrame::from_text(text.as_str()).unwrap();
        match pairing.step(&mut session, &frame).await {
            HubStep::Reply(answer) => socket.send(Message::text(answer.to_text())).await.unwrap(),
            HubStep::Wait => {}
            HubStep::Last(answer) => {
                socket.send(Message::text(answer.to_text())).await.unwrap();
                return;
            }
            HubStep::Close(reason) => {
                let frame = CloseFrame {
                    code: CloseCode::Policy,
                    reason: reason.into(),
                };
                let _ = socket.close(Some(frame)).await;
                return;
            }
            HubStep::Fail(error) => panic!("the hub failed: {error:?}"),
        }
    }
}

/// The connector to a hub in this test, over a session of `session`'s kind:
/// plain TCP, or the hub's own TLS with the key `key`.
struct Served {
    key: Option<TlsKey>,
}

impl Served {
    fn route(&self, port: u16) -> HubRoute {
        let session = match &self.key {
            None => RouteSession::Plain,
            Some(key) => RouteSession::Pinned(RcgenTlsKeys.fingerprint(key).unwrap()),
        };
        HubRoute {
            host: "127.0.0.1".to_owned(),
            port,
            session,
        }
    }
}

#[async_trait]
impl HubConnectorHarness for Served {
    async fn hub(&self, pairing: Arc<HubPairing>) -> (Arc<dyn HubConnector>, HubRoute) {
        let port = serve(pairing, self.key.clone()).await;
        (Arc::new(WebSocketHubConnector::new()), self.route(port))
    }

    async fn unanswered(&self) -> (Arc<dyn HubConnector>, HubRoute) {
        // A port that was free a moment ago, and now has nothing on it.
        let port = TcpListener::bind("127.0.0.1:0")
            .await
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        (Arc::new(WebSocketHubConnector::new()), self.route(port))
    }
}

#[tokio::test]
async fn the_websocket_connector_meets_the_hub_connector_contract_over_plain_tcp() {
    contract::hub_connector::run(&Served { key: None }).await;
}

#[tokio::test]
async fn the_websocket_connector_meets_the_hub_connector_contract_over_the_hubs_own_tls() {
    contract::hub_connector::run(&Served { key: Some(key()) }).await;
}

fn hub() -> Arc<HubPairing> {
    use app::fakes::{
        FakeClock, FakeSigning, InMemoryAgentRegistry, InMemoryIdentity, SeededRandom,
    };

    Arc::new(HubPairing::new(
        Arc::new(InMemoryAgentRegistry::new()),
        Arc::new(InMemoryIdentity::default()),
        Arc::new(FakeSigning),
        Arc::new(SeededRandom::new(3)),
        Arc::new(FakeClock::new(now())),
        "0.0.0-test".to_owned(),
    ))
}

/// A CA-checked address whose hub shows a certificate no authority this
/// machine trusts signed, as the hub's own self-signed one, is refused as
/// untrusted, before any frame.
#[tokio::test]
async fn a_certificate_the_trust_store_doesnt_accept_is_untrusted() {
    let port = serve(hub(), Some(key())).await;
    let route = HubRoute {
        host: "localhost".to_owned(),
        port,
        session: RouteSession::Ca,
    };
    let link = WebSocketHubConnector::new().connect(&route).await;
    assert!(
        matches!(link, Err(LinkError::Untrusted { .. })),
        "{:?}",
        link.err()
    );
}

/// A pinned address whose hub shows another key than the pinned one is
/// refused as a pin mismatch.
#[tokio::test]
async fn another_key_than_the_pinned_one_is_a_pin_mismatch() {
    let port = serve(hub(), Some(key())).await;
    let route = HubRoute {
        host: "127.0.0.1".to_owned(),
        port,
        session: RouteSession::Pinned(Fingerprint::from_digest([7; 32])),
    };
    let link = WebSocketHubConnector::new().connect(&route).await;
    assert_eq!(link.err(), Some(LinkError::PinMismatch));
}

/// A server that answers the upgrade with no WebSocket, as a daemon
/// without the hub does, is no hub.
#[tokio::test]
async fn an_address_with_no_hub_at_its_path_is_unreachable() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        use tokio::io::AsyncWriteExt as _;
        while let Ok((mut tcp, _)) = listener.accept().await {
            let _ = tcp
                .write_all(b"HTTP/1.1 404 Not Found\r\ncontent-length: 0\r\n\r\n")
                .await;
        }
    });
    let route = HubRoute {
        host: "127.0.0.1".to_owned(),
        port,
        session: RouteSession::Plain,
    };
    let link = WebSocketHubConnector::new().connect(&route).await;
    assert!(
        matches!(&link, Err(LinkError::Unreachable { detail }) if detail.contains("404")),
        "{:?}",
        link.err()
    );
}

/// A machine whose trust store holds no CA roots, as a Linux container with
/// no certificates installed.
fn without_ca_roots() -> WebSocketHubConnector {
    WebSocketHubConnector::with_verifier(|_| Err(rustls::Error::General("no CA roots".to_owned())))
}

/// A plain address needs no CA roots, so none missing doesn't stop it.
#[tokio::test]
async fn a_plain_address_connects_on_a_machine_with_no_ca_roots() {
    let port = serve(hub(), None).await;
    let route = HubRoute {
        host: "127.0.0.1".to_owned(),
        port,
        session: RouteSession::Plain,
    };
    let link = without_ca_roots().connect(&route).await;
    assert!(link.is_ok(), "{:?}", link.err());
}

/// A pinned address checks the one key its token names, so no CA roots are
/// needed to refuse another.
#[tokio::test]
async fn a_pinned_address_is_checked_on_a_machine_with_no_ca_roots() {
    let port = serve(hub(), Some(key())).await;
    let route = HubRoute {
        host: "127.0.0.1".to_owned(),
        port,
        session: RouteSession::Pinned(Fingerprint::from_digest([7; 32])),
    };
    let link = without_ca_roots().connect(&route).await;
    assert_eq!(link.err(), Some(LinkError::PinMismatch));
}

/// A CA-checked address has nothing to check the certificate against with
/// no roots, so it is refused, not trusted.
#[tokio::test]
async fn a_ca_checked_address_is_refused_on_a_machine_with_no_ca_roots() {
    let port = serve(hub(), Some(key())).await;
    let route = HubRoute {
        host: "localhost".to_owned(),
        port,
        session: RouteSession::Ca,
    };
    let link = without_ca_roots().connect(&route).await;
    assert!(
        matches!(&link, Err(LinkError::TlsRefused { detail }) if detail.contains("no CA roots")),
        "{:?}",
        link.err()
    );
}
