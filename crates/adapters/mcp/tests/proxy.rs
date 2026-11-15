//! Connect mode never sends through a proxy the environment names
//! (technical design §4.6, decision 87): the daemon is on this computer or
//! behind `https`, and a proxy on the way would read the API token a plain
//! `http` request to loopback carries.
//!
//! The proxy variables are read when the client is built, from the process's
//! environment, which a test can't change safely. So the test changes none
//! of its own: it runs itself again with them set, and that copy asks the
//! daemon. A stand-in proxy on loopback, in this process, records whether
//! anything reached it.

use std::net::TcpListener;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use axum::Router;
use axum::routing::get;
use mcp::{ApiToken, Backend, Connect, DaemonUrl};
use serde_json::{Value, json};

const NAME: &str = "a_proxy_the_environment_names_is_never_sent_the_request_or_its_token";
/// Set in the copy: the proxy variable it was given, which it reports.
const COPY: &str = "LADDERBACK_MCP_PROXY_TEST_COPY";
/// A canary token, never a real one.
const TOKEN: &str = "ladderback_demo_canary-proxy-token";

const PROXY_VARIABLES: [&str; 9] = [
    "HTTP_PROXY",
    "http_proxy",
    "HTTPS_PROXY",
    "https_proxy",
    "ALL_PROXY",
    "all_proxy",
    "NO_PROXY",
    "no_proxy",
    "REQUEST_METHOD",
];

#[test]
fn a_proxy_the_environment_names_is_never_sent_the_request_or_its_token() {
    if std::env::var_os(COPY).is_some() {
        ask_in_copy();
        return;
    }
    let proxy = TcpListener::bind("127.0.0.1:0").expect("a port for the proxy");
    let proxy_url = format!("http://{}", proxy.local_addr().expect("an address"));
    let reached = Arc::new(AtomicUsize::new(0));
    let counting = Arc::clone(&reached);
    std::thread::spawn(move || {
        // Whatever connects is counted and dropped unanswered.
        for stream in proxy.incoming() {
            counting.fetch_add(1, Ordering::SeqCst);
            drop(stream);
        }
    });

    for variable in ["HTTP_PROXY", "http_proxy", "ALL_PROXY", "all_proxy"] {
        run_copy(variable, &proxy_url);
        assert_eq!(
            reached.load(Ordering::SeqCst),
            0,
            "{variable}: a request went to the proxy"
        );
    }
}

/// Runs this test again with `variable` naming `proxy` as the only proxy
/// variable set, and checks that the copy passed.
fn run_copy(variable: &str, proxy: &str) {
    let mut copy = Command::new(std::env::current_exe().expect("the test binary"));
    copy.args(["--exact", NAME, "--nocapture", "--test-threads=1"]);
    for name in PROXY_VARIABLES {
        copy.env_remove(name);
    }
    copy.env(variable, proxy)
        .env(COPY, variable)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let output = copy.output().expect("the copy runs");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    // "1 passed" shows that the copy ran the test, and did not just match none.
    assert!(
        output.status.success() && stdout.contains("1 passed"),
        "{variable}: {stdout}{stderr}"
    );
}

/// The copy's half: a daemon on loopback, asked for its plans with a token,
/// answers them, the token included, as the IDs of its plans.
fn ask_in_copy() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("a runtime");
    let sent = runtime.block_on(async {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("a port for the daemon");
        let addr = listener.local_addr().expect("an address");
        let routes = Router::new().route(
            "/api/v1/plans",
            get(|headers: axum::http::HeaderMap| async move {
                let plans: Vec<Value> = headers
                    .get_all("authorization")
                    .iter()
                    .map(|value| {
                        json!({
                            "id": value.to_str().unwrap_or_default(),
                            "repository": "nas",
                            "scheduled": false,
                            "next-run": null,
                        })
                    })
                    .collect();
                axum::Json(json!({ "plans": plans }))
            }),
        );
        tokio::spawn(async move { axum::serve(listener, routes).await });
        let url = DaemonUrl::parse(&format!("http://{addr}")).expect("a daemon URL");
        let connect = Connect::new(url)
            .expect("a client")
            .with_token(ApiToken::parse(TOKEN).expect("a token"))
            .expect("loopback keeps a token private");
        connect.plans().await
    });
    let ids: Vec<String> = sent
        .expect("the daemon's plans")
        .plans
        .into_iter()
        .map(|plan| plan.id)
        .collect();
    assert_eq!(ids, [format!("Bearer {TOKEN}")]);
}
