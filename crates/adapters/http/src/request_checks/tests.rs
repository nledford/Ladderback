use axum::body::Body;
use axum::http::{HeaderValue, Request, header};
use http_body_util::BodyExt;
use tower::ServiceExt;

use super::*;
use crate::{AppState, Asset, AssetSource, router};
use async_trait::async_trait;

/// A build without a web UI: every request that passes the checks reaches
/// the UI router and gets its answer, never a refusal's JSON.
pub(crate) struct NoUi;

impl AssetSource for NoUi {
    fn get(&self, _path: &str) -> Option<Asset> {
        None
    }
}

const NAME: &str = "nas.demo.test";

async fn send(request: Request<Body>) -> Response {
    let checks = RequestChecks::new([NAME.to_owned()]);
    router(Arc::new(NoUi), checks, test_state())
        .oneshot(request)
        .await
        .unwrap()
}

/// A state with the in-memory event bus: these tests only check the checks.
pub(crate) fn test_state() -> AppState {
    AppState {
        events: Arc::new(app::fakes::InMemoryEventBus::new()),
        operations: Arc::new(NoOperations),
        catalog: Arc::new(EmptyCatalog),
        repositories: Arc::new(NoRepositories),
        sign_in: crate::SignIn::Off,
    }
}

/// Does nothing: the checks never reach a route.
struct NoRepositories;

#[async_trait]
impl crate::Repositories for NoRepositories {
    async fn test(
        &self,
        _request: api::TestRepositoryDto,
    ) -> Result<api::RepositoryIdDto, crate::RepositoriesError> {
        Err(crate::RepositoriesError::InvalidBody)
    }

    async fn connect(
        &self,
        _request: api::ConnectRepositoryDto,
    ) -> Result<api::RepositoryIdDto, crate::RepositoriesError> {
        Err(crate::RepositoriesError::InvalidBody)
    }

    async fn init(
        &self,
        _request: api::InitRepositoryDto,
    ) -> Result<api::RepositoryInitDto, crate::RepositoriesError> {
        Err(crate::RepositoriesError::InvalidBody)
    }

    async fn create_saved(
        &self,
        _alias: String,
    ) -> Result<api::RepositoryIdDto, crate::RepositoriesError> {
        Err(crate::RepositoriesError::InvalidBody)
    }
}

/// Lists nothing: the checks never reach a route.
struct EmptyCatalog;

#[async_trait]
impl crate::Catalog for EmptyCatalog {
    async fn plans(&self) -> Result<api::PlansDto, crate::CatalogError> {
        Ok(api::PlansDto::default())
    }

    async fn repositories(&self) -> Result<api::RepositoriesDto, crate::CatalogError> {
        Ok(api::RepositoriesDto::default())
    }

    async fn snapshots(&self, _alias: String) -> Result<api::SnapshotsDto, crate::SnapshotsError> {
        Ok(api::SnapshotsDto::default())
    }
}

/// Starts nothing: the checks never reach a route.
struct NoOperations;

#[async_trait]
impl crate::Operations for NoOperations {
    async fn start_backup(&self, _plan: String) -> Result<String, crate::StartError> {
        Err(crate::StartError::UnknownPlan)
    }

    async fn start_maintenance(
        &self,
        _repository: String,
        _request: crate::MaintenanceRequest,
        _selection: Option<String>,
    ) -> Result<String, crate::MaintenanceRefusal> {
        Err(crate::MaintenanceRefusal::UnknownRepository)
    }

    async fn preview(
        &self,
        _repository: String,
        _request: crate::MaintenanceRequest,
    ) -> Result<api::MaintenancePreviewDto, crate::MaintenanceRefusal> {
        Err(crate::MaintenanceRefusal::UnknownRepository)
    }

    async fn groups(
        &self,
        _repository: String,
    ) -> Result<api::SnapshotGroupsDto, crate::MaintenanceRefusal> {
        Err(crate::MaintenanceRefusal::UnknownRepository)
    }

    async fn history(
        &self,
        _plan: Option<String>,
        _after: Option<String>,
        _limit: Option<u64>,
    ) -> Result<api::OperationsPageDto, crate::HistoryError> {
        Err(crate::HistoryError::BadCursor)
    }

    async fn operation(&self, _id: String) -> Result<api::OperationDetailDto, crate::LookupError> {
        Err(crate::LookupError::Unknown)
    }

    async fn cancel(&self, _id: String) -> Result<(), crate::CancelError> {
        Err(crate::CancelError::Unknown)
    }
}

pub(crate) fn request(method: &str, path: &str, headers: &[(&str, &str)]) -> Request<Body> {
    let mut builder = Request::builder().method(method).uri(path);
    for (name, value) in headers {
        builder = builder.header(*name, *value);
    }
    builder.body(Body::empty()).unwrap()
}

/// The error code of a refusal, or `None` for any other response.
pub(crate) async fn code(response: Response) -> Option<(StatusCode, String)> {
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    Some((status, json.get("code")?.as_str()?.to_owned()))
}

async fn assert_served(request: Request<Body>, what: &str) {
    let response = send(request).await;
    let status = response.status();
    assert!(
        ![StatusCode::FORBIDDEN, StatusCode::UNSUPPORTED_MEDIA_TYPE].contains(&status),
        "{what} was refused (status {status})"
    );
    let refused = code(response).await;
    assert_eq!(refused, None, "{what} was refused (status {status})");
}

/// A HEAD response has no body, so its refusal shows in the status and
/// the content type only.
async fn assert_refused(request: Request<Body>, status: StatusCode, code_name: &str, what: &str) {
    let head = request.method() == Method::HEAD;
    let response = send(request).await;
    if head {
        assert_eq!(response.status(), status, "{what} wasn't refused");
        assert_eq!(response.headers()[header::CONTENT_TYPE], "application/json");
        return;
    }
    let refused = code(response).await;
    assert_eq!(
        refused,
        Some((status, code_name.to_owned())),
        "{what} wasn't refused as expected"
    );
}

async fn assert_host_refused(request: Request<Body>, what: &str) {
    assert_refused(request, StatusCode::FORBIDDEN, "host-not-allowed", what).await;
}

async fn assert_origin_refused(request: Request<Body>, what: &str) {
    assert_refused(request, StatusCode::FORBIDDEN, "origin-not-allowed", what).await;
}

async fn assert_json_required(request: Request<Body>, what: &str) {
    assert_refused(
        request,
        StatusCode::UNSUPPORTED_MEDIA_TYPE,
        "json-required",
        what,
    )
    .await;
}

#[tokio::test]
async fn loopback_addresses_and_configured_names_are_served() {
    for host in [
        "127.0.0.1",
        "127.0.0.1:9898",
        "127.1.2.3:9898",
        "localhost",
        "localhost:9898",
        "LocalHost:9898",
        "[::1]",
        "[::1]:9898",
        "nas.demo.test",
        "nas.demo.test:443",
        "NAS.Demo.Test:9898",
    ] {
        assert_served(request("GET", "/", &[("host", host)]), host).await;
    }
}

#[tokio::test]
async fn any_other_host_is_refused() {
    for host in [
        "evil.example",
        "evil.example:9898",
        "nas.demo.test.evil.example",
        "evil.nas.demo.test",
        "localhost.evil.example",
        "localhost@evil.example",
        "evil.example@localhost",
        "localhost:9898@evil.example",
        "10.0.0.5:9898",
        "0.0.0.0:9898",
        "[::2]:9898",
        "[::]:9898",
        "[::ffff:127.0.0.1]:9898",
        "128.0.0.1",
        "127.1",
        "::1",
        "[::1",
        "[::1]x",
        "localhost:",
        "localhost:port",
        "localhost:+80",
        "localhost:99999",
        "localhost:80:80",
        "local host",
        "localhost/",
        "",
    ] {
        assert_host_refused(request("GET", "/", &[("host", host)]), host).await;
    }
}

#[tokio::test]
async fn the_host_is_checked_on_every_method_and_path() {
    for method in ["GET", "HEAD", "OPTIONS", "POST", "DELETE"] {
        for path in ["/", "/machines/demo", "/web-1a2b.js", "/api/v1/plans"] {
            assert_host_refused(
                request(
                    method,
                    path,
                    &[
                        ("host", "evil.example"),
                        ("content-type", "application/json"),
                    ],
                ),
                &format!("{method} {path}"),
            )
            .await;
        }
    }
}

#[tokio::test]
async fn a_request_without_a_host_is_refused() {
    assert_host_refused(request("GET", "/", &[]), "no Host").await;
}

#[tokio::test]
async fn two_host_headers_are_refused_even_when_both_are_allowed() {
    assert_host_refused(
        request(
            "GET",
            "/",
            &[("host", "localhost"), ("host", "nas.demo.test")],
        ),
        "two Host headers",
    )
    .await;
}

#[tokio::test]
async fn a_host_that_isnt_text_is_refused() {
    let mut request = request("GET", "/", &[]);
    request.headers_mut().insert(
        header::HOST,
        HeaderValue::from_bytes(b"local\xffhost").unwrap(),
    );
    assert_host_refused(request, "a Host with a byte outside ASCII").await;
}

#[tokio::test]
async fn without_a_host_header_the_request_target_names_the_host() {
    // HTTP/2 carries the host as `:authority`, which hyper puts in the URI.
    assert_served(request("GET", "http://localhost:9898/", &[]), "localhost").await;
    assert_served(request("GET", "http://nas.demo.test/", &[]), "a name").await;
    assert_host_refused(request("GET", "http://evil.example/", &[]), "evil.example").await;
}

#[tokio::test]
async fn a_refusal_is_json_with_the_error_code_and_nothing_else() {
    let response = send(request("GET", "/", &[("host", "evil.example")])).await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert_eq!(response.headers()[header::CONTENT_TYPE], "application/json");
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(bytes.as_ref(), br#"{"code":"host-not-allowed"}"#);
}

#[tokio::test]
async fn requests_without_an_origin_aren_t_subject_to_the_origin_rule() {
    // curl, scripts and the `ladderback` CLI send no Origin.
    for method in ["GET", "HEAD", "OPTIONS"] {
        assert_served(request(method, "/", &[("host", "localhost:9898")]), method).await;
    }
    assert_served(
        request(
            "POST",
            "/api/v1/plans",
            &[
                ("host", "localhost:9898"),
                ("content-type", "application/json"),
            ],
        ),
        "POST without Origin",
    )
    .await;
}

#[tokio::test]
async fn the_uis_own_origin_is_served() {
    for (host, origin) in [
        ("localhost:9898", "http://localhost:9898"),
        ("127.0.0.1:9898", "http://127.0.0.1:9898"),
        ("[::1]:9898", "http://[::1]:9898"),
        ("localhost", "http://localhost"),
        ("NAS.demo.test:9898", "http://nas.DEMO.test:9898"),
        // Behind a TLS proxy the browser's scheme is https.
        ("nas.demo.test", "https://nas.demo.test"),
    ] {
        assert_served(
            request(
                "POST",
                "/api/v1/plans",
                &[
                    ("host", host),
                    ("origin", origin),
                    ("content-type", "application/json"),
                ],
            ),
            origin,
        )
        .await;
        assert_served(
            request("GET", "/", &[("host", host), ("origin", origin)]),
            origin,
        )
        .await;
    }
}

#[tokio::test]
async fn any_other_origin_is_refused() {
    let host = "localhost:9898";
    for origin in [
        "http://evil.example",
        "https://evil.example:9898",
        "http://localhost:9899",
        "http://localhost",
        "http://127.0.0.1:9898",
        "http://nas.demo.test:9898",
        "http://localhost:9898.evil.example",
        "http://localhost:9898@evil.example",
        "http://evil.example@localhost:9898",
        "http://localhost:9898/",
        "http://localhost:9898/path",
        "ftp://localhost:9898",
        "file://localhost:9898",
        "localhost:9898",
        "//localhost:9898",
        "null",
        "",
    ] {
        assert_origin_refused(
            request(
                "POST",
                "/api/v1/plans",
                &[
                    ("host", host),
                    ("origin", origin),
                    ("content-type", "application/json"),
                ],
            ),
            origin,
        )
        .await;
    }
}

#[tokio::test]
async fn two_origin_headers_are_refused_even_when_both_match() {
    assert_origin_refused(
        request(
            "GET",
            "/",
            &[
                ("host", "localhost:9898"),
                ("origin", "http://localhost:9898"),
                ("origin", "http://localhost:9898"),
            ],
        ),
        "two Origin headers",
    )
    .await;
}

#[tokio::test]
async fn an_origin_that_isnt_text_is_refused() {
    let mut request = request("GET", "/", &[("host", "localhost:9898")]);
    request.headers_mut().insert(
        header::ORIGIN,
        HeaderValue::from_bytes(b"http://local\xffhost").unwrap(),
    );
    assert_origin_refused(request, "an Origin with a byte outside ASCII").await;
}

#[tokio::test]
async fn the_origin_of_reads_and_the_events_handshake_is_checked_too() {
    for method in ["GET", "HEAD", "OPTIONS"] {
        assert_origin_refused(
            request(
                method,
                "/",
                &[
                    ("host", "localhost:9898"),
                    ("origin", "http://evil.example"),
                ],
            ),
            method,
        )
        .await;
    }
    let handshake = |origin: Option<&'static str>| {
        let mut headers = vec![
            ("host", "localhost:9898"),
            ("connection", "Upgrade"),
            ("upgrade", "websocket"),
            ("sec-websocket-version", "13"),
            ("sec-websocket-key", "dGhlIHNhbXBsZSBub25jZQ=="),
        ];
        headers.extend(origin.map(|origin| ("origin", origin)));
        request("GET", "/api/v1/events", &headers)
    };
    assert_origin_refused(
        handshake(Some("http://evil.example")),
        "a foreign handshake",
    )
    .await;
    assert_served(
        handshake(Some("http://localhost:9898")),
        "an own-origin handshake",
    )
    .await;
    assert_served(handshake(None), "a handshake without Origin").await;
}

#[tokio::test]
async fn a_cross_origin_preflight_is_refused_without_cors_headers() {
    let response = send(request(
        "OPTIONS",
        "/api/v1/plans",
        &[
            ("host", "localhost:9898"),
            ("origin", "http://evil.example"),
            ("access-control-request-method", "POST"),
            ("access-control-request-headers", "content-type"),
        ],
    ))
    .await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert!(
        !response
            .headers()
            .keys()
            .any(|name| name.as_str().starts_with("access-control-")),
        "{:?}",
        response.headers()
    );
}

/// A request of the UI's own page: it carries the page's `Origin`.
const OWN: [(&str, &str); 2] = [
    ("host", "localhost:9898"),
    ("origin", "http://localhost:9898"),
];

fn from_the_page(method: &str, extra: &[(&str, &str)]) -> Request<Body> {
    let headers = [OWN.as_slice(), extra].concat();
    request(method, "/api/v1/plans", &headers)
}

/// Every method but the reads: the extension methods and `TRACE` mutate as far
/// as a browser is concerned, and a method's case is part of its name.
const MUTATING: [&str; 8] = [
    "POST", "PUT", "PATCH", "DELETE", "TRACE", "CONNECT", "PROPFIND", "post",
];

#[tokio::test]
async fn a_mutating_request_from_the_page_must_be_json() {
    for method in MUTATING {
        for content_type in [
            "application/json",
            "application/json; charset=utf-8",
            "application/json;charset=UTF-8",
            "Application/JSON",
            "application/json ; charset=utf-8",
        ] {
            assert_served(
                from_the_page(method, &[("content-type", content_type)]),
                &format!("{method} as {content_type}"),
            )
            .await;
        }
        for content_type in [
            "text/plain",
            "text/plain;charset=UTF-8",
            "application/x-www-form-urlencoded",
            "multipart/form-data; boundary=x",
            "application/octet-stream",
            "application/jsonp",
            "application/json-seq",
            "application/x-json",
            "application/vnd.api+json",
            "text/json",
            "application/json, text/plain",
            "json",
            "",
        ] {
            assert_json_required(
                from_the_page(method, &[("content-type", content_type)]),
                &format!("{method} as {content_type:?}"),
            )
            .await;
        }
        assert_json_required(
            from_the_page(method, &[]),
            &format!("{method} without a Content-Type"),
        )
        .await;
    }
}

#[tokio::test]
async fn two_content_types_are_refused_even_when_one_is_json() {
    assert_json_required(
        from_the_page(
            "POST",
            &[
                ("content-type", "application/json"),
                ("content-type", "text/plain"),
            ],
        ),
        "two Content-Type headers",
    )
    .await;
}

#[tokio::test]
async fn a_content_type_that_isnt_text_is_refused() {
    let mut request = from_the_page("POST", &[]);
    request.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_bytes(b"application/json\xff").unwrap(),
    );
    assert_json_required(request, "a Content-Type with a byte outside ASCII").await;
}

#[tokio::test]
async fn a_mutating_request_needs_json_even_without_an_origin() {
    // A browser or extension that strips Origin must not let a cross-site
    // form post through; curl and scripts send the header.
    for method in MUTATING {
        for content_type in [
            Some("text/plain"),
            Some("application/x-www-form-urlencoded"),
            Some("multipart/form-data; boundary=x"),
            Some(""),
            None,
        ] {
            let mut headers = vec![("host", "localhost:9898")];
            headers.extend(content_type.map(|value| ("content-type", value)));
            assert_json_required(
                request(method, "/api/v1/plans", &headers),
                &format!("{method} as {content_type:?} without Origin"),
            )
            .await;
        }
        for content_type in ["application/json", "application/json; charset=utf-8"] {
            assert_served(
                request(
                    method,
                    "/api/v1/plans",
                    &[("host", "localhost:9898"), ("content-type", content_type)],
                ),
                &format!("{method} as {content_type} without Origin"),
            )
            .await;
        }
    }
}

#[tokio::test]
async fn reads_need_no_content_type() {
    for method in ["GET", "HEAD", "OPTIONS"] {
        assert_served(
            from_the_page(method, &[("content-type", "text/plain")]),
            method,
        )
        .await;
        assert_served(from_the_page(method, &[]), method).await;
    }
}

#[tokio::test]
async fn the_checks_run_host_then_origin_then_content_type() {
    let all_wrong = [
        ("host", "evil.example"),
        ("origin", "http://evil.example"),
        ("content-type", "text/plain"),
    ];
    assert_host_refused(request("POST", "/", &all_wrong), "all three wrong").await;
    assert_origin_refused(
        request(
            "POST",
            "/",
            &[("host", "localhost"), all_wrong[1], all_wrong[2]],
        ),
        "Origin and Content-Type wrong",
    )
    .await;
    // A cross-site form post: the Origin check stops it before the type.
    assert_origin_refused(
        request(
            "POST",
            "/api/v1/plans",
            &[
                ("host", "localhost:9898"),
                ("origin", "http://evil.example"),
                ("content-type", "application/x-www-form-urlencoded"),
            ],
        ),
        "a cross-site form post",
    )
    .await;
}

#[tokio::test]
async fn the_agent_sync_routes_are_exempt() {
    // Agents authenticate with a signed nonce (§7.1), and reach the hub by
    // whatever name it has. They aren't browsers and send no Origin.
    for path in [
        "/sync/v1",
        "/sync/v1/",
        "/sync/v1/connect",
        "/sync/v1/a/b?x=1",
        "/sync/v1?x=1",
    ] {
        for method in ["GET", "POST"] {
            assert_served(
                request(
                    method,
                    path,
                    &[
                        ("host", "hub.other.example:8443"),
                        ("origin", "http://evil.example"),
                        ("content-type", "text/plain"),
                    ],
                ),
                &format!("{method} {path}"),
            )
            .await;
        }
    }
}

#[tokio::test]
async fn only_the_sync_route_itself_is_exempt() {
    for path in [
        "/sync/v10",
        "/sync/v1x/connect",
        "/sync/v2",
        "/sync",
        "/api/v1/sync/v1",
        "/x/sync/v1",
        "/Sync/v1",
        "/api/v1/plans?next=/sync/v1",
    ] {
        assert_host_refused(
            request("GET", path, &[("host", "hub.other.example:8443")]),
            path,
        )
        .await;
    }
}

fn addr(text: &str) -> SocketAddr {
    text.parse().unwrap()
}

#[test]
fn loopback_addresses_may_be_listened_on_with_sign_in_on_or_off() {
    for text in [
        "127.0.0.1:9898",
        "127.0.0.1:0",
        "127.1.2.3:9898",
        "[::1]:9898",
    ] {
        for on in [false, true] {
            assert_eq!(
                startup_rule(addr(text), on, &[]),
                Ok(Exposure::Loopback),
                "{text} {on}"
            );
        }
    }
}

const OTHER_ADDRESSES: [&str; 9] = [
    "0.0.0.0:9898",
    "0.0.0.0:0",
    "[::]:9898",
    "192.168.1.5:9898",
    "10.0.0.5:9898",
    "128.0.0.1:9898",
    "[fd00::1]:9898",
    "[::ffff:192.168.1.5]:9898",
    "[::ffff:127.0.0.1]:9898",
];

#[test]
fn other_addresses_may_not_be_listened_on_with_sign_in_off() {
    let hosts = ["nas.example.com".to_owned()];
    for text in OTHER_ADDRESSES {
        assert_eq!(
            startup_rule(addr(text), false, &hosts),
            Err(StartupRefusal::SignInOff(addr(text))),
            "{text}"
        );
    }
}

#[test]
fn other_addresses_need_sign_in_and_a_configured_host_name() {
    let hosts = ["nas.example.com".to_owned()];
    for text in OTHER_ADDRESSES {
        assert_eq!(
            startup_rule(addr(text), true, &[]),
            Err(StartupRefusal::NoAllowedHost(addr(text))),
            "{text}"
        );
        assert_eq!(
            startup_rule(addr(text), true, &hosts),
            Ok(Exposure::Network),
            "{text}"
        );
    }
}

#[test]
fn a_name_or_an_address_without_a_port_may_be_configured() {
    for name in [
        "nas.example.com",
        "NAS.Example.com",
        "nas_1.local",
        "localhost",
        "192.168.1.5",
        "[fd00::1]",
    ] {
        assert_eq!(validate_host_name(name), Ok(name.to_owned()), "{name}");
    }
}

#[test]
fn a_configured_name_the_host_header_could_never_match_is_refused() {
    for name in [
        "nas.example.com:9898",
        "http://nas.example.com",
        "nas.example.com/",
        "user@nas.example.com",
        "*.example.com",
        "nas example",
        "fd00::1",
        "[fd00::1]:9898",
        "nas.example.com:",
        "",
    ] {
        assert_eq!(
            validate_host_name(name),
            Err(InvalidHostName(name.to_owned())),
            "{name:?}"
        );
    }
}

#[tokio::test]
async fn configured_names_match_whatever_their_case() {
    let checks = RequestChecks::new(["NAS.Example.COM".to_owned()]);
    for host in ["nas.example.com", "NAS.EXAMPLE.COM:9898"] {
        let response = router(Arc::new(NoUi), checks.clone(), test_state())
            .oneshot(request("GET", "/", &[("host", host)]))
            .await
            .unwrap();
        assert_ne!(response.status(), StatusCode::FORBIDDEN, "{host}");
    }
}
