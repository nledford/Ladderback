//! Steps for `features/fleet/hub_mode.feature`: hub mode, the address and
//! the allowed host names in the hub's `config.toml`, the flags of `serve`
//! over them, and hub mode turned off kept on while a machine is paired.

use std::net::SocketAddr;

use cucumber::{given, then, when};

use super::hub::HUB_KEY_FILES;
use super::hub_root;
use crate::request_checks_steps::{Outcome, Request, exchange, start};
use crate::{Agent, blocking};

/// What the scenario's steps wrote into the hub's `[serve]` table so far.
#[derive(Debug, Default)]
pub struct ServeTable {
    hub: Option<bool>,
    listen: Option<SocketAddr>,
    allowed_hosts: Vec<String>,
}

impl ServeTable {
    /// The file's text. With no address of its own it listens on a free
    /// port on loopback, so no scenario ever takes `serve`'s default port.
    fn text(&self) -> String {
        let listen = self
            .listen
            .map_or_else(|| "127.0.0.1:0".to_owned(), |address| address.to_string());
        let mut text = format!("schema = 1\n\n[serve]\nlisten = \"{listen}\"\n");
        if !self.allowed_hosts.is_empty() {
            let names: Vec<String> = self
                .allowed_hosts
                .iter()
                .map(|name| format!("\"{name}\""))
                .collect();
            text.push_str(&format!("allowed-hosts = [{}]\n", names.join(", ")));
        }
        if let Some(hub) = self.hub {
            text.push_str(&format!("hub = {hub}\n"));
        }
        text
    }
}

/// Writes the hub's `config.toml` from what the scenario set.
fn write_config(agent: &Agent) {
    let folder = testkit::daemon::config_dir(&hub_root(agent));
    std::fs::create_dir_all(&folder).expect("the hub's config folder");
    std::fs::write(
        folder.join("config.toml"),
        agent.pairing_terminal.serve_table.text(),
    )
    .expect("the hub's config.toml");
}

/// A port nothing listens on just now, on loopback.
fn free_port() -> SocketAddr {
    std::net::TcpListener::bind("127.0.0.1:0")
        .and_then(|listener| listener.local_addr())
        .expect("a free port on loopback")
}

#[given("the hub's config.toml turns hub mode on")]
fn config_turns_hub_mode_on(agent: &mut Agent) {
    agent.pairing_terminal.serve_table.hub = Some(true);
    write_config(agent);
}

#[given(
    expr = "the hub's config.toml listens on a free port on loopback and allows the host name {string}"
)]
fn config_listens_and_allows(agent: &mut Agent, name: String) {
    let table = &mut agent.pairing_terminal.serve_table;
    table.listen = Some(free_port());
    table.allowed_hosts.push(name);
    write_config(agent);
}

/// Starts the hub's daemon with `args` and keeps it as the serving hub.
async fn hub_starts_with(agent: &mut Agent, args: &[&str]) {
    let (child, outcome, log) = start(agent.scratch(), Some(&hub_root(agent)), args, None).await;
    let Outcome::Listening(address) = outcome else {
        panic!("the hub didn't start: {outcome:?}");
    };
    agent.pairing_terminal.hub = Some(child);
    agent.pairing_terminal.address = Some(address);
    agent.pairing_terminal.hub_log = Some(log);
}

#[when("the hub starts with no command-line flag")]
async fn hub_starts_without_flags(agent: &mut Agent) {
    hub_starts_with(agent, &[]).await;
}

#[when(expr = "the hub starts with {string}")]
async fn hub_starts_with_flags(agent: &mut Agent, flags: String) {
    let args: Vec<&str> = flags.split(' ').collect();
    hub_starts_with(agent, &args).await;
}

/// Ends the serving hub, writes its `config.toml` and starts it again with
/// `args`, keeping what came of it, a hub or a refusal.
async fn restart(agent: &mut Agent, args: &[&str]) {
    // Dropping the daemon ends it and waits for it.
    agent.pairing_terminal.hub = None;
    write_config(agent);
    let (child, outcome, log) = start(agent.scratch(), Some(&hub_root(agent)), args, None).await;
    agent.pairing_terminal.hub_log = Some(log);
    match outcome {
        Outcome::Listening(address) => {
            agent.pairing_terminal.hub = Some(child);
            agent.pairing_terminal.address = Some(address);
        }
        Outcome::Exited(status, said) => {
            assert!(!status.success(), "{status}: {said}");
            agent.pairing_terminal.refused_start = Some(said);
        }
    }
}

#[when("the hub restarts with hub mode off in its config.toml")]
async fn hub_restarts_with_hub_mode_off(agent: &mut Agent) {
    agent.pairing_terminal.serve_table.hub = Some(false);
    restart(agent, &[]).await;
}

#[when(expr = "the hub restarts with {string} over hub mode on in its config.toml")]
async fn hub_restarts_with_flag_over_config(agent: &mut Agent, flag: String) {
    agent.pairing_terminal.serve_table.hub = Some(true);
    restart(agent, &[flag.as_str()]).await;
}

#[when("the hub tries to start with no command-line flag")]
async fn hub_tries_to_start(agent: &mut Agent) {
    restart(agent, &[]).await;
}

#[given(expr = "the hub's config.toml allows the host name {string}")]
fn config_allows(agent: &mut Agent, name: String) {
    agent.pairing_terminal.serve_table.allowed_hosts.push(name);
    write_config(agent);
}

#[given(expr = "the hub's config.toml listens on {string}")]
fn config_listens_on(agent: &mut Agent, address: String) {
    agent.pairing_terminal.serve_table.listen = Some(address.parse().expect("an address"));
    write_config(agent);
}

#[then(expr = "the hub doesn't start, and says {string} in config.toml is the cause")]
fn refused_naming_config(agent: &mut Agent, key: String) {
    let said = agent
        .pairing_terminal
        .refused_start
        .as_deref()
        .expect("a hub that didn't start");
    for expected in [key.as_str(), "[serve]", "config.toml"] {
        assert!(said.contains(expected), "{expected}: {said}");
    }
}

fn configured_address(agent: &Agent) -> SocketAddr {
    agent
        .pairing_terminal
        .serve_table
        .listen
        .expect("an address in the hub's config.toml")
}

fn serving_address(agent: &Agent) -> SocketAddr {
    agent.pairing_terminal.address.expect("a serving hub")
}

#[then("the hub listens on the address its config.toml gives")]
fn listens_on_configured(agent: &mut Agent) {
    assert_eq!(serving_address(agent), configured_address(agent));
}

#[then("the hub listens on another address than its config.toml gives")]
fn listens_elsewhere(agent: &mut Agent) {
    assert_ne!(serving_address(agent), configured_address(agent));
}

/// The status of a request to the serving hub that names `host`.
async fn status_for_host(agent: &Agent, host: &str) -> u16 {
    let address = serving_address(agent);
    let request = Request {
        method: "GET".to_owned(),
        path: "/api/v1/repositories".to_owned(),
        headers: vec![format!("Host: {host}:{{port}}")],
        default_host: false,
        body: String::new(),
    };
    blocking(move || exchange(address, &request).status).await
}

#[then(expr = "the hub answers a request for the host name {string}")]
async fn answers_host(agent: &mut Agent, host: String) {
    assert_eq!(status_for_host(agent, &host).await, 200, "{host}");
}

#[then(expr = "the hub refuses a request for the host name {string}")]
async fn refuses_host(agent: &mut Agent, host: String) {
    assert_eq!(status_for_host(agent, &host).await, 403, "{host}");
}

#[then("the hub's data folder holds no hub key")]
fn no_hub_key(agent: &mut Agent) {
    let data = testkit::daemon::data_dir(&hub_root(agent));
    assert!(
        data.is_dir(),
        "the daemon never started: {}",
        data.display()
    );
    for name in HUB_KEY_FILES {
        assert!(!data.join(name).exists(), "{name} in {}", data.display());
    }
}

/// The daemon's log, from the hub that started.
fn started_log(agent: &Agent) -> String {
    assert!(
        agent.pairing_terminal.refused_start.is_none(),
        "{:?}",
        agent.pairing_terminal.refused_start
    );
    let log = agent
        .pairing_terminal
        .hub_log
        .as_ref()
        .expect("the daemon's log");
    std::fs::read_to_string(log).expect("the daemon's log")
}

#[then(
    expr = "the hub still starts as the hub, and says hub mode stays on while {string} is paired"
)]
fn hub_mode_kept_on(agent: &mut Agent, hostname: String) {
    let said = started_log(agent);
    for expected in [
        "serving the hub's pairing",
        "hub mode stays on",
        hostname.as_str(),
        "paired",
        "Settings",
    ] {
        assert!(said.contains(expected), "{expected}: {said}");
    }
}

#[then("the hub starts as a machine that isn't a hub")]
fn starts_as_a_machine(agent: &mut Agent) {
    let text = started_log(agent);
    assert!(!text.contains("serving the hub's pairing"), "{text}");
}
