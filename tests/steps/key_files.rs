//! Steps for `features/fleet/pairing.feature`: the key files in the data
//! folder, and a backup that leaves the secret ones out (§16, decisions 120
//! and 122). The snapshot is read with rustic itself.

use std::fs;

use cucumber::{given, then};
use domain::fleet::{COUNTER_LOCK_FILE, KeyFile, LEFT_OUT_OF_BACKUPS};

use super::Agent;
use super::backup_steps::names_in_newest;
use super::events_steps::{Events, save_daemon_config};

/// What a rotation stages beside the hub key, which the backup leaves out
/// with it.
const STAGED: &str = "hub-key.new";

/// The rest of the data folder the snapshot holds: the key files that hold
/// nothing secret, and the daemon's own database.
const KEPT: [&str; 5] = [
    "hub-recovery-key",
    "hub-pin",
    "connection-counter",
    COUNTER_LOCK_FILE,
    "ladderback.db",
];

/// Canaries, never real keys (§13.9): what a file holds doesn't matter to
/// a backup, only its name.
#[given(
    "the plan's source is Ladderback's data folder, which holds the hub's and the agent's key files"
)]
async fn the_data_folder_is_the_source(agent: &mut Agent) {
    let data = Events::data_dir(agent);
    for file in KeyFile::ALL {
        fs::write(data.join(file.file_name()), format!("canary {file}\n")).expect("a key file");
    }
    fs::write(data.join(COUNTER_LOCK_FILE), "").expect("the counter's lock");
    fs::write(data.join(STAGED), "canary staged\n").expect("a staged key");
    agent.backup.source = Some(data);
    save_daemon_config(agent, None).await;
}

#[then(
    "the backup's snapshot leaves out the hub key, the hub's TLS key, the recovery seed and the agent key"
)]
async fn leaves_out_the_secret_ones(agent: &mut Agent) {
    let names = names_in_newest(agent).await;
    let mut secret: Vec<&str> = LEFT_OUT_OF_BACKUPS
        .iter()
        .map(|file| file.file_name())
        .collect();
    secret.push(STAGED);
    for name in secret {
        assert!(
            !names.iter().any(|held| held == name),
            "the snapshot holds {name}: {names:?}"
        );
    }
}

#[then("the backup's snapshot holds the rest of the data folder")]
async fn holds_the_rest(agent: &mut Agent) {
    let names = names_in_newest(agent).await;
    for name in KEPT {
        assert!(
            names.iter().any(|held| held == name),
            "the snapshot lacks {name}: {names:?}"
        );
    }
}
