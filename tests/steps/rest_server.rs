//! A REST server on loopback for the `@e2e` scenarios about what a server
//! says of the credentials it is sent (#478). It takes only the user
//! [`USER`] with the password `canary-not-a-secret`, refusing any other
//! request with `401 Unauthorized`, and it holds no repository: every
//! request it takes is answered `404 Not Found`. That is all of rustic's
//! REST server these scenarios need, so the runner needs no `rustic-server`.

use std::net::SocketAddr;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

/// The one user the server takes.
pub(crate) const USER: &str = "demo";

/// Not a real secret: a password the server refuses.
pub(crate) const WRONG_SERVER_PASSWORD: &str = "canary-wrong-secret";

/// The `Authorization` header of the one user and password the server
/// takes: HTTP Basic, the two base64-encoded as `demo:canary-not-a-secret`.
const ACCEPTED: &str = "Basic ZGVtbzpjYW5hcnktbm90LWEtc2VjcmV0";

/// The most of one request's head the server reads.
const MAX_HEAD: usize = 64 * 1024;

/// Starts the server on a free loopback port. It serves until the runner
/// ends.
async fn serve() -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("a port");
    let address = listener.local_addr().expect("an address");
    tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            tokio::spawn(answer(stream));
        }
    });
    address
}

/// Answers one request, once its head has arrived: `404` with the accepted
/// credentials, `401` with any other or none.
async fn answer(mut stream: TcpStream) {
    let Some(head) = read_head(&mut stream).await else {
        return;
    };
    let accepted = head.lines().any(|line| {
        line.split_once(':').is_some_and(|(name, value)| {
            name.eq_ignore_ascii_case("authorization") && value.trim() == ACCEPTED
        })
    });
    let status = if accepted {
        "404 Not Found"
    } else {
        "401 Unauthorized"
    };
    let _ = stream
        .write_all(
            format!("HTTP/1.1 {status}\r\ncontent-length: 0\r\nconnection: close\r\n\r\n")
                .as_bytes(),
        )
        .await;
}

/// A request's head: its request line and headers, up to the blank line.
async fn read_head(stream: &mut TcpStream) -> Option<String> {
    let mut bytes = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        let read = stream.read(&mut chunk).await.ok()?;
        if read == 0 || bytes.len() > MAX_HEAD {
            return None;
        }
        bytes.extend_from_slice(chunk.get(..read)?);
        if let Some(end) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
            return String::from_utf8(bytes.get(..end)?.to_vec()).ok();
        }
    }
}

/// The location of a new server, as rustic writes it, with the user and
/// `password` in its URL.
pub(crate) async fn location(password: &str) -> String {
    format!("rest:http://{USER}:{password}@{}/", serve().await)
}
