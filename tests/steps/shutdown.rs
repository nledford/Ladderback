//! Steps for `features/execution/shutdown.feature`: the real `ladderback
//! serve` process, told to stop with SIGTERM, and the data folder it leaves.

use cucumber::{then, when};

use super::Agent;
use super::events_steps::Events;

#[when("the daemon is told to stop")]
async fn told_to_stop(agent: &mut Agent) {
    Events::stop(agent).await;
}

#[then(expr = "the daemon has exited with status {int}")]
fn exited_with(agent: &mut Agent, code: i32) {
    let status = agent.events.stopped();
    assert_eq!(status.code(), Some(code), "the daemon ended with {status}");
}

#[then(
    expr = "the operation log is closed, with no {string} or {string} file of it left in the data folder"
)]
fn the_log_is_closed(agent: &mut Agent, wal: String, shm: String) {
    let data = Events::data_dir(agent);
    let database = data.join("ladderback.db");
    assert!(database.is_file(), "{} is there", database.display());
    let left: Vec<String> = [wal, shm]
        .iter()
        .map(|suffix| data.join(format!("ladderback.db{suffix}")))
        .filter(|path| path.exists())
        .map(|path| path.display().to_string())
        .collect();
    assert!(
        left.is_empty(),
        "the daemon left the operation log open: {left:?} outlived it"
    );
}
