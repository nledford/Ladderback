//! Steps for `features/fleet/pairing_from_a_terminal.feature` and
//! `features/fleet/hub_status_from_a_terminal.feature`: the hub's
//! `ladderback pairing-token` and `ladderback serve --hub`, and a machine's
//! `ladderback pair` and `ladderback hub-status`, each a real process on
//! folders of its own.

use std::io::Write as _;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use app::ports::AgentRegistry as _;
use cucumber::{given, then, when};
use domain::fleet::AgentStatus;
use store::SqliteStore;
use testkit::children::Owned;

use super::request_checks_steps::{Outcome, Request, exchange, start};
use super::{Agent, LADDERBACK, blocking};

/// The address a token is made for: loopback, which the scenarios' machines
/// replace with the port the hub serves on.
const TOKEN_ADDRESS: &str = "http://127.0.0.1:9";

/// The same over the hub's built-in TLS, whose certificate a token for it
/// pins.
const TLS_TOKEN_ADDRESS: &str = "https://127.0.0.1:9";

/// What the scenarios keep between steps.
#[derive(Debug, Default)]
pub struct PairingTerminal {
    token: Option<String>,
    /// The hub's daemon, ended when the scenario ends or a step stops it.
    hub: Option<Owned>,
    address: Option<SocketAddr>,
    /// What the hub's daemon wrote to its standard error, its log among it.
    hub_log: Option<PathBuf>,
    /// Another hub's daemon, on folders of its own, and its address.
    other_hub: Option<Owned>,
    other_address: Option<SocketAddr>,
    said: Option<Said>,
}

/// What a command said, and whether it succeeded.
#[derive(Debug)]
struct Said {
    success: bool,
    stdout: String,
    stderr: String,
}

fn hub_root(agent: &Agent) -> PathBuf {
    agent.scratch().join("hub")
}

/// The folders of the machine the scenario pairs.
pub(super) fn machine_root(agent: &Agent) -> PathBuf {
    agent.scratch().join("machine")
}

/// The pairing token the hub's terminal made last.
pub(super) fn token(agent: &Agent) -> &str {
    agent.pairing_terminal.token.as_deref().expect("a token")
}

/// The address the hub serves on.
pub(super) fn hub_address(agent: &Agent) -> SocketAddr {
    agent.pairing_terminal.address.expect("a serving hub")
}

fn other_hub_root(agent: &Agent) -> PathBuf {
    agent.scratch().join("other-hub")
}

/// Runs `ladderback args` on the folders under `root`, in English, with
/// `stdin` as its standard input, on a blocking thread: see [`blocking`].
async fn ladderback(root: PathBuf, args: &[&str], stdin: Option<String>) -> Said {
    let args: Vec<String> = args.iter().map(|arg| (*arg).to_owned()).collect();
    blocking(move || run(&root, &args, stdin)).await
}

fn run(root: &Path, args: &[String], stdin: Option<String>) -> Said {
    let mut command = Command::new(LADDERBACK);
    testkit::daemon::isolate(&mut command, root);
    command
        .args(args)
        .stdin(if stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("LC_ALL", "C")
        .env_remove("LANGUAGE");
    let mut child = command.spawn().expect("ladderback starts");
    if let Some(text) = stdin {
        child
            .stdin
            .take()
            .expect("a standard input")
            .write_all(text.as_bytes())
            .expect("the token is written");
    }
    let output = child.wait_with_output().expect("ladderback ends");
    Said {
        success: output.status.success(),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    }
}

fn said(agent: &Agent) -> &Said {
    agent.pairing_terminal.said.as_ref().expect("a command ran")
}

async fn pair(agent: &mut Agent, hostname: &str, address: &str) {
    let token = agent.pairing_terminal.token.clone().expect("a token");
    pair_pasting(agent, hostname, address, format!("{token}\n")).await;
}

async fn pair_pasting(agent: &mut Agent, hostname: &str, address: &str, pasted: String) {
    let said = ladderback(
        machine_root(agent),
        &["pair", address, "--hostname", hostname, "--yes"],
        Some(pasted),
    )
    .await;
    agent.pairing_terminal.said = Some(said);
}

#[given("a hub with a pairing token made at its terminal")]
async fn token_made(agent: &mut Agent) {
    make_token(agent, TOKEN_ADDRESS).await;
}

#[given("a hub with a pairing token for its https address made at its terminal")]
async fn tls_token_made(agent: &mut Agent) {
    make_token(agent, TLS_TOKEN_ADDRESS).await;
}

async fn make_token(agent: &mut Agent, address: &str) {
    let said = ladderback(
        hub_root(agent),
        &["pairing-token", "--address", address],
        None,
    )
    .await;
    assert!(said.success, "{said:?}");
    let token = said.stdout.trim();
    assert!(token.starts_with("ladderback_pair_"), "{said:?}");
    agent.pairing_terminal.token = Some(token.to_owned());
}

#[given("the person revoked the token at the hub's terminal")]
async fn token_revoked(agent: &mut Agent) {
    let listed = ladderback(hub_root(agent), &["pairing-token", "--list"], None).await;
    let id = listed
        .stdout
        .split_whitespace()
        .next()
        .unwrap_or_else(|| panic!("a token ID: {listed:?}"))
        .to_owned();
    let said = ladderback(hub_root(agent), &["pairing-token", "--revoke", &id], None).await;
    assert!(said.success, "{said:?}");
}

#[given("the hub is serving on loopback")]
async fn hub_serving(agent: &mut Agent) {
    let (child, address, log) = serve_hub(agent, hub_root(agent), "127.0.0.1:0").await;
    agent.pairing_terminal.hub = Some(child);
    agent.pairing_terminal.address = Some(address);
    agent.pairing_terminal.hub_log = Some(log);
}

#[given("another hub is serving on loopback")]
async fn other_hub_serving(agent: &mut Agent) {
    let (child, address, _) = serve_hub(agent, other_hub_root(agent), "127.0.0.1:0").await;
    agent.pairing_terminal.other_hub = Some(child);
    agent.pairing_terminal.other_address = Some(address);
}

#[given("the hub's TLS key file is lost")]
fn tls_key_lost(agent: &mut Agent) {
    let key = testkit::daemon::data_dir(&hub_root(agent)).join("hub-tls-key");
    std::fs::remove_file(&key).unwrap_or_else(|error| panic!("{}: {error}", key.display()));
}

#[when("the hub restarts on the same address")]
async fn hub_restarts(agent: &mut Agent) {
    let address = agent.pairing_terminal.address.expect("a serving hub");
    // Dropping the daemon ends it and waits for it.
    agent.pairing_terminal.hub = None;
    let (child, again, log) = serve_hub(agent, hub_root(agent), &address.to_string()).await;
    assert_eq!(again, address);
    agent.pairing_terminal.hub = Some(child);
    agent.pairing_terminal.hub_log = Some(log);
}

/// `serve --hub` on the folders under `root`, listening on `listen`; its
/// address and the file its standard error goes to.
async fn serve_hub(agent: &Agent, root: PathBuf, listen: &str) -> (Owned, SocketAddr, PathBuf) {
    let (child, outcome, log) = start(
        agent.scratch(),
        Some(&root),
        &["--listen", listen, "--hub"],
        None,
    )
    .await;
    let Outcome::Listening(address) = outcome else {
        panic!("the hub didn't start: {outcome:?}");
    };
    (child, address, log)
}

/// A request to the serving hub, over plain HTTP on loopback.
async fn ask_the_hub(agent: &Agent, path: &str, headers: &[&str]) -> u16 {
    let address = agent.pairing_terminal.address.expect("a serving hub");
    let request = Request {
        method: "GET".to_owned(),
        path: path.to_owned(),
        headers: headers.iter().map(|header| (*header).to_owned()).collect(),
        default_host: true,
        body: String::new(),
    };
    blocking(move || exchange(address, &request).status).await
}

#[then("the hub refuses a machine's connection over plain http on loopback")]
async fn sync_refused_plain(agent: &mut Agent) {
    let status = ask_the_hub(
        agent,
        api::sync::PATH,
        &[
            "Connection: Upgrade",
            "Upgrade: websocket",
            "Sec-WebSocket-Version: 13",
            "Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==",
        ],
    )
    .await;
    assert_eq!(status, 503, "the hub answered a machine's connection");
}

#[then("the hub still serves its own machine on loopback")]
async fn serves_loopback(agent: &mut Agent) {
    assert_eq!(ask_the_hub(agent, "/api/v1/repositories", &[]).await, 200);
}

#[then("the hub's log says its TLS key is missing and to restore it from a backup")]
fn log_says_tls_missing(agent: &mut Agent) {
    let log = agent
        .pairing_terminal
        .hub_log
        .as_ref()
        .expect("a hub's log");
    let text = std::fs::read_to_string(log).expect("the hub's log");
    assert!(
        text.contains("hub-tls-key") && text.contains("restore it from your backup"),
        "{text}"
    );
}

#[when(expr = "a machine named {string} pairs with the token from its terminal")]
async fn machine_pairs(agent: &mut Agent, hostname: String) {
    let address = agent.pairing_terminal.address.expect("a serving hub");
    pair(agent, &hostname, &format!("http://{address}")).await;
}

#[when(
    expr = "a machine named {string} pairs with the token at the hub's https address from its terminal"
)]
async fn machine_pairs_over_tls(agent: &mut Agent, hostname: String) {
    let address = agent.pairing_terminal.address.expect("a serving hub");
    pair(agent, &hostname, &format!("https://{address}")).await;
}

#[when(
    expr = "a machine named {string} pairs with the token at the other hub's https address from its terminal"
)]
async fn machine_pairs_with_other(agent: &mut Agent, hostname: String) {
    let address = agent
        .pairing_terminal
        .other_address
        .expect("another serving hub");
    pair(agent, &hostname, &format!("https://{address}")).await;
}

#[given(expr = "a machine named {string} paired with the token from its terminal")]
async fn machine_paired_before(agent: &mut Agent, hostname: String) {
    machine_pairs(agent, hostname).await;
    let said = said(agent);
    assert!(said.success, "{said:?}");
}

#[given(
    expr = "a machine named {string} paired with the token at the hub's https address from its terminal"
)]
async fn machine_paired_over_tls_before(agent: &mut Agent, hostname: String) {
    machine_pairs_over_tls(agent, hostname).await;
    let said = said(agent);
    assert!(said.success, "{said:?}");
}

#[then("the machine read a token pinned to the hub's own certificate")]
fn read_pinned(agent: &mut Agent) {
    let said = said(agent);
    assert!(
        said.stderr
            .contains("its certificate is pinned by the token"),
        "{said:?}"
    );
}

#[when("the machine checks its link to the hub from its terminal")]
async fn machine_checks(agent: &mut Agent) {
    let said = ladderback(machine_root(agent), &["hub-status"], None).await;
    agent.pairing_terminal.said = Some(said);
}

#[when(expr = "a machine pairs with the token at the address {string} from its terminal")]
async fn machine_pairs_at(agent: &mut Agent, address: String) {
    pair(agent, "e2e-laptop", &address).await;
}

#[when(
    expr = "a machine named {string} pairs with the token pasted across two lines from its terminal"
)]
async fn machine_pairs_wrapped(agent: &mut Agent, hostname: String) {
    let address = agent.pairing_terminal.address.expect("a serving hub");
    let token = agent.pairing_terminal.token.clone().expect("a token");
    let (first, second) = token.split_at(token.len() / 2);
    let pasted = format!("{first}\n{second}\n");
    pair_pasting(agent, &hostname, &format!("http://{address}"), pasted).await;
}

#[when("the person asks the hub's terminal for a pairing token")]
async fn token_asked(agent: &mut Agent) {
    ask_for_token(agent, &["pairing-token", "--address", TOKEN_ADDRESS]).await;
}

#[when(expr = "the person asks the hub's terminal for a pairing token for {string}")]
async fn token_asked_for(agent: &mut Agent, address: String) {
    ask_for_token(agent, &["pairing-token", "--address", &address]).await;
}

#[when(expr = "the person asks the hub's terminal for a pairing token lasting {word} minutes")]
async fn token_asked_lasting(agent: &mut Agent, minutes: String) {
    let args = [
        "pairing-token",
        "--address",
        TOKEN_ADDRESS,
        "--ttl",
        &minutes,
    ];
    ask_for_token(agent, &args).await;
}

async fn ask_for_token(agent: &mut Agent, args: &[&str]) {
    let said = ladderback(hub_root(agent), args, None).await;
    agent.pairing_terminal.said = Some(said);
}

#[then("the machine is paired and waits for approval")]
fn machine_paired(agent: &mut Agent) {
    let said = said(agent);
    assert!(said.success, "{said:?}");
    assert!(
        said.stderr.contains("The hub holds this machine pending"),
        "{said:?}"
    );
}

#[then(expr = "once the hub stops, it lists {string} as waiting for approval")]
async fn hub_lists_pending(agent: &mut Agent, hostname: String) {
    // Dropping the daemon ends it and waits for it.
    agent.pairing_terminal.hub = None;
    let database = testkit::daemon::data_dir(&hub_root(agent)).join("ladderback.db");
    let store = SqliteStore::open(&database).await.expect("the hub's store");
    let agents = store.agents().await.expect("the hub's agents");
    store.close().await;
    let [listed] = agents.as_slice() else {
        panic!("one agent: {agents:?}");
    };
    assert_eq!(listed.hostname.as_str(), hostname);
    assert!(
        matches!(listed.status, AgentStatus::Pending { .. }),
        "{listed:?}"
    );
}

#[then("the hub's terminal lists no unused token")]
async fn no_unused_token(agent: &mut Agent) {
    agent.pairing_terminal.hub = None;
    let said = ladderback(hub_root(agent), &["pairing-token", "--list"], None).await;
    assert!(said.success, "{said:?}");
    assert_eq!(said.stdout, "", "{said:?}");
    assert!(said.stderr.contains("no unused pairing tokens"), "{said:?}");
}

#[then(regex = r"^the machine is refused because (.+)$")]
fn machine_refused(agent: &mut Agent, reason: String) {
    let expected = match reason.as_str() {
        "the token was revoked" => "This token was canceled on the hub",
        "the address would show the token to the network" => "shows the token to the network",
        "the hub's certificate isn't the one the token names" => {
            "showed a certificate that isn't the one its token names"
        }
        "the hub can't be reached" => "couldn't reach the hub at",
        other => panic!("no refusal is named {other:?}"),
    };
    let said = said(agent);
    assert!(!said.success, "{said:?}");
    assert!(said.stderr.contains(expected), "{said:?}");
}

#[then("the terminal refuses because the daemon holds the data folder")]
fn refused_while_running(agent: &mut Agent) {
    let said = said(agent);
    assert!(!said.success, "{said:?}");
    assert!(
        said.stderr
            .contains("the daemon is running on the data folder"),
        "{said:?}"
    );
}

#[then("the terminal refuses because the address isn't a hub address")]
fn refused_address(agent: &mut Agent) {
    let said = said(agent);
    assert!(!said.success, "{said:?}");
    assert!(said.stderr.contains("isn't a hub address"), "{said:?}");
}

#[then("the terminal refuses because a token lives 5 to 1440 minutes")]
fn refused_life(agent: &mut Agent) {
    let said = said(agent);
    assert!(!said.success, "{said:?}");
    assert!(
        said.stderr.contains("a token lives 5 to 1440 minutes"),
        "{said:?}"
    );
}

#[then(
    "the terminal refuses because no machine pairs until the hub starts with its TLS key restored from a backup"
)]
fn refused_tls_key_lost(agent: &mut Agent) {
    let said = said(agent);
    assert!(!said.success, "{said:?}");
    for expected in [
        "hub-tls-key",
        "pairs no machine until it starts with the key restored",
        "Restore the file from your backup",
        "start the hub",
    ] {
        assert!(said.stderr.contains(expected), "{expected}: {said:?}");
    }
}

#[then("the machine says the hub holds it waiting for approval")]
fn says_pending(agent: &mut Agent) {
    let said = said(agent);
    assert!(said.success, "{said:?}");
    assert!(
        said.stderr
            .contains("The hub holds this machine waiting for approval"),
        "{said:?}"
    );
}

#[then("the machine says it isn't paired with a hub")]
fn says_not_paired(agent: &mut Agent) {
    let said = said(agent);
    assert!(!said.success, "{said:?}");
    assert!(said.stderr.contains("isn't paired with a hub"), "{said:?}");
}

/// A command that only checks writes nothing, not even the agent ID a
/// machine gets when it first pairs or serves.
#[then("the machine's data folder holds nothing")]
fn data_folder_empty(agent: &mut Agent) {
    let data = testkit::daemon::data_dir(&machine_root(agent));
    let held: Vec<_> = match std::fs::read_dir(&data) {
        Ok(entries) => entries.map(|entry| entry.unwrap().file_name()).collect(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => vec![],
        Err(error) => panic!("{}: {error}", data.display()),
    };
    assert!(held.is_empty(), "{held:?}");
}

#[then("the terminal shows no token")]
fn no_token_shown(agent: &mut Agent) {
    let said = said(agent);
    assert!(!said.stdout.contains("ladderback_pair_"), "{said:?}");
    assert!(!said.stderr.contains("ladderback_pair_"), "{said:?}");
}
