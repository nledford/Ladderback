//! The HTTP notifier meets the `Notifier` contract against channels served on
//! loopback: one small HTTP server reads each request as a webhook, an ntfy
//! server or a healthchecks server would, and answers as the channel's
//! behaviour says.

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use app::contract::notifier::{Behaviour, NotifierHarness, Received, ReceivedAuth};
use app::ports::Notifier;
use base64::Engine as _;
use domain::notifications::{ChannelKind, Ping};
use l10n::Localizer;
use notify_adapter::HttpNotifier;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

/// How long a delivery may take here: long enough for loopback, short
/// enough that the check of a channel that never answers is quick.
const TIMEOUT: Duration = Duration::from_millis(500);

/// The most of a request the server reads.
const MAX_REQUEST: usize = 64 * 1024;

#[derive(Default)]
struct Served {
    kinds: Mutex<HashMap<usize, ChannelKind>>,
    received: Mutex<Vec<Received>>,
}

struct LoopbackHarness {
    notifier: HttpNotifier,
    port: u16,
    next: AtomicUsize,
    served: Arc<Served>,
}

impl LoopbackHarness {
    async fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("a port");
        let port = listener.local_addr().expect("an address").port();
        let served = Arc::new(Served::default());
        let serving = served.clone();
        tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                tokio::spawn(answer(stream, serving.clone()));
            }
        });
        let localizer = Localizer::new(&["en-US".parse().expect("a locale")]);
        Self {
            notifier: HttpNotifier::new(localizer, TIMEOUT).expect("a client"),
            port,
            next: AtomicUsize::new(0),
            served,
        }
    }
}

#[async_trait::async_trait]
impl NotifierHarness for LoopbackHarness {
    fn notifier(&self) -> &dyn Notifier {
        &self.notifier
    }

    fn url(&self, kind: ChannelKind, behaviour: Behaviour) -> String {
        let number = self.next.fetch_add(1, Ordering::Relaxed);
        self.served
            .kinds
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(number, kind);
        let port = self.port;
        match behaviour {
            Behaviour::Accepts => format!("http://127.0.0.1:{port}/accept/{number}"),
            Behaviour::Answers(status) => {
                format!("http://127.0.0.1:{port}/status/{status}/{number}")
            }
            Behaviour::Hangs => format!("http://127.0.0.1:{port}/hang/{number}"),
            Behaviour::Refuses => {
                // A port that was free a moment ago, and that nothing listens at.
                let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("a port");
                let port = listener.local_addr().expect("an address").port();
                drop(listener);
                format!("http://127.0.0.1:{port}/refuse/{number}")
            }
        }
    }

    async fn received(&self) -> Vec<Received> {
        self.served
            .received
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

/// One request as the server read it.
struct Request {
    method: String,
    path: String,
    headers: HashMap<String, String>,
    body: String,
}

async fn read_request(stream: &mut TcpStream) -> Option<Request> {
    let mut bytes = Vec::new();
    let mut chunk = [0u8; 4096];
    let head_end = loop {
        if let Some(end) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
            break end;
        }
        let read = stream.read(&mut chunk).await.ok()?;
        if read == 0 || bytes.len() > MAX_REQUEST {
            return None;
        }
        bytes.extend_from_slice(chunk.get(..read)?);
    };
    let head = String::from_utf8(bytes.get(..head_end)?.to_vec()).ok()?;
    let mut lines = head.split("\r\n");
    let mut first = lines.next()?.split(' ');
    let method = first.next()?.to_owned();
    let path = first.next()?.to_owned();
    let headers: HashMap<String, String> = lines
        .filter_map(|line| line.split_once(':'))
        .map(|(name, value)| (name.trim().to_ascii_lowercase(), value.trim().to_owned()))
        .collect();
    let length: usize = headers
        .get("content-length")
        .and_then(|length| length.parse().ok())
        .unwrap_or(0);
    let mut body = bytes.get(head_end + 4..)?.to_vec();
    while body.len() < length.min(MAX_REQUEST) {
        let read = stream.read(&mut chunk).await.ok()?;
        if read == 0 {
            break;
        }
        body.extend_from_slice(chunk.get(..read)?);
    }
    Some(Request {
        method,
        path,
        headers,
        body: String::from_utf8(body).ok()?,
    })
}

async fn respond(stream: &mut TcpStream, status: u16) {
    let extra = if (300..400).contains(&status) {
        "Location: http://127.0.0.1:9/elsewhere\r\n"
    } else {
        ""
    };
    let reply =
        format!("HTTP/1.1 {status} Demo\r\n{extra}Content-Length: 0\r\nConnection: close\r\n\r\n");
    let _ = stream.write_all(reply.as_bytes()).await;
    let _ = stream.shutdown().await;
}

fn auth_of(headers: &HashMap<String, String>) -> Option<ReceivedAuth> {
    let value = headers.get("authorization")?;
    if let Some(token) = value.strip_prefix("Bearer ") {
        return Some(ReceivedAuth::Bearer(token.to_owned()));
    }
    let encoded = value.strip_prefix("Basic ")?;
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .ok()?;
    let (username, password) = std::str::from_utf8(&decoded).ok()?.split_once(':')?;
    Some(ReceivedAuth::Basic {
        username: username.to_owned(),
        password: password.to_owned(),
    })
}

async fn answer(mut stream: TcpStream, served: Arc<Served>) {
    let Some(request) = read_request(&mut stream).await else {
        return;
    };
    let mut parts = request.path.trim_start_matches('/').split('/');
    match parts.next() {
        Some("hang") => {
            // Holds the connection open past any delivery's limit.
            tokio::time::sleep(Duration::from_secs(60)).await;
        }
        Some("status") => {
            let status = parts
                .next()
                .and_then(|status| status.parse().ok())
                .unwrap_or(500);
            respond(&mut stream, status).await;
        }
        Some("accept") => {
            let number: Option<usize> = parts.next().and_then(|number| number.parse().ok());
            let kind = number.and_then(|number| {
                served
                    .kinds
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .get(&number)
                    .copied()
            });
            let Some(kind) = kind.filter(|_| request.method == "POST") else {
                respond(&mut stream, 400).await;
                return;
            };
            let received = match kind {
                ChannelKind::Webhook => {
                    let body: serde_json::Value =
                        serde_json::from_str(&request.body).unwrap_or_default();
                    let text = |key: &str| body[key].as_str().unwrap_or_default().to_owned();
                    Received {
                        kind,
                        ping: None,
                        event: text("event"),
                        message: text("message"),
                        auth: auth_of(&request.headers),
                    }
                }
                ChannelKind::Ntfy => Received {
                    kind,
                    ping: None,
                    event: request.headers.get("title").cloned().unwrap_or_default(),
                    message: request.body.clone(),
                    auth: auth_of(&request.headers),
                },
                ChannelKind::Healthchecks => Received {
                    kind,
                    ping: Some(match parts.next() {
                        Some("start") => Ping::Start,
                        Some("fail") => Ping::Fail,
                        _ => Ping::Success,
                    }),
                    event: String::new(),
                    message: request.body.clone(),
                    auth: auth_of(&request.headers),
                },
            };
            served
                .received
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(received);
            respond(&mut stream, 200).await;
        }
        _ => respond(&mut stream, 404).await,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_http_notifier_meets_the_notifier_contract() {
    let harness = LoopbackHarness::start().await;
    app::contract::notifier::run(&harness).await;
}
