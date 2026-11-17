use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::{Arc, Mutex, PoisonError};

use axum::http::{Method, Request};
use domain::restore::download::TICKET_BYTES;
use futures::stream;
use http_body_util::BodyExt;
use tower::ServiceExt;

use super::*;
use crate::request_checks::tests::{NoUi, test_state};
use crate::{RequestChecks, router};

/// The snapshot the requests name: 64 hex digits.
fn snapshot() -> String {
    "3f2a1b4c".repeat(8)
}

fn ticket() -> DownloadTicket {
    DownloadTicket::from_bytes([0xab; TICKET_BYTES])
}

const HOME: SocketAddr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 0, 2, 7)), 51000);

/// What the scripted daemon hands over under its ticket.
enum Handing {
    File(&'static [&'static [u8]]),
    Archive(ArchiveFormat, &'static [&'static [u8]]),
    /// One piece, then a failure.
    Broken,
}

/// Answers each route with its scripted answer, and remembers what it was
/// asked, the confirmation included.
struct Scripted {
    start: Result<DownloadTicket, DownloadRefusal>,
    name: &'static str,
    handing: Mutex<Option<Handing>>,
    asked: Mutex<Vec<String>>,
}

impl Scripted {
    fn answering(start: Result<DownloadTicket, DownloadRefusal>) -> Arc<Self> {
        Arc::new(Self {
            start,
            name: "notes.md",
            handing: Mutex::new(None),
            asked: Mutex::new(Vec::new()),
        })
    }

    fn handing(name: &'static str, handing: Handing) -> Arc<Self> {
        Arc::new(Self {
            start: Err(DownloadRefusal::Busy),
            name,
            handing: Mutex::new(Some(handing)),
            asked: Mutex::new(Vec::new()),
        })
    }

    fn asked(&self) -> Vec<String> {
        self.asked
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

fn pieces(pieces: &'static [&'static [u8]]) -> BoxStream<'static, Result<Vec<u8>, FailureReason>> {
    stream::iter(pieces.iter().map(|piece| Ok(piece.to_vec()))).boxed()
}

#[async_trait]
impl Downloads for Scripted {
    async fn start(
        &self,
        alias: String,
        start: DownloadStart,
    ) -> Result<DownloadTicket, DownloadRefusal> {
        let DownloadStart {
            snapshot,
            entry,
            format,
            address,
            confirmation,
        } = start;
        self.asked
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(format!(
                "{alias}:{}:{entry}:{format:?}:{address}:{}",
                snapshot.as_str(),
                confirmation.as_password().expose()
            ));
        self.start.clone()
    }

    async fn fetch(&self, ticket: DownloadTicket) -> Option<Handover> {
        self.asked
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(ticket.encode());
        if ticket != self::ticket() {
            return None;
        }
        let handing = self
            .handing
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()?;
        let (shape, pieces) = match handing {
            Handing::File(given) => (
                DownloadShape::File {
                    size: given
                        .iter()
                        .map(|piece| u64::try_from(piece.len()).unwrap())
                        .sum(),
                },
                pieces(given),
            ),
            Handing::Archive(format, given) => (DownloadShape::Archive(format), pieces(given)),
            Handing::Broken => (
                DownloadShape::Archive(ArchiveFormat::Tar),
                stream::iter([
                    Ok(b"first".to_vec()),
                    Err(FailureReason::EngineFailed {
                        detail: "the worker went away".to_owned(),
                    }),
                ])
                .boxed(),
            ),
        };
        Some(Handover {
            file_name: self.name.to_owned(),
            shape,
            pieces,
        })
    }
}

/// A request from `HOME`, as the listener gives every request its
/// connection's address.
fn request(method: Method, path: &str, body: &str) -> Request<Body> {
    let mut request = Request::builder()
        .method(method)
        .uri(path)
        .header(header::HOST, "127.0.0.1:8080")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_owned()))
        .unwrap();
    request
        .extensions_mut()
        .insert(axum::extract::ConnectInfo(crate::PeerAddr(HOME)));
    request
}

fn start_path(snapshot: &str) -> String {
    format!("/api/v1/repositories/demo/snapshots/{snapshot}/download")
}

const NOTES: &str =
    r#"{"path":"home/docs/notes.md","format":"zip","confirmation":"canary-admin-password"}"#;

async fn send(downloads: Arc<Scripted>, request: Request<Body>) -> Response {
    let mut state = test_state();
    state.downloads = downloads;
    router(Arc::new(NoUi), RequestChecks::default(), state)
        .oneshot(request)
        .await
        .unwrap()
}

async fn json_of(response: Response) -> serde_json::Value {
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

fn header_of(response: &Response, name: header::HeaderName) -> Option<&str> {
    response
        .headers()
        .get(name)
        .map(|value| value.to_str().unwrap())
}

#[tokio::test]
async fn a_start_reads_the_request_and_answers_the_ticket() {
    let downloads = Scripted::answering(Ok(ticket()));
    let response = send(
        downloads.clone(),
        request(Method::POST, &start_path(&snapshot()), NOTES),
    )
    .await;
    assert_eq!(response.status(), StatusCode::CREATED);
    assert_eq!(
        json_of(response).await,
        serde_json::json!({ "ticket": "ab".repeat(TICKET_BYTES) })
    );
    assert_eq!(
        downloads.asked(),
        [format!(
            "demo:{}:home/docs/notes.md:Zip:192.0.2.7:canary-admin-password",
            snapshot()
        )]
    );
}

#[tokio::test]
async fn each_refusal_of_a_start_has_its_status_and_code() {
    for (refusal, status, code) in [
        (
            DownloadRefusal::StepUpFailed,
            StatusCode::FORBIDDEN,
            "step-up-failed",
        ),
        (
            DownloadRefusal::StepUpUnavailable("the keyring is locked".to_owned()),
            StatusCode::SERVICE_UNAVAILABLE,
            "sign-in-unavailable",
        ),
        (
            DownloadRefusal::TicketUnavailable("no random bytes".to_owned()),
            StatusCode::SERVICE_UNAVAILABLE,
            "downloads-unavailable",
        ),
        (
            DownloadRefusal::Busy,
            StatusCode::CONFLICT,
            "downloads-busy",
        ),
        (
            DownloadRefusal::SnapshotUnknown,
            StatusCode::NOT_FOUND,
            "snapshot-unknown",
        ),
        (
            DownloadRefusal::Repository(MaintenanceRefusal::UnknownRepository),
            StatusCode::NOT_FOUND,
            "repository-unknown",
        ),
    ] {
        let response = send(
            Scripted::answering(Err(refusal.clone())),
            request(Method::POST, &start_path(&snapshot()), NOTES),
        )
        .await;
        assert_eq!(response.status(), status, "{refusal:?}");
        assert_eq!(json_of(response).await["code"], code, "{refusal:?}");
    }
}

#[tokio::test]
async fn a_start_during_a_sign_in_wait_says_how_long_is_left() {
    let response = send(
        Scripted::answering(Err(DownloadRefusal::StepUpWait { seconds: 7 })),
        request(Method::POST, &start_path(&snapshot()), NOTES),
    )
    .await;
    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(header_of(&response, header::RETRY_AFTER), Some("7"));
}

#[tokio::test]
async fn an_entry_the_snapshot_does_not_hold_is_named_as_the_request_named_it() {
    let response = send(
        Scripted::answering(Err(DownloadRefusal::EntryUnknown)),
        request(Method::POST, &start_path(&snapshot()), NOTES),
    )
    .await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let body = json_of(response).await;
    assert_eq!(body["code"], "entry-unknown");
    assert_eq!(body["args"]["path"]["value"], "home/docs/notes.md");
}

#[tokio::test]
async fn a_start_that_cannot_be_read_asks_the_daemon_nothing() {
    for (path, body, status, code) in [
        (
            start_path(&snapshot()),
            r#"{"path":"","format":"rar","confirmation":"x"}"#,
            StatusCode::BAD_REQUEST,
            "invalid-body",
        ),
        (
            start_path(&snapshot()),
            r#"{"path":"","format":"tar","confirmation":"x","extra":1}"#,
            StatusCode::BAD_REQUEST,
            "invalid-body",
        ),
        (
            start_path("3f2a1b4c"),
            NOTES,
            StatusCode::NOT_FOUND,
            "snapshot-unknown",
        ),
        (
            start_path(&snapshot()),
            r#"{"path":"home//docs","format":"tar","confirmation":"x"}"#,
            StatusCode::NOT_FOUND,
            "entry-unknown",
        ),
    ] {
        let downloads = Scripted::answering(Ok(ticket()));
        let response = send(downloads.clone(), request(Method::POST, &path, body)).await;
        assert_eq!(response.status(), status, "{body}");
        assert_eq!(json_of(response).await["code"], code, "{body}");
        assert!(downloads.asked().is_empty(), "{body}");
    }
}

#[tokio::test]
async fn a_confirmation_of_nothing_or_longer_than_any_password_fails_unasked() {
    let longest = domain::access::PASSWORD_MAX_BYTES;
    // The last is the snapshot's short ID, padded with spaces.
    for confirmation in [
        String::new(),
        "a".repeat(longest + 1),
        format!("{}3f2a1b4c", " ".repeat(longest)),
    ] {
        let body = serde_json::json!({
            "path": "home/docs/notes.md",
            "format": "zip",
            "confirmation": confirmation,
        })
        .to_string();
        let downloads = Scripted::answering(Ok(ticket()));
        let response = send(
            downloads.clone(),
            request(Method::POST, &start_path(&snapshot()), &body),
        )
        .await;
        let length = confirmation.len();
        assert_eq!(response.status(), StatusCode::FORBIDDEN, "{length} bytes");
        assert_eq!(
            json_of(response).await["code"],
            "step-up-failed",
            "{length} bytes"
        );
        assert!(downloads.asked().is_empty(), "{length} bytes");
    }
}

fn fetch_path(ticket: &str) -> String {
    format!("/api/v1/downloads/{ticket}")
}

#[tokio::test]
async fn a_file_is_handed_over_as_an_attachment_of_its_size() {
    let downloads = Scripted::handing("notes.md", Handing::File(&[b"hello ", b"world"]));
    let response = send(
        downloads,
        request(Method::GET, &fetch_path(&ticket().encode()), ""),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        header_of(&response, header::CONTENT_TYPE),
        Some("application/octet-stream")
    );
    assert_eq!(header_of(&response, header::CONTENT_LENGTH), Some("11"));
    assert_eq!(
        header_of(&response, header::CONTENT_DISPOSITION),
        Some("attachment; filename=\"notes.md\"; filename*=UTF-8''notes.md")
    );
    assert_eq!(
        header_of(&response, header::CACHE_CONTROL),
        Some("no-store")
    );
    let body = response.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(&body[..], b"hello world");
}

/// A body of length 0 is never read, so the download's end would never be
/// recorded: an empty file goes with no length, as an archive does.
#[tokio::test]
async fn an_empty_file_goes_with_no_size_announced() {
    let downloads = Scripted::handing("empty.txt", Handing::File(&[]));
    let response = send(
        downloads,
        request(Method::GET, &fetch_path(&ticket().encode()), ""),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(header_of(&response, header::CONTENT_LENGTH), None);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    assert!(body.is_empty());
}

#[tokio::test]
async fn an_archive_goes_as_its_format_with_no_size_announced() {
    for (format, content_type) in [
        (ArchiveFormat::Tar, "application/x-tar"),
        (ArchiveFormat::Zip, "application/zip"),
    ] {
        let downloads = Scripted::handing("docs", Handing::Archive(format, &[b"archive"]));
        let response = send(
            downloads,
            request(Method::GET, &fetch_path(&ticket().encode()), ""),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK, "{format:?}");
        assert_eq!(
            header_of(&response, header::CONTENT_TYPE),
            Some(content_type)
        );
        assert_eq!(header_of(&response, header::CONTENT_LENGTH), None);
    }
}

#[tokio::test]
async fn a_download_the_engine_cannot_finish_ends_the_body_short() {
    let downloads = Scripted::handing("docs.tar", Handing::Broken);
    let response = send(
        downloads,
        request(Method::GET, &fetch_path(&ticket().encode()), ""),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert!(response.into_body().collect().await.is_err());
}

#[tokio::test]
async fn a_ticket_waiting_for_nothing_is_a_download_unknown() {
    for asked in ["cd".repeat(TICKET_BYTES), "not-a-ticket".to_owned()] {
        let downloads = Scripted::handing("notes.md", Handing::File(&[b"x"]));
        let response = send(downloads, request(Method::GET, &fetch_path(&asked), "")).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{asked}");
        assert_eq!(json_of(response).await["code"], "download-unknown");
    }
}

#[tokio::test]
async fn a_ticket_is_handed_over_once() {
    let downloads = Scripted::handing("notes.md", Handing::File(&[b"x"]));
    let path = fetch_path(&ticket().encode());
    let first = send(downloads.clone(), request(Method::GET, &path, "")).await;
    assert_eq!(first.status(), StatusCode::OK);
    let second = send(downloads, request(Method::GET, &path, "")).await;
    assert_eq!(second.status(), StatusCode::NOT_FOUND);
}

#[test]
fn a_name_beyond_ascii_is_saved_as_itself_where_the_browser_reads_filename_star() {
    assert_eq!(
        content_disposition("Résumé \"final\".pdf"),
        "attachment; filename=\"R_sum_ _final_.pdf\"; filename*=UTF-8''R%C3%A9sum%C3%A9%20%22final%22.pdf"
    );
    assert_eq!(
        content_disposition("a\\b;c%d.txt"),
        "attachment; filename=\"a_b;c%d.txt\"; filename*=UTF-8''a%5Cb%3Bc%25d.txt"
    );
}
