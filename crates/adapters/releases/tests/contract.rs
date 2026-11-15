//! The GitHub release feed meets the `ReleaseFeed` contract against a feed
//! served on loopback, never the network: one small HTTP server answers each
//! request as the path it was sent to says, and records what it was sent.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use app::contract::release_feed::{Answer, ReleaseFeedHarness};
use app::ports::{ReleaseFeed, ReleaseFeedError};
use domain::updates::{Release, Version};
use releases_adapter::{ClientError, GitHubReleases, LATEST_RELEASE};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

/// How long a request may take here: long enough for loopback, short enough
/// that the check of a feed that never answers is quick.
const TIMEOUT: Duration = Duration::from_millis(500);

/// The most of a request the server reads.
const MAX_REQUEST: usize = 64 * 1024;

/// The most of an answer the reader reads, as security §10.7 documents it:
/// 1 MiB, written out here so that a change to the reader's cap fails.
const DOCUMENTED_CAP: usize = 1024 * 1024;

/// One request as the server read it.
#[derive(Clone, Debug)]
struct Received {
    method: String,
    path: String,
    headers: BTreeMap<String, String>,
}

struct LoopbackFeeds {
    port: u16,
    received: Arc<Mutex<Vec<Received>>>,
}

impl LoopbackFeeds {
    async fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("a port");
        let port = listener.local_addr().expect("an address").port();
        let received = Arc::new(Mutex::new(Vec::new()));
        let recording = received.clone();
        tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                tokio::spawn(answer(stream, port, recording.clone()));
            }
        });
        Self { port, received }
    }

    fn url(&self, path: &str) -> String {
        format!("http://127.0.0.1:{}{path}", self.port)
    }

    fn reader(&self, url: &str) -> GitHubReleases {
        GitHubReleases::new(url, &version("0.2.0"), TIMEOUT).expect("a client")
    }

    fn received(&self) -> Vec<Received> {
        self.received
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

impl ReleaseFeedHarness for LoopbackFeeds {
    fn feed(&self, answer: Answer) -> Arc<dyn ReleaseFeed> {
        let url = match answer {
            Answer::Release { tag, prerelease } => {
                self.url(&format!("/release/{tag}/{prerelease}"))
            }
            Answer::Status(status) => self.url(&format!("/status/{status}")),
            Answer::Garbage => self.url("/garbage"),
            Answer::Oversized => self.url("/oversized"),
            Answer::Hangs => self.url("/hang"),
            Answer::Refuses => {
                // A port that was free a moment ago, and that nothing listens at.
                let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("a port");
                let port = listener.local_addr().expect("an address").port();
                drop(listener);
                format!("http://127.0.0.1:{port}/release/v0.3.0/false")
            }
        };
        Arc::new(self.reader(&url))
    }
}

fn version(text: &str) -> Version {
    Version::parse(text).unwrap_or_else(|_| panic!("{text:?} is a version"))
}

async fn read_request(stream: &mut TcpStream) -> Option<Received> {
    let mut bytes = Vec::new();
    let mut chunk = [0u8; 4096];
    while !bytes.windows(4).any(|window| window == b"\r\n\r\n") {
        let read = stream.read(&mut chunk).await.ok()?;
        if read == 0 || bytes.len() > MAX_REQUEST {
            return None;
        }
        bytes.extend_from_slice(&chunk[..read]);
    }
    let text = String::from_utf8_lossy(&bytes).into_owned();
    let mut lines = text.split("\r\n");
    let mut first = lines.next()?.split(' ');
    let (method, path) = (first.next()?.to_owned(), first.next()?.to_owned());
    let headers = lines
        .take_while(|line| !line.is_empty())
        .filter_map(|line| line.split_once(':'))
        .map(|(name, value)| (name.trim().to_ascii_lowercase(), value.trim().to_owned()))
        .collect();
    Some(Received {
        method,
        path,
        headers,
    })
}

/// The newest release as GitHub tells it, with the fields the reader must
/// leave alone: a name, notes, and a page elsewhere.
fn release_json(tag: &str, prerelease: bool, draft: bool, notes: &str) -> String {
    serde_json::json!({
        "tag_name": tag,
        "name": "A name that is not read",
        "prerelease": prerelease,
        "draft": draft,
        "html_url": "https://example.com/not-the-notes",
        "body": notes,
    })
    .to_string()
}

async fn answer(mut stream: TcpStream, port: u16, received: Arc<Mutex<Vec<Received>>>) {
    let Some(request) = read_request(&mut stream).await else {
        return;
    };
    received
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .push(request.clone());
    let parts: Vec<&str> = request.path.split('/').skip(1).collect();
    let (status, body) = match parts.as_slice() {
        ["release", tag, kind] => {
            let (prerelease, draft) = (*kind == "true", *kind == "draft");
            (200, release_json(tag, prerelease, draft, "Notes."))
        }
        ["status", status] => (status.parse().expect("a status"), String::new()),
        ["garbage"] => (200, "<html>not the feed's JSON</html>".to_owned()),
        ["oversized"] => (
            200,
            release_json("v0.3.0", false, false, &"a".repeat(DOCUMENTED_CAP)),
        ),
        ["at-cap"] => {
            let mut body = release_json("v0.3.0", false, false, "");
            let notes = "a".repeat(DOCUMENTED_CAP - body.len());
            body = release_json("v0.3.0", false, false, &notes);
            assert_eq!(body.len(), DOCUMENTED_CAP, "the answer fills the cap");
            (200, body)
        }
        ["hang"] => {
            tokio::time::sleep(Duration::from_secs(60)).await;
            return;
        }
        _ => (404, String::new()),
    };
    // A redirect points at a release the reader would find, were it to
    // follow it.
    let head = format!(
        "HTTP/1.1 {status} Answer\r\nContent-Type: application/json\r\nContent-Length: {}\r\nLocation: http://127.0.0.1:{port}/release/v9.0.0/false\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(head.as_bytes()).await;
    let _ = stream.write_all(body.as_bytes()).await;
    let _ = stream.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn github_releases_meets_the_release_feed_contract() {
    let feeds = LoopbackFeeds::start().await;
    app::contract::release_feed::run(&feeds).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_draft_counts_as_a_prerelease() {
    let feeds = LoopbackFeeds::start().await;
    let read = feeds
        .reader(&feeds.url("/release/v0.3.0/draft"))
        .newest()
        .await;
    assert_eq!(
        read,
        Ok(Release {
            version: version("0.3.0"),
            prerelease: true
        })
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_answer_that_fills_the_cap_is_read() {
    let feeds = LoopbackFeeds::start().await;
    let read = feeds.reader(&feeds.url("/at-cap")).newest().await;
    assert_eq!(
        read,
        Ok(Release {
            version: version("0.3.0"),
            prerelease: false
        })
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_request_carries_only_what_the_docs_say() {
    let feeds = LoopbackFeeds::start().await;
    let read = feeds
        .reader(&feeds.url("/release/v0.3.0/false"))
        .newest()
        .await;
    assert!(read.is_ok(), "{read:?}");
    let received = feeds.received();
    let [request] = received.as_slice() else {
        panic!("one request, not {received:?}");
    };
    assert_eq!(request.method, "GET");
    assert_eq!(request.path, "/release/v0.3.0/false");
    let names: Vec<&str> = request.headers.keys().map(String::as_str).collect();
    assert_eq!(names, ["accept", "host", "user-agent"], "{request:?}");
    assert_eq!(request.headers["user-agent"], "ladderback/0.2.0");
    assert_eq!(request.headers["accept"], "application/vnd.github+json");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_redirect_is_not_followed() {
    let feeds = LoopbackFeeds::start().await;
    let read = feeds.reader(&feeds.url("/status/302")).newest().await;
    assert_eq!(read, Err(ReleaseFeedError::Rejected { status: 302 }));
    assert_eq!(feeds.received().len(), 1, "only the one request was sent");
}

#[test]
fn only_an_http_or_https_address_is_a_feed() {
    let running = version("0.2.0");
    for url in [
        "",
        "not a url",
        "ftp://example.com/latest",
        "file:///etc/hosts",
    ] {
        assert!(
            matches!(
                GitHubReleases::new(url, &running, TIMEOUT),
                Err(ClientError)
            ),
            "{url:?}"
        );
    }
    assert!(GitHubReleases::new(LATEST_RELEASE, &running, TIMEOUT).is_ok());
    assert!(GitHubReleases::new("http://127.0.0.1:9/", &running, TIMEOUT).is_ok());
}
