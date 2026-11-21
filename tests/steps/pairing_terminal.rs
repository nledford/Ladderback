//! Steps for `features/fleet/pairing_from_a_terminal.feature`,
//! `features/fleet/approving_from_a_terminal.feature`,
//! `features/fleet/hub_status_from_a_terminal.feature` and the `@e2e`
//! scenarios of `features/fleet/approving_on_the_hubs_page.feature`: the
//! hub's `ladderback pairing-token`, `ladderback serve --hub` and the routes
//! its page approves through, and a machine's `ladderback pair` and
//! `ladderback hub-status`, each a real process on folders of its own.

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
    /// The key fingerprint the machine showed when it paired.
    machine_key: Option<String>,
    said: Option<Said>,
    /// The status and body of the hub's answer to the page's last request.
    page_answer: Option<(u16, String)>,
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
    let key = said
        .stderr
        .split("This machine's key: ")
        .nth(1)
        .and_then(|rest| rest.split(". ").next())
        .unwrap_or_else(|| panic!("the machine showed its key: {said:?}"))
        .to_owned();
    agent.pairing_terminal.machine_key = Some(key);
}

fn machine_key(agent: &Agent) -> &str {
    agent
        .pairing_terminal
        .machine_key
        .as_deref()
        .expect("the machine showed its key")
}

#[given("the hub has stopped")]
fn hub_stopped(agent: &mut Agent) {
    // Dropping the daemon ends it and waits for it.
    agent.pairing_terminal.hub = None;
}

#[when("the hub serves again on the same address")]
async fn hub_serves_again(agent: &mut Agent) {
    let address = agent.pairing_terminal.address.expect("the hub served");
    let (child, again, log) = serve_hub(agent, hub_root(agent), &address.to_string()).await;
    assert_eq!(again, address);
    agent.pairing_terminal.hub = Some(child);
    agent.pairing_terminal.hub_log = Some(log);
}

#[when(expr = "the person approves {string} at the hub's terminal with --yes")]
async fn approves_with_yes(agent: &mut Agent, hostname: String) {
    let args = ["pairing-token", "--approve", &hostname, "--yes"];
    let said = ladderback(hub_root(agent), &args, None).await;
    agent.pairing_terminal.said = Some(said);
}

#[given(expr = "the person approved {string} at the hub's terminal")]
async fn approved_before(agent: &mut Agent, hostname: String) {
    approves_with_yes(agent, hostname).await;
    let said = said(agent);
    assert!(said.success, "{said:?}");
}

#[when(expr = "the person approves {string} at the hub's terminal with no terminal to ask on")]
async fn approves_without_terminal(agent: &mut Agent, hostname: String) {
    let args = ["pairing-token", "--approve", &hostname];
    let said = ladderback(hub_root(agent), &args, None).await;
    agent.pairing_terminal.said = Some(said);
}

#[when("the person lists the machines waiting at the hub's terminal")]
async fn lists_waiting(agent: &mut Agent) {
    let said = ladderback(hub_root(agent), &["pairing-token", "--pending"], None).await;
    agent.pairing_terminal.said = Some(said);
}

#[then(expr = "the terminal named {string} with the key the machine showed, and approved it")]
fn named_and_approved(agent: &mut Agent, hostname: String) {
    let said = said(agent);
    assert!(said.success, "{said:?}");
    let key = machine_key(agent);
    assert!(
        said.stderr.contains(&format!("{hostname} (agent ID "))
            && said.stderr.contains(&format!("is waiting with key {key}")),
        "{said:?}"
    );
    assert!(
        said.stderr.contains(&format!("{hostname} is approved")),
        "{said:?}"
    );
}

#[then("the machine says the hub approved it")]
fn says_approved(agent: &mut Agent) {
    let said = said(agent);
    assert!(said.success, "{said:?}");
    assert!(
        said.stderr
            .contains("Connected. The hub approved this machine."),
        "{said:?}"
    );
}

#[then(expr = "the terminal lists {string} with the key the machine showed")]
fn lists_with_key(agent: &mut Agent, hostname: String) {
    let said = said(agent);
    assert!(said.success, "{said:?}");
    let key = machine_key(agent);
    let lines: Vec<&str> = said.stdout.lines().collect();
    let [line] = lines.as_slice() else {
        panic!("one machine waits: {said:?}");
    };
    // The hostname comes last, since it alone may hold spaces.
    assert!(
        line.contains(&format!("  {key}  ")) && line.ends_with(&format!("  {hostname}")),
        "{said:?}"
    );
}

#[then("the terminal says no machine waits for approval")]
fn says_none_waiting(agent: &mut Agent) {
    let said = said(agent);
    assert!(said.success, "{said:?}");
    assert!(said.stdout.is_empty(), "{said:?}");
    assert!(
        said.stderr.contains("no machines waiting for approval"),
        "{said:?}"
    );
}

#[then("the terminal approved nothing and says to compare the key and run again with --yes")]
fn approved_nothing(agent: &mut Agent) {
    let said = said(agent);
    assert!(!said.success, "{said:?}");
    assert!(
        said.stderr
            .contains(&format!("is waiting with key {}", machine_key(agent))),
        "{said:?}"
    );
    assert!(
        said.stderr.contains("nothing was approved") && said.stderr.contains("--yes"),
        "{said:?}"
    );
}

#[then(expr = "the terminal refuses because {string} is approved already")]
fn refused_approved(agent: &mut Agent, hostname: String) {
    let said = said(agent);
    assert!(!said.success, "{said:?}");
    assert!(
        said.stderr
            .contains(&format!("{hostname} is approved already")),
        "{said:?}"
    );
    // An approved machine is refused before anything is asked.
    assert!(!said.stderr.contains("is waiting with key"), "{said:?}");
}

#[then(expr = "the terminal refuses because no machine named {string} paired with the hub")]
fn refused_unknown(agent: &mut Agent, hostname: String) {
    let said = said(agent);
    assert!(!said.success, "{said:?}");
    assert!(
        said.stderr.contains(&format!(
            "no machine named {hostname} has paired with this hub"
        )),
        "{said:?}"
    );
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

/// A request to the serving hub as its page sends it, over plain HTTP on
/// loopback with sign-in off: the status and body of its answer.
async fn page_asks(agent: &Agent, method: &str, path: &str, body: String) -> (u16, String) {
    let address = agent.pairing_terminal.address.expect("a serving hub");
    let request = Request {
        method: method.to_owned(),
        path: path.to_owned(),
        headers: vec![
            "Origin: http://localhost:{port}".to_owned(),
            "Content-Type: application/json".to_owned(),
        ],
        default_host: true,
        body,
    };
    let answer = blocking(move || exchange(address, &request)).await;
    (answer.status, answer.body)
}

/// The machines the hub's page lists.
async fn listed_machines(agent: &Agent) -> api::HubAgentsDto {
    let (status, body) = page_asks(agent, "GET", api::HUB_AGENTS_PATH, String::new()).await;
    assert_eq!(status, 200, "{body}");
    serde_json::from_str(&body).unwrap_or_else(|error| panic!("{error}: {body}"))
}

/// The hub key's key ID, which confirms an approval with sign-in off.
async fn hub_key_id(agent: &Agent) -> String {
    let (status, body) = page_asks(agent, "GET", api::HUB_PATH, String::new()).await;
    assert_eq!(status, 200, "{body}");
    let hub: api::HubDto = serde_json::from_str(&body).expect("the hub's key");
    hub.key_id
}

/// Approves `agent_id` under `key` on the hub's page, confirmed with the
/// hub key's key ID, and keeps the hub's answer.
async fn page_approves(agent: &mut Agent, agent_id: &str, key: String) {
    let confirmation = hub_key_id(agent).await;
    let body = serde_json::to_string(&api::ApproveAgentDto { key, confirmation }).expect("JSON");
    let path = api::agent_approval_path(agent_id);
    let answer = page_asks(agent, "POST", &path, body).await;
    agent.pairing_terminal.page_answer = Some(answer);
}

async fn approves_on_the_page(agent: &mut Agent, hostname: &str) {
    let listed = listed_machines(agent).await;
    let machine = listed
        .agents
        .into_iter()
        .find(|machine| machine.hostname == hostname)
        .unwrap_or_else(|| panic!("the page lists {hostname}"));
    page_approves(agent, &machine.agent_id, machine.key).await;
}

fn page_answer(agent: &Agent) -> &(u16, String) {
    agent
        .pairing_terminal
        .page_answer
        .as_ref()
        .expect("the page asked the hub")
}

#[when("the person reads the machines on the hub's page")]
async fn reads_machines(agent: &mut Agent) {
    let (status, body) = page_asks(agent, "GET", api::HUB_AGENTS_PATH, String::new()).await;
    agent.pairing_terminal.page_answer = Some((status, body));
}

#[then(expr = "the page lists {string} as waiting with the key the machine showed")]
fn page_lists_waiting(agent: &mut Agent, hostname: String) {
    let (status, body) = page_answer(agent);
    assert_eq!(*status, 200, "{body}");
    let listed: api::HubAgentsDto = serde_json::from_str(body).expect("the machines");
    let [machine] = listed.agents.as_slice() else {
        panic!("one machine: {body}");
    };
    assert_eq!(machine.hostname, hostname);
    assert_eq!(machine.status, api::AgentStatusDto::Pending);
    assert_eq!(machine.fingerprint, machine_key(agent));
}

#[when(expr = "the person approves {string} on the hub's page, confirming with the hub's key ID")]
async fn approves_on_page(agent: &mut Agent, hostname: String) {
    approves_on_the_page(agent, &hostname).await;
}

#[given(expr = "the person approved {string} on the hub's page")]
async fn approved_on_page_before(agent: &mut Agent, hostname: String) {
    approves_on_the_page(agent, &hostname).await;
    let (status, body) = page_answer(agent);
    assert_eq!(*status, 200, "{body}");
}

#[when(
    "the person approves an agent ID that never paired on the hub's page, confirming with the hub's key ID"
)]
async fn approves_unknown_on_page(agent: &mut Agent) {
    // A UUIDv7 no machine made, under a key no machine holds.
    let key = format!("05{}", "0".repeat(62));
    page_approves(agent, "01890a5d-ac96-774b-bcce-b302099a8057", key).await;
}

#[then(expr = "the hub answers that {string} is approved")]
fn answers_approved(agent: &mut Agent, hostname: String) {
    let (status, body) = page_answer(agent);
    assert_eq!(*status, 200, "{body}");
    let machine: api::HubAgentDto = serde_json::from_str(body).expect("the machine");
    assert_eq!(machine.hostname, hostname);
    assert_eq!(machine.status, api::AgentStatusDto::Approved);
    assert_eq!(machine.fingerprint, machine_key(agent));
}

#[then(expr = "the hub refuses with the code {string}")]
fn refuses_with_code(agent: &mut Agent, code: String) {
    let (status, body) = page_answer(agent);
    let answer: serde_json::Value = serde_json::from_str(body).unwrap_or_default();
    assert_eq!(
        answer["code"].as_str(),
        Some(code.as_str()),
        "{status} {body}"
    );
}
