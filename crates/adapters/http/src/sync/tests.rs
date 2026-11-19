use std::sync::Arc;
use std::time::Duration;

use api::sync::{AgentFrame, HubFrame, PATH};
use app::fakes::{FakeClock, FakeSigning, InMemoryAgentRegistry, InMemoryIdentity, SeededRandom};
use app::pairing::{BINDING_UNAVAILABLE, HubPairing, PROTOCOL_ERROR};
use domain::Timestamp;
use futures::{SinkExt as _, StreamExt as _};
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async};

use super::served::Bounds;
use super::*;

type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;

/// How long a test waits for the hub to say anything.
const PATIENCE: Duration = Duration::from_secs(5);

fn pairing() -> Arc<HubPairing> {
    Arc::new(HubPairing::new(
        Arc::new(InMemoryAgentRegistry::new()),
        Arc::new(InMemoryIdentity::default()),
        Arc::new(FakeSigning),
        Arc::new(SeededRandom::new(5)),
        Arc::new(FakeClock::new(
            Timestamp::from_second(1_800_000_000).unwrap(),
        )),
    ))
}

/// `hub`'s routes on loopback; their address.
async fn serve(hub: &SyncHub) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let app = routes::<()>(hub);
    tokio::spawn(async move { axum::serve(listener, app).await });
    format!("127.0.0.1:{}", address.port())
}

async fn open(address: &str) -> Socket {
    connect_async(format!("ws://{address}{PATH}"))
        .await
        .unwrap()
        .0
}

fn connect(binding_kind: u8) -> Message {
    Message::text(
        AgentFrame::Connect {
            agent_id: "00000000-0000-0000-0000-000000000001".to_owned(),
            agent_nonce: "A".repeat(43),
            binding_kind,
        }
        .to_text(),
    )
}

/// The next message, past pings; `None` when the connection ended.
async fn next(socket: &mut Socket) -> Option<Message> {
    let read = tokio::time::timeout(PATIENCE, socket.next()).await.unwrap();
    read.and_then(Result::ok)
}

/// The reason the hub closed with, `""` for none; panics on anything else.
async fn closed_with(socket: &mut Socket) -> String {
    match next(socket).await {
        Some(Message::Close(frame)) => frame.map(|f| f.reason.to_string()).unwrap_or_default(),
        None => String::new(),
        other => panic!("not closed: {other:?}"),
    }
}

#[tokio::test]
async fn a_connect_is_answered_with_the_hubs_challenge() {
    let address = serve(&SyncHub::serving(pairing())).await;
    let mut socket = open(&address).await;
    socket.send(connect(0)).await.unwrap();
    let Some(Message::Text(text)) = next(&mut socket).await else {
        panic!("no answer");
    };
    assert!(matches!(
        HubFrame::from_text(text.as_str()),
        Ok(HubFrame::Challenge { .. })
    ));
}

/// This listener has no TLS of the hub's own, so a `Connect` that binds to
/// one is closed `binding-unavailable` (decision 121).
#[tokio::test]
async fn a_connect_binding_to_tls_closes_binding_unavailable_here() {
    let address = serve(&SyncHub::serving(pairing())).await;
    let mut socket = open(&address).await;
    socket.send(connect(1)).await.unwrap();
    assert_eq!(closed_with(&mut socket).await, BINDING_UNAVAILABLE);
}

#[tokio::test]
async fn a_frame_that_isnt_one_closes_protocol_error() {
    let address = serve(&SyncHub::serving(pairing())).await;
    let mut socket = open(&address).await;
    socket
        .send(Message::text("{\"type\":\"hello\"}"))
        .await
        .unwrap();
    assert_eq!(closed_with(&mut socket).await, PROTOCOL_ERROR);
}

/// A frame over 4 KiB ends the connection before it is read (decision 120,
/// "Bounds before the proofs").
#[tokio::test]
async fn a_frame_over_4_kib_ends_the_connection() {
    let address = serve(&SyncHub::serving(pairing())).await;
    let mut socket = open(&address).await;
    let long = format!("{{\"type\":\"connect\",\"pad\":\"{}\"}}", "a".repeat(4096));
    let _ = socket.send(Message::text(long)).await;
    match next(&mut socket).await {
        None | Some(Message::Close(_)) => {}
        other => panic!("the connection went on: {other:?}"),
    }
}

#[tokio::test]
async fn a_connection_silent_past_its_first_bound_is_closed() {
    let bounds = Bounds {
        connect_within: Duration::from_millis(100),
        ..Bounds::DECIDED
    };
    let address = serve(&SyncHub::bounded(pairing(), bounds)).await;
    let mut socket = open(&address).await;
    assert_eq!(closed_with(&mut socket).await, "");
}

#[tokio::test]
async fn a_handshake_not_over_within_its_bound_is_closed() {
    let bounds = Bounds {
        handshake_within: Duration::from_millis(300),
        ..Bounds::DECIDED
    };
    let address = serve(&SyncHub::bounded(pairing(), bounds)).await;
    let mut socket = open(&address).await;
    socket.send(connect(0)).await.unwrap();
    assert!(matches!(next(&mut socket).await, Some(Message::Text(_))));
    assert_eq!(closed_with(&mut socket).await, "");
}

/// When the table of unauthenticated connections is full, the oldest is
/// evicted for the newcomer, which is served.
#[tokio::test]
async fn a_full_table_evicts_its_oldest_connection() {
    let bounds = Bounds {
        unauthenticated: 2,
        ..Bounds::DECIDED
    };
    let address = serve(&SyncHub::bounded(pairing(), bounds)).await;
    let mut oldest = open(&address).await;
    let mut second = open(&address).await;
    // Both are in the table once the hub has answered each.
    for socket in [&mut oldest, &mut second] {
        socket.send(connect(0)).await.unwrap();
        assert!(matches!(next(socket).await, Some(Message::Text(_))));
    }
    let mut newcomer = open(&address).await;
    assert_eq!(closed_with(&mut oldest).await, "");
    newcomer.send(connect(0)).await.unwrap();
    assert!(matches!(next(&mut newcomer).await, Some(Message::Text(_))));
}

/// `serve` without `--hub` answers 404 under `/sync/`, as a release does.
#[tokio::test]
async fn without_the_hub_every_path_under_sync_answers_404() {
    let address = serve(&SyncHub::default()).await;
    let refused = connect_async(format!("ws://{address}{PATH}")).await;
    let Err(tokio_tungstenite::tungstenite::Error::Http(response)) = refused else {
        panic!("upgraded without the hub");
    };
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}
