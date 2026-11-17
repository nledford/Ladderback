//! Steps for the `@e2e` scenarios of `features/secrets/keyring_check.feature`:
//! the real `ladderback check-keyring`, run as the interop scenarios run it
//! (`interop_steps::run`: a scratch home and configuration folder, nothing
//! of the developer's environment). No scenario names a keyring entry: the
//! program would ask the real keyring of the computer the tests run on.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

use cucumber::{given, then};

use super::Agent;

fn config_file(agent: &Agent) -> PathBuf {
    let folder = agent.scratch().join("config");
    fs::create_dir_all(&folder).expect("the configuration folder");
    folder.join("config.toml")
}

/// The file the password command leaves when it runs.
fn mark(agent: &Agent) -> PathBuf {
    agent.scratch().join("password-command-ran")
}

#[given("a repository reads its password from a command that leaves a mark")]
fn password_from_marking_command(agent: &mut Agent) {
    let program = agent.scratch().join("demo-password");
    // Written while no child starts: a child forked meanwhile would hold the
    // file open for writing, and running it would fail with "text file busy".
    spawn_gate::hold(|| {
        fs::write(
            &program,
            format!(
                "#!/bin/sh\n: > '{}'\necho demo-password\n",
                mark(agent).display()
            ),
        )
        .expect("the password command is written");
        fs::set_permissions(&program, fs::Permissions::from_mode(0o755))
            .expect("the password command is made runnable");
    });
    fs::write(
        config_file(agent),
        format!(
            "schema = 1\n\n[repositories.nas]\nrepository = \"/srv/demo-repo\"\npassword-command = \"{}\"\n",
            program.display()
        ),
    )
    .expect("the configuration is written");
}

#[given("the configuration file isn't valid TOML")]
fn config_not_toml(agent: &mut Agent) {
    fs::write(config_file(agent), "schema = 1\n\n[repositories.nas\n")
        .expect("the configuration is written");
}

#[then("the repository's password command never ran")]
fn password_command_never_ran(agent: &mut Agent) {
    assert!(
        !mark(agent).exists(),
        "the password command ran: {} is there",
        mark(agent).display()
    );
}
