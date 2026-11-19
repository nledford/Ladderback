//! Steps for `features/fleet/pairing_from_a_terminal.feature`: the hub's
//! `ladderback pairing-token` and `ladderback serve --hub`, and a machine's
//! `ladderback pair`, each a real process on folders of its own.

use std::io::Write as _;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use app::ports::AgentRegistry as _;
use cucumber::{given, then, when};
use domain::fleet::AgentStatus;
use store::SqliteStore;
use testkit::children::Owned;

use super::request_checks_steps::{Outcome, start};
use super::{Agent, LADDERBACK, blocking};

/// The address a token is made for: loopback, which the scenarios' machines
/// replace with the port the hub serves on.
const TOKEN_ADDRESS: &str = "http://127.0.0.1:9";

/// What the scenarios keep between steps.
#[derive(Debug, Default)]
pub struct PairingTerminal {
    token: Option<String>,
    /// The hub's daemon, ended when the scenario ends or a step stops it.
    hub: Option<Owned>,
    address: Option<SocketAddr>,
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

fn machine_root(agent: &Agent) -> PathBuf {
    agent.scratch().join("machine")
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
    let said = ladderback(
        machine_root(agent),
        &["pair", address, "--hostname", hostname, "--yes"],
        Some(format!("{token}\n")),
    )
    .await;
    agent.pairing_terminal.said = Some(said);
}

#[given("a hub with a pairing token made at its terminal")]
async fn token_made(agent: &mut Agent) {
    let said = ladderback(
        hub_root(agent),
        &["pairing-token", "--address", TOKEN_ADDRESS],
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
    let root = hub_root(agent);
    let (child, outcome, _) = start(
        agent.scratch(),
        Some(&root),
        &["--listen", "127.0.0.1:0", "--hub"],
        None,
    )
    .await;
    let Outcome::Listening(address) = outcome else {
        panic!("the hub didn't start: {outcome:?}");
    };
    agent.pairing_terminal.hub = Some(child);
    agent.pairing_terminal.address = Some(address);
}

#[when(expr = "a machine named {string} pairs with the token from its terminal")]
async fn machine_pairs(agent: &mut Agent, hostname: String) {
    let address = agent.pairing_terminal.address.expect("a serving hub");
    pair(agent, &hostname, &format!("http://{address}")).await;
}

#[when(expr = "a machine pairs with the token at the address {string} from its terminal")]
async fn machine_pairs_at(agent: &mut Agent, address: String) {
    pair(agent, "e2e-laptop", &address).await;
}

#[when("the person asks the hub's terminal for a pairing token")]
async fn token_asked(agent: &mut Agent) {
    let said = ladderback(
        hub_root(agent),
        &["pairing-token", "--address", TOKEN_ADDRESS],
        None,
    )
    .await;
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

#[then("the terminal shows no token")]
fn no_token_shown(agent: &mut Agent) {
    let said = said(agent);
    assert!(!said.stdout.contains("ladderback_pair_"), "{said:?}");
    assert!(!said.stderr.contains("ladderback_pair_"), "{said:?}");
}
