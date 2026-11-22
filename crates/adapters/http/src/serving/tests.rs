use std::net::{Ipv4Addr, SocketAddr};
use std::time::Duration;

use axum::Router;
use axum::routing::get;
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::net::TcpStream;

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
    let listener = GatedListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))
        .await
        .unwrap();
    let at = listener.local_addr().unwrap();
    let app = Router::new().route("/", get(|| async { BODY }));
    tokio::spawn(serve_within(bounds, listener, app, std::future::pending()));
    at
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
