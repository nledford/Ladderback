//! The Full Disk Access routes with sign-in off; `api_tokens/tests.rs` checks
//! that no API token reaches them.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::atomic::{AtomicUsize, Ordering};

use app::fakes::FakeFullDiskAccessProbe;
use axum::body::Body;
use axum::http::{Method, Request, header};
use domain::execution::full_disk_access::FullDiskAccess;
use http_body_util::BodyExt;
use tower::ServiceExt;

use super::*;
use crate::request_checks::tests::{NoUi, test_state};
use crate::{PeerAddr, RequestChecks, router};

/// The agent's own Mac, as its loopback connection shows it.
const SAME_MAC: SocketAddr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 51000);
/// Another computer on the network.
const ELSEWHERE: SocketAddr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 0, 2, 7)), 51000);
/// A configured host name, as a proxy forwards requests by.
const PROXIED: &str = "backup.example.lan";

/// Opens the pane as it was told to, or fails with the platform's words, and
/// counts how often it was asked.
struct Opener {
    fails: Option<&'static str>,
    asked: AtomicUsize,
}

impl Opener {
    fn opening() -> Arc<Self> {
        Arc::new(Self {
            fails: None,
            asked: AtomicUsize::new(0),
        })
    }

    fn failing(detail: &'static str) -> Arc<Self> {
        Arc::new(Self {
            fails: Some(detail),
            asked: AtomicUsize::new(0),
        })
    }

    fn asked(&self) -> usize {
        self.asked.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl SettingsOpener for Opener {
    async fn open(&self) -> Result<(), SettingsUnopened> {
        self.asked.fetch_add(1, Ordering::SeqCst);
        match self.fails {
            None => Ok(()),
            Some(detail) => Err(SettingsUnopened {
                detail: detail.to_owned(),
            }),
        }
    }
}

/// An agent on macOS whose probe finds `found`, already run once, as the
/// agent runs it before it serves.
async fn on_macos(
    found: FullDiskAccess,
    opener: Arc<Opener>,
) -> (FullDiskAccessRoute, Arc<FakeFullDiskAccessProbe>) {
    let probe = Arc::new(FakeFullDiskAccessProbe::new(found));
    let check = Arc::new(FullDiskAccessCheck::new(
        Arc::clone(&probe) as Arc<dyn app::ports::FullDiskAccessProbe>
    ));
    check.at_start().await;
    (
        FullDiskAccessRoute::new(check, opener as Arc<dyn SettingsOpener>),
        probe,
    )
}

/// A request from `peer`, naming `host`, as the listener gives every request
/// its connection's address; a change is JSON, as the page sends it.
fn request(method: Method, path: &str, peer: SocketAddr, host: &str) -> Request<Body> {
    let mut request = Request::builder()
        .method(method)
        .uri(path)
        .header(header::HOST, host)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from("{}"))
        .unwrap();
    request
        .extensions_mut()
        .insert(axum::extract::ConnectInfo(PeerAddr(peer)));
    request
}

fn get(peer: SocketAddr, host: &str) -> Request<Body> {
    request(Method::GET, FULL_DISK_ACCESS_PATH, peer, host)
}

fn settings(peer: SocketAddr, host: &str) -> Request<Body> {
    request(Method::POST, SETTINGS_PATH, peer, host)
}

async fn send(
    route: &FullDiskAccessRoute,
    checks: RequestChecks,
    request: Request<Body>,
) -> (StatusCode, serde_json::Value) {
    let mut state = test_state();
    state.full_disk_access = route.clone();
    let response = router(Arc::new(NoUi), checks, state)
        .oneshot(request)
        .await
        .unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (status, serde_json::from_slice(&bytes).unwrap_or_default())
}

#[tokio::test]
async fn off_macos_the_grant_is_not_applicable_and_nothing_opens() {
    let route = FullDiskAccessRoute::default();
    let (status, body) = send(
        &route,
        RequestChecks::default(),
        get(SAME_MAC, "127.0.0.1:8080"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body,
        serde_json::json!({ "state": "not-applicable", "opens-settings": false })
    );
    let (status, body) = send(
        &route,
        RequestChecks::default(),
        request(Method::POST, CHECK_PATH, SAME_MAC, "127.0.0.1:8080"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["state"], "not-applicable");
}

#[tokio::test]
async fn the_read_answers_what_the_last_probe_found() {
    for (found, state) in [
        (FullDiskAccess::Granted, "granted"),
        (FullDiskAccess::Missing, "missing"),
        (FullDiskAccess::Unknown, "unknown"),
    ] {
        let (route, probe) = on_macos(found, Opener::opening()).await;
        let (status, body) = send(
            &route,
            RequestChecks::default(),
            get(SAME_MAC, "127.0.0.1:8080"),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{found:?}");
        assert_eq!(
            body,
            serde_json::json!({ "state": state, "opens-settings": true }),
            "{found:?}"
        );
        // A read runs no probe: the last one's answer stands.
        assert_eq!(probe.asked(), 1, "{found:?}");
    }
}

#[tokio::test]
async fn only_a_browser_on_the_agent_s_own_mac_may_open_the_settings() {
    let (route, _) = on_macos(FullDiskAccess::Missing, Opener::opening()).await;
    let same_mac_v6 = SocketAddr::new(IpAddr::V6(Ipv6Addr::LOCALHOST), 51000);
    let mapped = SocketAddr::new(IpAddr::V6(Ipv4Addr::LOCALHOST.to_ipv6_mapped()), 51000);
    for (peer, host, opens) in [
        (SAME_MAC, "127.0.0.1:8080", true),
        (SAME_MAC, "localhost:8080", true),
        (same_mac_v6, "[::1]:8080", true),
        (mapped, "127.0.0.1:8080", true),
        (ELSEWHERE, "127.0.0.1:8080", false),
        // A proxy on the same Mac forwards for whoever reached it.
        (SAME_MAC, PROXIED, false),
    ] {
        let (status, body) = send(
            &route,
            RequestChecks::new([PROXIED.to_owned()]),
            get(peer, host),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{peer} {host}");
        assert_eq!(body["opens-settings"], opens, "{peer} {host}");
    }
}

#[tokio::test]
async fn checking_again_runs_the_probe_and_answers_what_it_found() {
    let (route, probe) = on_macos(FullDiskAccess::Missing, Opener::opening()).await;
    probe.set(FullDiskAccess::Granted);
    let (status, body) = send(
        &route,
        RequestChecks::default(),
        request(Method::POST, CHECK_PATH, ELSEWHERE, "127.0.0.1:8080"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body,
        serde_json::json!({ "state": "granted", "opens-settings": false })
    );
    assert_eq!(probe.asked(), 2);
    let (_, body) = send(
        &route,
        RequestChecks::default(),
        get(SAME_MAC, "127.0.0.1:8080"),
    )
    .await;
    assert_eq!(body["state"], "granted");
}

#[tokio::test]
async fn the_settings_open_for_a_browser_on_the_same_mac() {
    let opener = Opener::opening();
    let (route, _) = on_macos(FullDiskAccess::Missing, Arc::clone(&opener)).await;
    let (status, _) = send(
        &route,
        RequestChecks::default(),
        settings(SAME_MAC, "localhost:8080"),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(opener.asked(), 1);
}

#[tokio::test]
async fn the_settings_never_open_for_another_computer() {
    let opener = Opener::opening();
    let (route, _) = on_macos(FullDiskAccess::Missing, Arc::clone(&opener)).await;
    for (peer, host) in [(ELSEWHERE, "127.0.0.1:8080"), (SAME_MAC, PROXIED)] {
        let (status, body) = send(
            &route,
            RequestChecks::new([PROXIED.to_owned()]),
            settings(peer, host),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{peer} {host}");
        assert_eq!(body["code"], "full-disk-access-not-local", "{peer} {host}");
    }
    assert_eq!(opener.asked(), 0);
}

#[tokio::test]
async fn a_request_without_its_connection_s_address_is_not_from_the_same_mac() {
    let opener = Opener::opening();
    let (route, _) = on_macos(FullDiskAccess::Missing, Arc::clone(&opener)).await;
    let mut request = settings(SAME_MAC, "127.0.0.1:8080");
    request
        .extensions_mut()
        .remove::<axum::extract::ConnectInfo<PeerAddr>>();
    let (status, body) = send(&route, RequestChecks::default(), request).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["code"], "full-disk-access-not-local");
    assert_eq!(opener.asked(), 0);
}

#[tokio::test]
async fn off_macos_the_settings_are_refused_as_unopened() {
    let (status, body) = send(
        &FullDiskAccessRoute::default(),
        RequestChecks::default(),
        settings(SAME_MAC, "127.0.0.1:8080"),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["code"], "full-disk-access-settings-unopened");
}

#[tokio::test]
async fn settings_macos_refused_to_open_are_an_error_without_its_words() {
    let opener = Opener::failing("LSOpenURLsWithRole() failed with error -10814");
    let (route, _) = on_macos(FullDiskAccess::Missing, Arc::clone(&opener)).await;
    let (status, body) = send(
        &route,
        RequestChecks::default(),
        settings(SAME_MAC, "127.0.0.1:8080"),
    )
    .await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(body["code"], "full-disk-access-settings-unopened");
    // The platform's words stay in the daemon's log.
    assert!(!body.to_string().contains("LSOpenURLsWithRole"), "{body}");
    assert_eq!(opener.asked(), 1);
}
