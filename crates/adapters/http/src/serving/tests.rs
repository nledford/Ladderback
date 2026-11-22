use std::net::{Ipv4Addr, SocketAddr};
use std::time::Duration;

use axum::Router;
use axum::extract::ws::{Message, WebSocketUpgrade};
use axum::routing::get;
use futures::{SinkExt as _, StreamExt as _};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::net::TcpStream;
use tokio::task::JoinHandle;

use super::{Bounds, serve_within};
use crate::GatedListener;

/// The head bound the tests serve with, short so they run quickly.
const HEAD_WITHIN: Duration = Duration::from_millis(300);

/// Long past `HEAD_WITHIN`, for a server that would close at its bound.
const WELL_PAST: Duration = Duration::from_secs(3);

const REQUEST: &[u8] = b"GET / HTTP/1.1\r\nHost: localhost\r\n\r\n";

const BODY: &str = "served";

/// A server on a loopback port within `bounds`, and where it listens.
async fn serving(bounds: Bounds) -> SocketAddr {
    served(bounds, std::future::pending()).await.0
}

/// A server on a loopback port within `bounds` until `shutdown`: where it
/// listens, and its run. Besides `/`, it answers `/slow` well past the head
/// bound, and echoes on a WebSocket at `/echo`.
async fn served(
    bounds: Bounds,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> (SocketAddr, JoinHandle<std::io::Result<()>>) {
    let listener = GatedListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))
        .await
        .unwrap();
    let at = listener.local_addr().unwrap();
    let app = Router::new()
        .route("/", get(|| async { BODY }))
        .route(
            "/slow",
            get(|| async {
                tokio::time::sleep(HEAD_WITHIN * 4).await;
                BODY
            }),
        )
        .route(
            "/echo",
            get(|upgrade: WebSocketUpgrade| async move {
                upgrade.on_upgrade(|mut socket| async move {
                    while let Some(Ok(Message::Text(text))) = socket.recv().await {
                        if socket.send(Message::Text(text)).await.is_err() {
                            break;
                        }
                    }
                })
            }),
        );
    let run = tokio::spawn(serve_within(bounds, listener, app, shutdown));
    (at, run)
}

fn within(connections: usize) -> Bounds {
    Bounds {
        head_within: HEAD_WITHIN,
        connections,
    }
}

/// Whether the server closed `stream` within `limit`: reading it ends.
async fn closed_within(stream: &mut TcpStream, limit: Duration) -> bool {
    let mut read = Vec::new();
    tokio::time::timeout(limit, stream.read_to_end(&mut read))
        .await
        .is_ok()
}

/// Whether a response to `REQUEST` arrives on `stream` within `limit`.
async fn answered_within(stream: &mut TcpStream, limit: Duration) -> bool {
    let reading = async {
        let mut read = Vec::new();
        let mut buffer = [0; 512];
        loop {
            let n = stream.read(&mut buffer).await.unwrap();
            assert_ne!(n, 0, "closed unanswered");
            read.extend_from_slice(buffer.get(..n).unwrap());
            if String::from_utf8_lossy(&read).ends_with(BODY) {
                return;
            }
        }
    };
    tokio::time::timeout(limit, reading).await.is_ok()
}

/// Half a request line, then nothing: the connection is closed at the
/// bound rather than held open for as long as its client likes.
#[tokio::test]
async fn a_request_head_that_stalls_is_closed_at_the_bound() {
    let at = serving(within(8)).await;
    let mut stream = TcpStream::connect(at).await.unwrap();
    stream
        .write_all(b"GET / HTTP/1.1\r\nHost: loc")
        .await
        .unwrap();
    assert!(closed_within(&mut stream, WELL_PAST).await, "still open");
}

/// A head sent a byte at a time, each in good time, still has to end
/// within the bound: the bound is on the whole head, not on each read.
#[tokio::test]
async fn a_head_that_trickles_is_closed_at_the_bound() {
    let at = serving(within(8)).await;
    let (mut reading, mut writing) = TcpStream::connect(at).await.unwrap().into_split();
    let trickle = tokio::spawn(async move {
        let mut line: &[u8] = b"GET / HTTP/1.1\r\n";
        // It ends once the server has closed the connection.
        while writing.write_all(line).await.is_ok() {
            line = b"X-A: b\r\n";
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    });
    let mut read = Vec::new();
    let closed = tokio::time::timeout(WELL_PAST, reading.read_to_end(&mut read))
        .await
        .is_ok();
    trickle.abort();
    assert!(closed, "still open");
}

/// A connection kept alive after its answer waits for its next request's
/// head within the same bound, so idle connections don't pile up.
#[tokio::test]
async fn an_idle_connection_kept_alive_is_closed_at_the_bound() {
    let at = serving(within(8)).await;
    let mut stream = TcpStream::connect(at).await.unwrap();
    stream.write_all(REQUEST).await.unwrap();
    assert!(answered_within(&mut stream, WELL_PAST).await, "unanswered");
    assert!(closed_within(&mut stream, WELL_PAST).await, "still open");
}

/// Past the bound on connections, a newcomer waits for a place rather than
/// being served, and is served once a connection ends.
#[tokio::test]
async fn a_connection_past_the_bound_waits_for_a_place() {
    let at = serving(Bounds {
        head_within: Duration::from_secs(30),
        connections: 2,
    })
    .await;
    let first = TcpStream::connect(at).await.unwrap();
    let _second = TcpStream::connect(at).await.unwrap();
    let mut third = TcpStream::connect(at).await.unwrap();
    third.write_all(REQUEST).await.unwrap();
    assert!(
        !answered_within(&mut third, Duration::from_millis(300)).await,
        "served past the bound"
    );
    drop(first);
    assert!(
        answered_within(&mut third, WELL_PAST).await,
        "still waiting once a place was free"
    );
}

/// The bound is on the head only: a response that takes longer than it
/// still arrives.
#[tokio::test]
async fn a_response_slower_than_the_head_bound_still_arrives() {
    let at = serving(within(8)).await;
    let mut stream = TcpStream::connect(at).await.unwrap();
    stream
        .write_all(b"GET /slow HTTP/1.1\r\nHost: localhost\r\n\r\n")
        .await
        .unwrap();
    assert!(answered_within(&mut stream, WELL_PAST).await, "unanswered");
}

/// A WebSocket, as the events stream and the sync channel are, outlives
/// the head bound, and keeps its place among those served while it is
/// open.
#[tokio::test]
async fn a_websocket_outlives_the_head_bound_and_keeps_its_place() {
    let at = serving(within(1)).await;
    let (mut socket, _) = tokio_tungstenite::connect_async(format!("ws://{at}/echo"))
        .await
        .unwrap();
    tokio::time::sleep(HEAD_WITHIN * 3).await;
    socket
        .send(tokio_tungstenite::tungstenite::Message::text("still here"))
        .await
        .unwrap();
    let echo = tokio::time::timeout(WELL_PAST, socket.next())
        .await
        .unwrap();
    assert_eq!(echo.unwrap().unwrap().into_text().unwrap(), "still here");
    let mut newcomer = TcpStream::connect(at).await.unwrap();
    newcomer.write_all(REQUEST).await.unwrap();
    assert!(
        !answered_within(&mut newcomer, Duration::from_millis(300)).await,
        "served past the bound while the WebSocket was open"
    );
    drop(socket);
    assert!(
        answered_within(&mut newcomer, WELL_PAST).await,
        "still waiting once the WebSocket closed"
    );
}

/// At its signal the server ends, though a connection it serves has sent
/// nothing and another waits for a place.
#[tokio::test]
async fn the_server_ends_at_its_signal_with_connections_waiting() {
    let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
    let (at, run) = served(
        Bounds {
            head_within: Duration::from_secs(30),
            connections: 1,
        },
        async move {
            let _ = stopped.await;
        },
    )
    .await;
    let mut served = TcpStream::connect(at).await.unwrap();
    let _waiting = TcpStream::connect(at).await.unwrap();
    stop.send(()).unwrap();
    let ended = tokio::time::timeout(WELL_PAST, run).await;
    assert!(matches!(ended, Ok(Ok(Ok(())))), "still serving: {ended:?}");
    assert!(closed_within(&mut served, WELL_PAST).await, "still open");
}

/// A request in flight at the signal is answered before the server ends.
#[tokio::test]
async fn a_request_in_flight_at_the_signal_is_answered() {
    let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
    let (at, run) = served(within(8), async move {
        let _ = stopped.await;
    })
    .await;
    let mut stream = TcpStream::connect(at).await.unwrap();
    stream
        .write_all(b"GET /slow HTTP/1.1\r\nHost: localhost\r\n\r\n")
        .await
        .unwrap();
    // Long enough for the server to read the head, short of the answer.
    tokio::time::sleep(HEAD_WITHIN).await;
    stop.send(()).unwrap();
    assert!(answered_within(&mut stream, WELL_PAST).await, "unanswered");
    let ended = tokio::time::timeout(WELL_PAST, run).await;
    assert!(matches!(ended, Ok(Ok(Ok(())))), "still serving: {ended:?}");
}
