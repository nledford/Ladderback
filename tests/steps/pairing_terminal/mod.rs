//! Steps for `features/fleet/pairing_from_a_terminal.feature`,
//! `features/fleet/approving_from_a_terminal.feature`,
//! `features/fleet/hub_status_from_a_terminal.feature`, the `@e2e`
//! scenarios of `features/fleet/approving_on_the_hubs_page.feature` and
//! `features/fleet/following_the_hub.feature` and the terminal scenarios of
//! `features/fleet/machine_zone.feature`: the hub's
//! `ladderback pairing-token`, `ladderback serve --hub` and the routes its
//! page approves through, and a machine's `ladderback pair` and
//! `ladderback hub-status`, each a real process on folders of its own, and
//! the scenarios of `features/fleet/hub_keys_at_start.feature`.

use std::io::Write as _;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use testkit::children::Owned;

use super::{Agent, LADDERBACK, blocking};

mod hub;
mod hub_page;
mod hub_terminal;
mod machine_terminal;

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
    /// The time zone the machine's commands run in, through `TZ`; this
    /// computer's when none.
    machine_zone: Option<String>,
    /// The time zone the hub's terminal commands run in, through `TZ`; this
    /// computer's when none.
    hub_zone: Option<String>,
    said: Option<Said>,
    /// The status and body of the hub's answer to the page's last request.
    page_answer: Option<(u16, String)>,
    /// What a hub that didn't start said before it ended.
    refused_start: Option<String>,
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
    blocking(move || run(&root, &args, stdin, None)).await
}

/// [`ladderback`] on the machine's folders, in the machine's time zone.
async fn on_machine(agent: &Agent, args: &[&str], stdin: Option<String>) -> Said {
    let (root, zone) = (
        machine_root(agent),
        agent.pairing_terminal.machine_zone.clone(),
    );
    let args: Vec<String> = args.iter().map(|arg| (*arg).to_owned()).collect();
    blocking(move || run(&root, &args, stdin, zone.as_deref())).await
}

/// [`ladderback`] on the hub's folders, in the hub's terminal's time zone.
async fn on_hub(agent: &Agent, args: &[&str]) -> Said {
    let (root, zone) = (hub_root(agent), agent.pairing_terminal.hub_zone.clone());
    let args: Vec<String> = args.iter().map(|arg| (*arg).to_owned()).collect();
    blocking(move || run(&root, &args, None, zone.as_deref())).await
}

fn run(root: &Path, args: &[String], stdin: Option<String>, zone: Option<&str>) -> Said {
    let mut command = Command::new(LADDERBACK);
    testkit::daemon::isolate(&mut command, root);
    if let Some(zone) = zone {
        command.env("TZ", zone);
    }
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

fn machine_key(agent: &Agent) -> &str {
    agent
        .pairing_terminal
        .machine_key
        .as_deref()
        .expect("the machine showed its key")
}
