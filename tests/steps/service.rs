//! Steps for `features/cli/service.feature`: the real `ladderback`, run as
//! the interop scenarios run it (`interop_steps::run`: a scratch home and
//! nothing of the developer's environment), with a `PATH` that holds the
//! stand-in service managers alone, so no real one is ever asked anything.
//! Which manager the program asks is this platform's: systemd's user manager
//! on Linux, launchd on macOS.

use std::path::PathBuf;

use cucumber::{given, then};
use testkit::service_manager::FakeServiceManagers;

use super::interop_steps::{ran, run};
use super::{Agent, LADDERBACK};

#[derive(Debug, Default)]
pub(super) struct ServiceE2e {
    managers: Option<FakeServiceManagers>,
    /// How many calls the managers had when the scenario's command ran.
    calls_before: usize,
}

/// The service's name: the systemd unit or the launchd label.
const NAME: &str = if cfg!(target_os = "macos") {
    "io.github.nledford.ladderback"
} else {
    "ladderback.service"
};

fn managers(agent: &Agent) -> &FakeServiceManagers {
    agent
        .service
        .managers
        .as_ref()
        .expect("the service manager is a stand-in")
}

/// Where this platform's service file is, under the scenario's home.
fn service_file(agent: &Agent) -> PathBuf {
    let home = agent.scratch().join("home");
    if cfg!(target_os = "macos") {
        home.join("Library/LaunchAgents")
            .join(format!("{NAME}.plist"))
    } else {
        home.join(".config/systemd/user").join(NAME)
    }
}

fn service_text(agent: &Agent) -> String {
    let file = service_file(agent);
    std::fs::read_to_string(&file)
        .unwrap_or_else(|error| panic!("the service file {}: {error}", file.display()))
}

#[given("the service manager is a stand-in")]
fn manager_is_a_stand_in(agent: &mut Agent) {
    let managers = FakeServiceManagers::install(&agent.scratch().join("managers"));
    agent
        .interop
        .env
        .push(("PATH".to_owned(), managers.bin().display().to_string()));
    agent.service.managers = Some(managers);
}

#[given("the service is installed")]
async fn service_is_installed(agent: &mut Agent) {
    run(agent, &["install-service"]).await;
    let installed = ran(agent);
    assert!(
        installed.success,
        "the install failed: {}{}",
        installed.stdout, installed.stderr
    );
    agent.service.calls_before = managers(agent).commands().len();
}

#[given(expr = "the service manager refuses with {string}")]
fn manager_refuses(agent: &mut Agent, words: String) {
    managers(agent).refuse(&words);
}

/// The link a package manager puts on the `PATH`, under the scenario's
/// scratch.
fn package_link(agent: &Agent) -> PathBuf {
    agent.scratch().join("package/bin/ladderback")
}

#[given("the program is run by a package manager's link on the PATH")]
fn run_by_a_link(agent: &mut Agent) {
    let link = package_link(agent);
    let folder = link.parent().expect("the link's folder");
    std::fs::create_dir_all(folder).expect("the link's folder is made");
    std::os::unix::fs::symlink(LADDERBACK, &link).expect("the link is made");
    let rest = agent
        .interop
        .env
        .iter()
        .rev()
        .find(|(name, _)| name == "PATH")
        .map_or_default(|(_, value)| format!(":{value}"));
    agent
        .interop
        .env
        .push(("PATH".to_owned(), format!("{}{rest}", folder.display())));
    agent.interop.program = Some("ladderback".to_owned());
}

/// A service file the owner wrote by hand.
const BY_HAND: &str = "[Service]\nExecStart=/usr/local/bin/my-backups\n";

#[given("a service file that Ladderback did not write")]
fn service_file_by_hand(agent: &mut Agent) {
    let file = service_file(agent);
    std::fs::create_dir_all(file.parent().expect("the file's folder"))
        .expect("the file's folder is made");
    std::fs::write(&file, BY_HAND).expect("the file is written");
}

#[then("the service file is still the one Ladderback did not write")]
fn service_file_still_by_hand(agent: &mut Agent) {
    assert_eq!(service_text(agent), BY_HAND);
}

#[then(expr = "the service file runs this program's {string}")]
fn file_runs_program(agent: &mut Agent, command: String) {
    let text = service_text(agent);
    // The path this program finds itself at: as it was started, or with its
    // links resolved, as Linux gives it.
    let canonical = std::fs::canonicalize(LADDERBACK).expect("this program's path");
    assert!(
        text.contains(LADDERBACK) || text.contains(&*canonical.to_string_lossy()),
        "the service file doesn't run {LADDERBACK}:\n{text}"
    );
    let runs = if cfg!(target_os = "macos") {
        format!("<string>{command}</string>")
    } else {
        format!("\" {command}\n")
    };
    assert!(
        text.contains(&runs),
        "the service doesn't run {command}:\n{text}"
    );
}

#[then("the service file runs the program by the link")]
fn file_runs_the_link(agent: &mut Agent) {
    let text = service_text(agent);
    let link = package_link(agent).display().to_string();
    assert!(
        text.contains(&format!("{link}\"")) || text.contains(&format!("{link}<")),
        "the service file doesn't run {link}:\n{text}"
    );
}

#[then("the service runs")]
fn service_runs(agent: &mut Agent) {
    assert!(managers(agent).runs(NAME), "{}", managers(agent).calls());
}

#[then("the service does not run")]
fn service_does_not_run(agent: &mut Agent) {
    assert!(!managers(agent).runs(NAME), "{}", managers(agent).calls());
}

#[then("there is no service file")]
fn no_service_file(agent: &mut Agent) {
    let file = service_file(agent);
    assert!(!file.exists(), "{} is still there", file.display());
}

#[then("the service manager stopped the service before it started it again")]
fn stopped_then_started(agent: &mut Agent) {
    let since: Vec<String> = managers(agent).commands()[agent.service.calls_before..].to_vec();
    let position = |word: &str| since.iter().position(|call| call.contains(word));
    if cfg!(target_os = "macos") {
        let (out, back) = (position(" bootout "), position(" bootstrap "));
        assert!(
            matches!((out, back), (Some(out), Some(back)) if out < back),
            "{since:?}"
        );
    } else {
        // A restart stops a running unit before it starts it.
        assert!(
            position(" restart ladderback.service").is_some(),
            "{since:?}"
        );
    }
}

#[then(expr = "the service file never shows {string}")]
fn file_never_shows(agent: &mut Agent, text: String) {
    assert!(!service_text(agent).contains(&text));
}

#[then(expr = "the service manager never saw {string}")]
fn manager_never_saw(agent: &mut Agent, text: String) {
    let calls = managers(agent).calls();
    assert!(!calls.is_empty(), "the service manager was asked nothing");
    assert!(!calls.contains(&text), "{calls}");
}

#[then("the service keeps rustic's cache in a private folder")]
fn cache_is_private(agent: &mut Agent) {
    let text = service_text(agent);
    if cfg!(target_os = "macos") {
        // The agent's own cache folder, which it makes private (decision
        // 59): the plist moves it nowhere.
        assert!(!text.contains("EnvironmentVariables"), "{text}");
    } else {
        for line in [
            "CacheDirectory=ladderback\n",
            "CacheDirectoryMode=0700\n",
            "Environment=LADDERBACK_CACHE_DIR=%C/ladderback\n",
        ] {
            assert!(text.contains(line), "the unit lacks {line:?}:\n{text}");
        }
    }
}

#[then("the service manager was asked nothing")]
fn manager_asked_nothing(agent: &mut Agent) {
    let since = &managers(agent).commands()[agent.service.calls_before..];
    assert!(since.is_empty(), "{since:?}");
}

#[then(expr = "standard output says {string}")]
fn stdout_says(agent: &mut Agent, text: String) {
    let stdout = &ran(agent).stdout;
    assert!(
        stdout.contains(&text),
        "standard output {stdout:?} doesn't say {text:?}"
    );
}
