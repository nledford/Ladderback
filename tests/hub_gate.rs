//! A build without the `hub` feature, as a release is until M8 (§16, decision
//! 122, item 5): it refuses the hub's commands as unknown, and its `serve`
//! answers 404 on every path under `/sync/`, the agent routes, whatever
//! name it is reached by. `just test-release` runs these; a default build
//! has the hub, whose own tests cover it.

#![cfg(not(feature = "hub"))]

use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

const LADDERBACK: &str = env!("CARGO_BIN_EXE_ladderback");

/// How long a daemon that was just started gets to say where it listens.
const STARTUP: Duration = Duration::from_secs(30);

/// How long one request to the daemon may take on loopback.
const ANSWER: Duration = Duration::from_secs(10);

/// A running `ladderback serve`, stopped when dropped.
struct Daemon {
    child: Child,
    address: String,
    _scratch: tempfile::TempDir,
}

impl Daemon {
    fn start() -> Self {
        Self::start_with(None)
    }

    /// A daemon whose `config.toml` holds `config`, when given.
    fn start_with(config: Option<&str>) -> Self {
        let scratch = tempfile::tempdir().expect("a scratch folder for the daemon");
        if let Some(config) = config {
            let folder = testkit::daemon::config_dir(scratch.path());
            std::fs::create_dir_all(&folder).expect("a config folder");
            std::fs::write(folder.join("config.toml"), config).expect("a config.toml");
        }
        let mut command = Command::new(LADDERBACK);
        testkit::daemon::isolate(&mut command, scratch.path());
        command
            .args(["serve", "--listen", "127.0.0.1:0"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .env("NO_COLOR", "1")
            .env("LC_ALL", "C")
            .env_remove("LANGUAGE")
            .env("LADDERBACK_RELEASES_URL", "http://127.0.0.1:9/");
        let mut child = spawn_gate::hold(|| command.spawn()).expect("ladderback serve started");
        let stderr = child.stderr.take().expect("a piped stderr");
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                if sender.send(line).is_err() {
                    return;
                }
            }
        });
        let address = listening_address(&receiver);
        Self {
            child,
            address,
            _scratch: scratch,
        }
    }

    /// The status of `GET target` with `Host: host`.
    fn status_of(&self, target: &str, host: &str) -> u16 {
        let address = self
            .address
            .parse()
            .expect("an address the daemon listens on");
        let mut stream = testkit::net::connect(address).expect("the daemon accepts");
        stream
            .set_read_timeout(Some(ANSWER))
            .expect("a read timeout");
        write!(
            stream,
            "GET {target} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n"
        )
        .expect("the request is sent");
        let mut answer = String::new();
        let _ = stream.read_to_string(&mut answer);
        let status = answer.split(' ').nth(1).unwrap_or_default();
        status
            .parse()
            .unwrap_or_else(|_| panic!("a status line in {answer:?}"))
    }
}

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// The `host:port` of the daemon's `listening on http://…` line.
fn listening_address(lines: &Receiver<String>) -> String {
    let deadline = Instant::now() + STARTUP;
    let mut seen = Vec::new();
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        match lines.recv_timeout(left) {
            Ok(line) => {
                if let Some((_, rest)) = line.split_once("listening on http://") {
                    let address = rest.split(['/', ' ']).next().unwrap_or_default();
                    return address.to_owned();
                }
                seen.push(line);
            }
            Err(RecvTimeoutError::Timeout) => {
                panic!("no `listening on` line in {STARTUP:?}; stderr so far: {seen:#?}")
            }
            Err(RecvTimeoutError::Disconnected) => {
                panic!("ladderback exited before it listened; stderr: {seen:#?}")
            }
        }
    }
}

#[test]
fn every_path_under_sync_answers_404() {
    let daemon = Daemon::start();
    for target in [
        "/sync",
        "/sync/",
        "/sync/v1",
        "/sync/v1/",
        "/sync/v1/connect",
        "/sync/v2/a/b?x=1",
    ] {
        assert_eq!(daemon.status_of(target, "127.0.0.1"), 404, "{target}");
    }
    // The daemon's own routes are still served: the 404 is the agent routes'.
    assert_eq!(daemon.status_of("/api/v1/log", "127.0.0.1"), 200);
    // Nothing in the build can pair, so it has no pairing state to answer.
    assert_eq!(daemon.status_of("/api/v1/pairing", "127.0.0.1"), 404);
}

/// Without the hub, `/sync/v1` isn't behind a check of its own: a name the
/// daemon wasn't told to answer to is refused there as anywhere.
#[test]
fn the_sync_routes_get_the_request_checks() {
    let daemon = Daemon::start();
    for target in ["/sync/v1", "/sync/v1/connect"] {
        assert_eq!(
            daemon.status_of(target, "hub.other.example:8443"),
            403,
            "{target}"
        );
    }
}

/// Hub mode in `config.toml` is read by a build without the hub too, so a
/// configuration copied from a hub still loads; the daemon serves as a
/// machine, and the agent routes stay unserved (§16, decisions 122 and 167).
#[test]
fn hub_mode_in_the_config_serves_no_hub_without_the_hub() {
    let daemon = Daemon::start_with(Some("schema = 1\n\n[serve]\nhub = true\n"));
    assert_eq!(daemon.status_of("/sync/v1", "127.0.0.1"), 404);
    assert_eq!(daemon.status_of("/api/v1/log", "127.0.0.1"), 200);
}

/// The hub's commands are refused by the argument parser, before anything
/// starts, as any command it doesn't know is.
#[test]
fn the_hub_commands_are_unknown() {
    for args in [
        &["serve", "--hub"][..],
        &["pairing-token"],
        &["pair"],
        &["hub-status"],
        &["recovery-kit"],
        &["hub-recover"],
    ] {
        let scratch = tempfile::tempdir().expect("a scratch folder");
        let mut command = Command::new(LADDERBACK);
        testkit::daemon::isolate(&mut command, scratch.path());
        command
            .args(args)
            .env("LC_ALL", "C")
            .env_remove("LANGUAGE")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let output = spawn_gate::hold(|| command.spawn())
            .expect("ladderback ran")
            .wait_with_output()
            .expect("ladderback ran");
        assert_eq!(output.status.code(), Some(2), "{args:?}");
    }
}

/// The MCP server has no `pairing_state` without the hub, since nothing in
/// the build can pair: a one-shot call of it is an unknown tool, refused
/// before any session starts, as any tool the server doesn't have is.
#[test]
fn the_mcp_server_has_no_pairing_tool() {
    let scratch = tempfile::tempdir().expect("a scratch folder");
    let mut command = Command::new(LADDERBACK);
    testkit::daemon::isolate(&mut command, scratch.path());
    command
        .args(["mcp", "call", "pairing_state"])
        .env("LC_ALL", "C")
        .env_remove("LANGUAGE")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let output = spawn_gate::hold(|| command.spawn())
        .expect("ladderback ran")
        .wait_with_output()
        .expect("ladderback ran");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(2), "{stderr}");
    assert!(stderr.contains("pairing_state"), "{stderr}");
}

/// Nor has it the pairing guide, which explains a `pair` command the build
/// doesn't have: `resources/list` leaves it out, and reading it is refused
/// as reading any resource the server doesn't have is.
#[test]
fn the_mcp_server_has_no_pairing_guide() {
    let scratch = tempfile::tempdir().expect("a scratch folder");
    let mut command = Command::new(LADDERBACK);
    testkit::daemon::isolate(&mut command, scratch.path());
    command
        .args(["mcp", "--headless"])
        .env("LC_ALL", "C")
        .env_remove("LANGUAGE")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = spawn_gate::hold(|| command.spawn()).expect("ladderback mcp started");
    let meta = serde_json::json!({
        "io.modelcontextprotocol/protocolVersion": "2026-07-28",
        "io.modelcontextprotocol/clientCapabilities": {},
    });
    let requests = [
        serde_json::json!({"jsonrpc": "2.0", "id": 1, "method": "resources/list",
            "params": {"_meta": meta}}),
        serde_json::json!({"jsonrpc": "2.0", "id": 2, "method": "resources/read",
            "params": {"_meta": meta, "uri": "ladderback://pairing"}}),
    ];
    {
        let mut stdin = child.stdin.take().expect("a piped stdin");
        for request in requests {
            writeln!(stdin, "{request}").expect("the request is sent");
        }
    }
    let output = child.wait_with_output().expect("ladderback mcp ended");
    let stderr = String::from_utf8_lossy(&output.stderr);
    let answers: Vec<serde_json::Value> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|line| serde_json::from_str(line).expect("a JSON-RPC answer"))
        .collect();
    let answer = |id: u64| {
        answers
            .iter()
            .find(|answer| answer["id"] == id)
            .unwrap_or_else(|| panic!("no answer to {id} in {answers:#?}; stderr: {stderr}"))
    };
    let listed = answer(1)["result"]["resources"]
        .as_array()
        .unwrap_or_else(|| panic!("a resource list in {:#}", answer(1)));
    assert!(!listed.is_empty(), "the other resources are listed");
    assert!(
        listed
            .iter()
            .all(|resource| resource["uri"] != "ladderback://pairing"),
        "{listed:#?}"
    );
    assert!(answer(2)["result"].is_null(), "{:#}", answer(2));
    assert!(answer(2)["error"]["code"].is_i64(), "{:#}", answer(2));
}
