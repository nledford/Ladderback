//! Steps for `features/cli/service.feature`: the real `ladderback`, run as
//! the interop scenarios run it (`interop_steps::run`: a scratch home and
//! nothing of the developer's environment), with a `PATH` that holds the
//! stand-in service managers alone, so no real one is ever asked anything.
//! Which manager the program asks is this platform's: systemd's user manager
//! on Linux, launchd on macOS.

use std::assert_matches;
use std::path::PathBuf;

use cucumber::{given, then, when};
use platform::data_dir_lock::DataDirLock;
use testkit::service_manager::FakeServiceManagers;

use super::interop_steps::{ran, run};
use super::{Agent, LADDERBACK, events_steps};

#[derive(Debug, Default)]
pub(super) struct ServiceE2e {
    managers: Option<FakeServiceManagers>,
    /// How many calls the managers had when the scenario's command ran.
    calls_before: usize,
    /// The data folder's lock, held by the test's own process as a daemon
    /// holds it, until the scenario ends.
    holder: Option<DataDirLock>,
}

/// The service's name: the systemd unit or the launchd label.
pub(super) const NAME: &str = if cfg!(target_os = "macos") {
    "io.github.nledford.ladderback"
} else {
    "ladderback.service"
};

pub(super) fn managers(agent: &Agent) -> &FakeServiceManagers {
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

#[given("the user lingers")]
fn user_lingers(agent: &mut Agent) {
    managers(agent).linger();
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
    // Its words, after the program's path: each its own string in the plist,
    // and one line of the unit, which may go on with further words.
    let runs = if cfg!(target_os = "macos") {
        command
            .split(' ')
            .map(|word| format!("\t\t<string>{word}</string>\n"))
            .collect::<String>()
    } else {
        format!("\" {command}")
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

/// The program of the service the stand-in runs ends with `status` of
/// itself, and the manager starts it again as the file it loaded says: a
/// new process.
fn starts_again_after(agent: &Agent, status: u8) {
    let managers = managers(agent);
    let before = managers.process(NAME).expect("the service runs");
    managers.exits(NAME, status);
    let after = managers.process(NAME);
    assert!(
        after.is_some_and(|after| after != before),
        "the service manager left the service stopped after it exited with status {status}:\n{}",
        service_text(agent)
    );
}

/// The SIGTERM comes from the scenario, not from a service manager: the
/// stop systemd and launchd would make, sent by someone else.
#[when("a program other than the service manager sends the agent SIGTERM")]
async fn sigterm_from_another_program(agent: &mut Agent) {
    events_steps::Events::stop(agent).await;
}

/// The daemon's exit, as the service's: the status it ended with is the one
/// the stand-in reads its service file for.
#[then("the service manager starts the service again")]
fn manager_starts_again(agent: &mut Agent) {
    let stopped = agent.events.stopped();
    let status = stopped
        .code()
        .and_then(|code| u8::try_from(code).ok())
        .unwrap_or_else(|| panic!("the daemon ended with {stopped}, not a status"));
    starts_again_after(agent, status);
}

/// The program's exit that follows the manager's own stop: the manager has
/// let the service go, so nothing starts it.
#[then("the service manager does not start it again once its program has exited")]
fn stays_stopped(agent: &mut Agent) {
    managers(agent).exits(NAME, 0);
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
        assert_matches!((out, back), (Some(out), Some(back)) if out < back, "{since:?}");
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

#[then(expr = "the service manager waits {int} seconds for the service to stop before it kills it")]
fn manager_waits_to_kill(agent: &mut Agent, seconds: u64) {
    let text = service_text(agent);
    let wanted = if cfg!(target_os = "macos") {
        format!("<key>ExitTimeOut</key>\n\t<integer>{seconds}</integer>\n")
    } else {
        format!("\nTimeoutStopSec={seconds}\n")
    };
    assert!(
        text.contains(&wanted),
        "the service file lacks {wanted:?}:\n{text}"
    );
}

// Any exit the manager didn't ask for is followed by a start (#1035), and
// systemd marks the unit failed after a status other than 0 unless
// `SuccessExitStatus=` names it.
#[then(expr = "the service manager starts the service again when it exits with status {int}")]
fn manager_restarts_on(agent: &mut Agent, status: u8) {
    starts_again_after(agent, status);
    if cfg!(target_os = "linux") {
        let text = service_text(agent);
        let wanted = format!("\nSuccessExitStatus={status}\n");
        assert!(
            text.contains(&wanted),
            "the service file lacks {wanted:?}:\n{text}"
        );
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

/// What the hint at lingering names, in every language.
const LINGER_HINT: &str = "loginctl enable-linger";

/// Why a lingering step checks nothing on macOS: a launchd agent runs while
/// its user is logged out without lingering, so nothing about it is said.
const NO_LINGERING: &str = "lingering is systemd's: a launchd install never says how to linger";

#[then("standard output says how to keep the service running while logged out")]
fn stdout_says_how_to_linger(agent: &mut Agent) {
    if cfg!(target_os = "macos") {
        testkit::skip!("{NO_LINGERING}");
    }
    let stdout = &ran(agent).stdout;
    assert!(
        stdout.contains(LINGER_HINT),
        "standard output {stdout:?} doesn't say {LINGER_HINT:?}"
    );
}

#[then("standard output never says how to linger")]
fn stdout_never_says_how_to_linger(agent: &mut Agent) {
    if cfg!(target_os = "macos") {
        testkit::skip!("{NO_LINGERING}");
    }
    let stdout = &ran(agent).stdout;
    assert!(
        !stdout.contains(LINGER_HINT),
        "standard output {stdout:?} says {LINGER_HINT:?}"
    );
}

/// Writes the configuration the scenario's `ladderback` reads: one
/// repository, whose password is where `password` says.
fn one_repository(agent: &Agent, password: &str) {
    let folder = agent.scratch().join("config");
    std::fs::create_dir_all(&folder).expect("the configuration folder");
    std::fs::write(
        folder.join("config.toml"),
        format!("schema = 1\n\n[repositories.nas]\nrepository = \"/srv/demo-repo\"\n{password}\n"),
    )
    .expect("the configuration is written");
}

#[given("a repository reads its password from the keyring")]
fn password_in_the_keyring(agent: &mut Agent) {
    one_repository(agent, "password-keyring = \"ladderback/nas\"");
}

#[given("a repository reads its password from a file")]
fn password_in_a_file(agent: &mut Agent) {
    one_repository(
        agent,
        "password-file = \"/home/demo/.config/ladderback/nas.password\"",
    );
}

/// What the note on a keyring password names, in every language: the
/// source it suggests instead.
const KEYRING_NOTE: &str = "password-file";

#[then(
    "standard output says, on Linux alone, that a password in the keyring can't be read before login"
)]
fn stdout_says_keyring_waits_for_login(agent: &mut Agent) {
    let stdout = &ran(agent).stdout;
    if cfg!(target_os = "macos") {
        assert!(
            !stdout.contains(KEYRING_NOTE),
            "a launchd agent runs only after login, yet standard output {stdout:?} says {KEYRING_NOTE:?}"
        );
    } else {
        assert!(
            stdout.contains(KEYRING_NOTE),
            "standard output {stdout:?} doesn't say {KEYRING_NOTE:?}"
        );
    }
}

#[then("standard output never says that a password in the keyring can't be read before login")]
fn stdout_never_says_keyring_waits_for_login(agent: &mut Agent) {
    let stdout = &ran(agent).stdout;
    assert!(
        !stdout.contains(KEYRING_NOTE),
        "standard output {stdout:?} says {KEYRING_NOTE:?}"
    );
}

/// Holds the data folder `install-service` runs with
/// (`interop_steps::environment`) as a daemon does: the lock file names the
/// test's own process.
fn hold_the_data_folder(agent: &mut Agent) {
    let folder = agent.scratch().join("data");
    std::fs::create_dir_all(&folder).expect("the data folder");
    agent.service.holder = Some(DataDirLock::acquire(&folder).expect("the data folder is free"));
}

#[given("another Ladderback holds the data folder")]
fn another_holds_the_data_folder(agent: &mut Agent) {
    hold_the_data_folder(agent);
}

#[given("the service's own process holds the data folder")]
fn the_service_holds_the_data_folder(agent: &mut Agent) {
    hold_the_data_folder(agent);
    managers(agent).runs_as(NAME, std::process::id());
}

/// What `install-service` says of a data folder another Ladderback holds,
/// in every case.
const HELD_NOTE: &str = "is using the data folder";

#[then(
    "standard output names the process that holds the data folder and says to stop it for the service to take over"
)]
fn stdout_names_the_holder(agent: &mut Agent) {
    let stdout = &ran(agent).stdout;
    let named = format!(
        "another Ladderback, process {}, {HELD_NOTE}",
        std::process::id()
    );
    let stop = "stop that process for the service to take over";
    assert!(
        stdout.contains(&named) && stdout.contains(stop),
        "standard output {stdout:?} doesn't say {named:?} and {stop:?}"
    );
}

#[then("standard output never says that another Ladderback holds the data folder")]
fn stdout_never_names_a_holder(agent: &mut Agent) {
    let stdout = &ran(agent).stdout;
    assert!(
        !stdout.contains(HELD_NOTE),
        "standard output {stdout:?} says {HELD_NOTE:?}"
    );
}
