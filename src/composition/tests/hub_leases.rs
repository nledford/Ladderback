//! The leases a paired machine maintains under, as the daemon wires them
//! (§7.3, I1; §16, decision 151).

use app::ports::Identity as _;
use domain::fleet::{HubAddress, HubPin, PublicKey, TokenTls};
use domain::maintenance::{Grant, LeaseEpoch, NoLease, RepoGrants};
use domain::{RepositoryId, SignedDuration};
use platform::SystemClock;
use platform::identity::FileIdentity;

use super::support::own_folders;
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
        .expect("a build with the hub maintains under leases");
    assert_eq!(
        maintenance.permits(Some(&repository)).await,
        Err(NoLease::RanOut)
    );
}
