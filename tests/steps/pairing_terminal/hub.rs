//! The hub's daemon: `ladderback serve --hub` serving, stopping, restarting
//! and moving, the hub keys its first start makes, what its log says, and
//! what its store holds once it stops.

use std::net::SocketAddr;
use std::path::PathBuf;

use app::ports::AgentRegistry as _;
use cucumber::{given, then, when};
use domain::fleet::AgentStatus;
use store::SqliteStore;
use testkit::children::Owned;

use super::hub_page::listed_machines;
use super::{hub_root, machine_key, other_hub_root, token};
use crate::locations_steps::level_of;
use crate::request_checks_steps::{Outcome, Request, exchange, start};
use crate::{Agent, blocking};

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

#[when("the hub moves to another address")]
async fn hub_moves(agent: &mut Agent) {
    let before = agent.pairing_terminal.address.expect("a serving hub");
    // Dropping the daemon ends it and waits for it, so nothing answers at
    // the address the machine paired at.
    agent.pairing_terminal.hub = None;
    // The OS may hand back the port just released; start again until it
    // doesn't (dropping a daemon ends it), so the move is a real one.
    let mut started = serve_hub(agent, hub_root(agent), "127.0.0.1:0").await;
    for _ in 0..3 {
        if started.1 != before {
            break;
        }
        drop(started);
        started = serve_hub(agent, hub_root(agent), "127.0.0.1:0").await;
    }
    let (child, moved, log) = started;
    assert_ne!(moved, before);
    agent.pairing_terminal.hub = Some(child);
    agent.pairing_terminal.address = Some(moved);
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

/// What the hub's daemon has written to its standard error so far.
fn hub_log_text(agent: &Agent) -> String {
    let log = agent
        .pairing_terminal
        .hub_log
        .as_ref()
        .expect("a hub's log");
    std::fs::read_to_string(log).expect("the hub's log")
}

#[then(
    expr = "the hub's log says in one line that {string} paired and waits for approval, naming its agent ID and the key the machine showed"
)]
async fn log_says_paired(agent: &mut Agent, hostname: String) {
    let listed = listed_machines(agent).await;
    let machine = listed
        .agents
        .into_iter()
        .find(|machine| machine.hostname == hostname)
        .unwrap_or_else(|| panic!("the page lists {hostname}"));
    assert_eq!(machine.fingerprint, machine_key(agent));
    let text = hub_log_text(agent);
    let lines: Vec<&str> = text
        .lines()
        .filter(|line| line.contains(&machine.agent_id))
        .collect();
    let [line] = lines.as_slice() else {
        panic!("one line names the agent {}: {text}", machine.agent_id);
    };
    assert_eq!(level_of(line), Some("INFO"), "{line}");
    for expected in [
        format!("\"{hostname}\""),
        machine.fingerprint,
        "waits for approval".to_owned(),
    ] {
        assert!(line.contains(&expected), "{expected}: {line}");
    }
}

#[then("the hub's log holds nothing of the token")]
fn log_holds_no_token(agent: &mut Agent) {
    let token = token(agent);
    let text = hub_log_text(agent);
    assert!(!text.contains(token), "{text}");
    assert!(!text.contains("ladderback_pair_"), "{text}");
}

/// The one line of the hub's log that names `hostname`'s agent ID and says
/// `what`: at INFO, with the hostname in quotes and the key the machine
/// showed, as the person matches it against the machine's screen.
async fn one_line_saying(agent: &Agent, hostname: &str, what: &str) {
    let listed = listed_machines(agent).await;
    let machine = listed
        .agents
        .into_iter()
        .find(|machine| machine.hostname == hostname)
        .unwrap_or_else(|| panic!("the page lists {hostname}"));
    assert_eq!(machine.fingerprint, machine_key(agent));
    let text = hub_log_text(agent);
    let lines: Vec<&str> = text
        .lines()
        .filter(|line| line.contains(&machine.agent_id) && line.contains(what))
        .collect();
    let [line] = lines.as_slice() else {
        panic!(
            "one line names the agent {} and {what:?}: {text}",
            machine.agent_id
        );
    };
    assert_eq!(level_of(line), Some("INFO"), "{line}");
    for expected in [format!("machine \"{hostname}\""), machine.fingerprint] {
        assert!(line.contains(&expected), "{expected}: {line}");
    }
}

#[then(
    expr = "the hub's log says in one line that {string} was approved, naming its agent ID and the key the machine showed"
)]
async fn log_says_approved(agent: &mut Agent, hostname: String) {
    one_line_saying(agent, &hostname, " approved as agent ").await;
}

#[then(
    expr = "the hub's log says in one line that {string} connected for the first time since it was approved, naming its agent ID and the key the machine showed"
)]
async fn log_says_first_connection(agent: &mut Agent, hostname: String) {
    one_line_saying(
        agent,
        &hostname,
        "its first connection since it was approved",
    )
    .await;
}

#[then(
    expr = "the hub's log names the machine by the hostname {string} it holds and the hostname {string} it asked for"
)]
async fn log_names_held_hostname(agent: &mut Agent, held: String, asked: String) {
    let expected = format!("machine \"{held}\" (asked as \"{asked}\")");
    one_line_saying(agent, &held, &expected).await;
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

#[then(expr = "once the hub stops, it holds {string} as the machine's time zone")]
async fn hub_holds_zone(agent: &mut Agent, zone: String) {
    // Dropping the daemon ends it and waits for it.
    agent.pairing_terminal.hub = None;
    let database = testkit::daemon::data_dir(&hub_root(agent)).join("ladderback.db");
    let store = SqliteStore::open(&database).await.expect("the hub's store");
    let agents = store.agents().await.expect("the hub's agents");
    store.close().await;
    let [listed] = agents.as_slice() else {
        panic!("one agent: {agents:?}");
    };
    assert_eq!(
        listed.zone.as_ref().map(|zone| zone.as_str()),
        Some(zone.as_str()),
        "{listed:?}"
    );
}

/// The hub's key files the first start makes.
const HUB_KEY_FILES: [&str; 3] = ["hub-key", "hub-recovery-key", "hub-recovery-seed"];

#[when("the hub starts for the first time")]
async fn hub_starts_first_time(agent: &mut Agent) {
    hub_serving(agent).await;
}

#[when("Ladderback starts for the first time on a machine that isn't a hub")]
async fn plain_daemon_starts(agent: &mut Agent) {
    let (child, outcome, _) = start(
        agent.scratch(),
        Some(&other_hub_root(agent)),
        &["--listen", "127.0.0.1:0"],
        None,
    )
    .await;
    assert!(matches!(outcome, Outcome::Listening(_)), "{outcome:?}");
    agent.pairing_terminal.other_hub = Some(child);
}

#[then("the hub's data folder holds a hub key, a recovery key and a recovery seed")]
fn hub_keys_made(agent: &mut Agent) {
    let data = testkit::daemon::data_dir(&hub_root(agent));
    for name in HUB_KEY_FILES {
        let file = data.join(name);
        let held = std::fs::read(&file).unwrap_or_else(|error| panic!("{name}: {error}"));
        assert!(!held.is_empty(), "{name} in {} is empty", data.display());
    }
}

#[then("the machine's data folder holds no hub key, no recovery key and no recovery seed")]
fn no_hub_keys_made(agent: &mut Agent) {
    let data = testkit::daemon::data_dir(&other_hub_root(agent));
    assert!(
        data.is_dir(),
        "the machine never started: {}",
        data.display()
    );
    for name in HUB_KEY_FILES {
        assert!(!data.join(name).exists(), "{name} in {}", data.display());
    }
}

/// What the hub key file holds when it holds no key.
const NOT_A_KEY: &str = "this is not a key\n";

#[given("the hub's data folder holds a hub key file with no key in it")]
fn hub_key_file_is_no_key(agent: &mut Agent) {
    use std::os::unix::fs::PermissionsExt as _;
    let data = testkit::daemon::data_dir(&hub_root(agent));
    std::fs::create_dir_all(&data).expect("the data folder");
    let file = data.join("hub-key");
    std::fs::write(&file, NOT_A_KEY).expect("a hub key file");
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600))
        .expect("the file's mode");
}

#[when("the hub starts")]
async fn hub_starts(agent: &mut Agent) {
    let (_child, outcome, _) = start(
        agent.scratch(),
        Some(&hub_root(agent)),
        &["--listen", "127.0.0.1:0", "--hub"],
        None,
    )
    .await;
    let Outcome::Exited(status, said) = outcome else {
        panic!("the hub started: {outcome:?}");
    };
    assert!(!status.success(), "{status}: {said}");
    agent.pairing_terminal.refused_start = Some(said);
}

#[then("the hub doesn't start, and says its hub key file holds no key it can use")]
fn hub_refused_its_key_file(agent: &mut Agent) {
    let said = agent
        .pairing_terminal
        .refused_start
        .as_deref()
        .expect("a hub that didn't start");
    for expected in ["hub-key", "holds no key", "didn't start"] {
        assert!(said.contains(expected), "{expected}: {said}");
    }
}

#[then("the hub key file still holds what it held")]
fn hub_key_file_unchanged(agent: &mut Agent) {
    let file = testkit::daemon::data_dir(&hub_root(agent)).join("hub-key");
    assert_eq!(
        std::fs::read_to_string(&file).expect("the hub key file"),
        NOT_A_KEY
    );
}

#[given("the hub's hub key file is lost")]
fn hub_key_lost(agent: &mut Agent) {
    let key = testkit::daemon::data_dir(&hub_root(agent)).join("hub-key");
    std::fs::remove_file(&key).unwrap_or_else(|error| panic!("{}: {error}", key.display()));
}

#[then("the hub makes no new hub key")]
fn no_new_hub_key(agent: &mut Agent) {
    let key = testkit::daemon::data_dir(&hub_root(agent)).join("hub-key");
    assert!(!key.exists(), "a new hub key was made");
}

#[then("the hub's log says its hub key file is missing and to restore it from a backup")]
fn log_says_hub_key_missing(agent: &mut Agent) {
    let log = agent
        .pairing_terminal
        .hub_log
        .as_ref()
        .expect("a hub's log");
    let text = std::fs::read_to_string(log).expect("the hub's log");
    assert!(
        text.contains("hub-key")
            && text.contains("is missing while machines are registered")
            && text.contains("restore it from your backup"),
        "{text}"
    );
}
