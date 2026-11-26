//! The leases a paired machine maintains under, and the backups its
//! heartbeats name, as the daemon wires them (§7.3, I1; §16, decisions 151
//! and 166).

use app::ports::Identity as _;
use domain::fleet::{HubAddress, HubPin, PublicKey, TokenTls};
use domain::fleet::{SIGNATURE_BYTES, Signature};
use domain::maintenance::{Grant, LeaseEpoch, NoLease, RepoGrants};
use domain::{RepositoryId, SignedDuration};
use platform::SystemClock;
use platform::identity::FileIdentity;

use super::support::{own_folders, runner_at_a_gate};
use super::*;

/// The grants the sync with the hub took while the machine's wall clock
/// read 10 days later than it does now, the clock set back since, run out
/// for its maintenance on the monotonic time that passed: the sync and the
/// maintenance judge a lease by one floor (#1498).
#[tokio::test]
async fn i1_a_clock_set_back_after_the_syncs_take_doesnt_lengthen_the_lease_maintenance_reads() {
    let scratch = tempfile::tempdir().expect("a scratch dir");
    let (config, data) = own_folders(scratch.path());
    let key = PublicKey::from_bytes([7; 32]).expect("a key");
    let hub = HubAddress::parse("https://hub.example:9898").expect("an address");
    FileIdentity::in_data_dir(&data)
        .write_pin(&HubPin::new(key, TokenTls::Ca, key, hub))
        .await
        .expect("the machine is paired");
    let daemon = daemon_at(Some(config), Some(data), None)
        .await
        .map_err(|refused| refused.message)
        .expect("the daemon starts");
    let repository = RepositoryId::parse(&"a".repeat(64)).expect("an ID");
    // Asked 10 days ahead of the wall clock now, for a lease of 10 ms.
    let asked_at = SystemClock
        .now()
        .checked_add(SignedDuration::from_hours(10 * 24))
        .expect("a time");
    let grants = RepoGrants {
        generation: 1,
        signed_at: asked_at,
        grants: vec![Grant {
            repository: repository.clone(),
            epoch: LeaseEpoch::new(1).expect("an epoch"),
            expires_at: asked_at
                .checked_add(SignedDuration::from_millis(10))
                .expect("a time"),
            signature: Signature::from_bytes([0; SIGNATURE_BYTES]).expect("a signature"),
        }],
    };
    daemon
        .background
        .leases()
        .take(&grants, 0, asked_at)
        .await
        .expect("the grants are kept");
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    let maintenance = daemon
        .background
        .maintenance_reads()
        .expect("the daemon maintains under leases");
    assert_eq!(
        maintenance.permits(Some(&repository)).await,
        Err(NoLease::RanOut)
    );
}

/// The backups the daemon hands its sync with a hub are its runner's: a
/// sync given a list of its own would name no backup, and the hub would
/// hold no other machine's prune (§16, decision 166).
#[tokio::test]
async fn the_sync_names_the_backups_the_daemons_runner_runs() {
    let scratch = tempfile::tempdir().expect("a scratch dir");
    let (config, data) = own_folders(scratch.path());
    let daemon = daemon_at(Some(config), Some(data), None)
        .await
        .map_err(|refused| refused.message)
        .expect("the daemon starts");
    assert!(Arc::ptr_eq(
        &daemon.background.running(),
        &daemon.background.runner().running
    ));
}

/// A backup the runner runs is among its backups from before its engine
/// is asked until it ends, so the heartbeats name it while it runs (§16,
/// decision 166).
#[tokio::test]
async fn a_backup_the_runner_runs_is_among_its_backups_while_it_runs() {
    let (runner, _entered, open_gate, _scratch) = runner_at_a_gate().await;
    open_gate.send(true).expect("the gate opens");
    let id = runner
        .start_backup("home".into(), Trigger::Manual)
        .await
        .expect("the plan starts");
    let operation = operation_of(&id).expect("the answer is an operation ID");
    // The engine parks, so the backup runs until the test ends.
    let named = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while !runner
            .running
            .listed()
            .is_some_and(|running| running.iter().any(|backup| backup.op_id == operation))
        {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await;
    assert!(named.is_ok(), "the running backup was never named");
}
