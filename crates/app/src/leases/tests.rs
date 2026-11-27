use std::sync::Arc;

use domain::fleet::{HubAddress, HubPin, KeyFile, PublicKey, TokenTls};
use domain::maintenance::{EpochFloor, Grant, HeldLeases, Pause};
use domain::{SignedDuration, Timestamp};

use super::*;
use crate::fakes::{FakeClock, InMemoryIdentity, InMemoryLeaseCache};
use crate::ports::IdentityError;

fn repository(digit: char) -> RepositoryId {
    RepositoryId::parse(&digit.to_string().repeat(64)).unwrap()
}

fn start() -> Timestamp {
    Timestamp::from_second(1_800_000_000).unwrap()
}

fn epoch(number: u64) -> LeaseEpoch {
    LeaseEpoch::new(number).unwrap()
}

async fn paired() -> Arc<InMemoryIdentity> {
    let identity = Arc::new(InMemoryIdentity::default());
    let key = |byte| PublicKey::from_bytes([byte; 32]).unwrap();
    let pin = HubPin::new(
        key(7),
        TokenTls::Ca,
        key(9),
        HubAddress::parse("https://hub.example:9898").unwrap(),
    );
    identity.write_pin(&pin).await.unwrap();
    identity
}

/// The hub's grant of `photos` at epoch 1 for 7 days, signed at `signed_at`.
fn grants(signed_at: Timestamp) -> RepoGrants {
    RepoGrants {
        generation: 1,
        signed_at,
        grants: vec![Grant {
            repository: repository('a'),
            epoch: epoch(1),
            expires_at: signed_at
                .checked_add(SignedDuration::from_hours(7 * 24))
                .unwrap(),
            signature: domain::fleet::Signature::from_bytes([0; 64]).unwrap(),
        }],
    }
}

struct Machine {
    leases: MaintenanceLeases,
    cache: Arc<InMemoryLeaseCache>,
    clock: Arc<FakeClock>,
}

fn machine(identity: Arc<InMemoryIdentity>) -> Machine {
    let cache = Arc::new(InMemoryLeaseCache::default());
    let clock = Arc::new(FakeClock::new(start()));
    Machine {
        leases: MaintenanceLeases::new(identity, cache.clone(), clock.clone()),
        cache,
        clock,
    }
}

#[tokio::test]
async fn a_machine_that_isnt_paired_maintains_without_a_lease() {
    let machine = machine(Arc::new(InMemoryIdentity::default()));
    assert_eq!(
        machine.leases.permits(Some(&repository('a'))).await,
        Ok(None)
    );
    assert_eq!(machine.leases.permits(None).await, Ok(None));
}

#[tokio::test]
async fn i1_a_paired_machine_with_no_grant_maintains_nothing() {
    let machine = machine(paired().await);
    assert_eq!(
        machine.leases.permits(Some(&repository('a'))).await,
        Err(NoLease::NoneHeld)
    );
}

#[tokio::test]
async fn i1_a_paired_machine_maintains_only_while_its_lease_holds() {
    let machine = machine(paired().await);
    let refused = machine
        .leases
        .take(&grants(start()), 0, start())
        .await
        .unwrap();
    assert_eq!(refused, vec![]);
    // It says which lease it holds, so a check later can tell the same
    // lease from one granted again since (decision 161).
    assert_eq!(
        machine.leases.permits(Some(&repository('a'))).await,
        Ok(Some(epoch(1)))
    );
    assert_eq!(
        machine.leases.permits(Some(&repository('b'))).await,
        Err(NoLease::NoneHeld)
    );
    // A repository whose ID its entry never recorded can't hold a lease.
    assert_eq!(machine.leases.permits(None).await, Err(NoLease::NoneHeld));
    machine.clock.advance(SignedDuration::from_hours(7 * 24));
    assert_eq!(
        machine.leases.permits(Some(&repository('a'))).await,
        Err(NoLease::RanOut)
    );
}

#[tokio::test]
async fn i1_a_pin_that_cant_be_read_maintains_nothing() {
    let machine = machine(Arc::new(InMemoryIdentity::unreadable(
        IdentityError::Damaged {
            file: KeyFile::HubPin,
        },
    )));
    assert_eq!(
        machine.leases.permits(Some(&repository('a'))).await,
        Err(NoLease::NoneHeld)
    );
}

#[tokio::test]
async fn grants_and_floors_are_kept_and_released() {
    let machine = machine(paired().await);
    machine
        .leases
        .take(&grants(start()), 0, start())
        .await
        .unwrap();
    assert_eq!(
        machine.leases.held().await.unwrap(),
        vec![(repository('a'), epoch(1))]
    );
    let kept = machine.cache.load().await.unwrap();
    assert_eq!(kept.holds(&repository('a'), start()), Ok(epoch(1)));
    machine
        .leases
        .released(&repository('a'), epoch(1))
        .await
        .unwrap();
    assert_eq!(machine.leases.held().await.unwrap(), vec![]);
    let floors: Vec<EpochFloor> = machine
        .cache
        .load()
        .await
        .unwrap()
        .floors()
        .map(|(_, floor)| *floor)
        .collect();
    assert_eq!(floors.len(), 1);
    assert_ne!(machine.cache.load().await.unwrap(), HeldLeases::default());
}

/// A wall clock set back after a grant doesn't lengthen the lease: the
/// machine counts at least the monotonic time that passed since it took
/// the grant, so its lease still runs out on the hub's time (I1).
#[tokio::test]
async fn i1_a_clock_set_back_doesnt_lengthen_a_lease() {
    let machine = machine(paired().await);
    machine.clock.advance(SignedDuration::from_hours(24));
    let asked_at = machine.clock.now();
    machine
        .leases
        .take(&grants(asked_at), 0, asked_at)
        .await
        .unwrap();
    machine.clock.advance(SignedDuration::from_hours(6 * 24));
    machine.clock.set_back(SignedDuration::from_hours(3 * 24));
    assert_eq!(
        machine.leases.permits(Some(&repository('a'))).await,
        Ok(Some(epoch(1)))
    );
    machine.clock.advance(SignedDuration::from_hours(24));
    assert_eq!(
        machine.leases.permits(Some(&repository('a'))).await,
        Err(NoLease::RanOut)
    );
}

/// A cache that kept a pause of `photos` since the start, not yet told.
async fn kept_pause(cache: &InMemoryLeaseCache) {
    let paused = HeldLeases::default().with_pauses([(
        repository('a'),
        Pause {
            since: start(),
            alerted: false,
        },
    )]);
    cache.save_pauses(&paused).await.unwrap();
}

/// A read notes nothing: the pause is noted where it begins, when
/// maintenance is turned on (#1671), or else by the watch, which keeps its
/// date.
#[tokio::test]
async fn a_paired_machine_notes_and_reads_its_pauses_now() {
    let machine = machine(paired().await);
    assert_eq!(machine.leases.paused(Some(&repository('a'))).await, None);
    assert_eq!(machine.cache.load().await.unwrap().pauses().count(), 0);
    assert_eq!(machine.leases.watch(&[repository('a')]).await, Ok(vec![]));
    machine.clock.advance(SignedDuration::from_hours(24));
    assert_eq!(
        machine.leases.paused(Some(&repository('a'))).await,
        Some(start())
    );
    assert_eq!(machine.leases.paused(None).await, None);
    machine.clock.advance(SignedDuration::from_hours(13 * 24));
    assert_eq!(
        machine.leases.watch(&[repository('a')]).await,
        Ok(vec![(repository('a'), start())])
    );
    assert_eq!(machine.leases.told(&repository('a'), start()).await, Ok(()));
    assert_eq!(
        machine
            .cache
            .load()
            .await
            .unwrap()
            .pauses()
            .collect::<Vec<_>>(),
        vec![(
            &repository('a'),
            &Pause {
                since: start(),
                alerted: true
            }
        )]
    );
    assert_eq!(machine.leases.watch(&[repository('a')]).await, Ok(vec![]));
}

#[tokio::test]
async fn a_machine_that_isnt_paired_is_never_paused_and_drops_what_it_noted() {
    let machine = machine(Arc::new(InMemoryIdentity::default()));
    kept_pause(&machine.cache).await;
    assert_eq!(machine.leases.paused(Some(&repository('a'))).await, None);
    machine.clock.advance(SignedDuration::from_hours(30 * 24));
    assert_eq!(machine.leases.watch(&[repository('a')]).await, Ok(vec![]));
    assert_eq!(machine.cache.load().await.unwrap().pauses().count(), 0);
}

/// A watch whose pin can't be read notes nothing, and says why in the log
/// rather than nothing at all (#1671).
#[tokio::test]
async fn a_pin_that_cant_be_read_notes_nothing_and_says_so_in_the_log() {
    let machine = machine(Arc::new(InMemoryIdentity::unreadable(
        IdentityError::Damaged {
            file: KeyFile::HubPin,
        },
    )));
    kept_pause(&machine.cache).await;
    assert_eq!(machine.leases.paused(Some(&repository('a'))).await, None);
    machine.clock.advance(SignedDuration::from_hours(30 * 24));
    crate::test_log::capture_the_log();
    assert_eq!(machine.leases.watch(&[repository('a')]).await, Ok(vec![]));
    assert_eq!(machine.cache.load().await.unwrap().pauses().count(), 1);
    assert_eq!(
        logged(),
        vec![(
            log::Level::Warn,
            "the maintenance watch noted no pause: this machine's hub pin can't be read \
             (Damaged { file: HubPin })"
                .to_owned()
        )]
    );
}

/// What `app::leases` logged since the capture began.
fn logged() -> Vec<(log::Level, String)> {
    crate::test_log::logged_by("app::leases")
}

/// A paired machine's watch says in one line what it found: how many of
/// the repositories it asks to maintain are paused (#1671).
#[tokio::test]
async fn a_paired_machines_watch_says_what_it_found_in_one_line() {
    let machine = machine(paired().await);
    machine
        .leases
        .take(&grants(start()), 0, start())
        .await
        .unwrap();
    crate::test_log::capture_the_log();
    assert_eq!(
        machine
            .leases
            .watch(&[repository('a'), repository('b')])
            .await,
        Ok(vec![])
    );
    assert_eq!(
        logged(),
        vec![(
            log::Level::Info,
            "the maintenance watch found 1 of the 2 repositories this machine asks to \
             maintain paused, for want of a lease from its hub"
                .to_owned()
        )]
    );
}

/// A machine that isn't paired has nothing to watch, and says nothing.
#[tokio::test]
async fn a_machine_that_isnt_paired_watches_without_a_line() {
    let machine = machine(Arc::new(InMemoryIdentity::default()));
    crate::test_log::capture_the_log();
    assert_eq!(machine.leases.watch(&[repository('a')]).await, Ok(vec![]));
    assert_eq!(logged(), vec![]);
}

/// A cache whose pauses can't be written, as a read-only database.
struct PausesUnwritable;

#[async_trait::async_trait]
impl LeaseCache for PausesUnwritable {
    async fn load(&self) -> Result<HeldLeases, StoreError> {
        Ok(HeldLeases::default())
    }

    async fn save(&self, _leases: &HeldLeases) -> Result<(), StoreError> {
        Ok(())
    }

    async fn save_pauses(&self, _leases: &HeldLeases) -> Result<(), StoreError> {
        Err(StoreError::Unavailable {
            detail: "read-only".to_owned(),
        })
    }
}

/// Turns maintenance of `photos` on (`maintain_here`) or off on a machine
/// keeping `leases`, as the Snapshots page's toggle does.
#[expect(clippy::result_large_err, reason = "the use case's own error")]
async fn turn(
    config: Arc<crate::fakes::InMemoryConfigStore>,
    leases: Arc<MaintenanceLeases>,
    maintain_here: bool,
) -> Result<domain::planning::Maintenance, crate::repositories::MaintainerError> {
    crate::repositories::ChangeMaintainer::new(
        config,
        Arc::new(crate::fakes::InMemoryOperationStore::new()),
        Arc::default(),
    )
    .with_leases(leases)
    .change(&RepositoryAlias::parse("photos").unwrap(), maintain_here)
    .await
}

/// A paired machine that turns maintenance on for a repository its hub
/// doesn't lease it notes the pause there and then, so every page says it
/// at once rather than after the next watch (#1671); the watch keeps that
/// date.
#[tokio::test]
async fn turning_maintenance_on_without_a_lease_notes_the_pause_at_once() {
    let machine = machine(paired().await);
    let leases = Arc::new(machine.leases);
    let config = photos(domain::planning::Maintenance::None).await;
    assert_eq!(
        turn(config, leases.clone(), true).await,
        Ok(domain::planning::Maintenance::Local)
    );
    assert_eq!(leases.paused(Some(&repository('a'))).await, Some(start()));
    machine.clock.advance(SignedDuration::from_hours(1));
    assert_eq!(leases.watch(&[repository('a')]).await, Ok(vec![]));
    assert_eq!(leases.paused(Some(&repository('a'))).await, Some(start()));
}

/// Turning maintenance on while holding a live lease notes no pause (I1:
/// a held lease is never shown paused).
#[tokio::test]
async fn turning_maintenance_on_with_a_lease_notes_no_pause() {
    let machine = machine(paired().await);
    machine
        .leases
        .take(&grants(start()), 0, start())
        .await
        .unwrap();
    let leases = Arc::new(machine.leases);
    let config = photos(domain::planning::Maintenance::None).await;
    turn(config, leases.clone(), true).await.unwrap();
    assert_eq!(leases.paused(Some(&repository('a'))).await, None);
    assert_eq!(machine.cache.load().await.unwrap().pauses().count(), 0);
}

/// Turning maintenance off drops the pause, so none is left behind for a
/// repository this machine no longer asks to maintain.
#[tokio::test]
async fn turning_maintenance_off_drops_its_pause() {
    let machine = machine(paired().await);
    let leases = Arc::new(machine.leases);
    let config = photos(domain::planning::Maintenance::None).await;
    turn(config.clone(), leases.clone(), true).await.unwrap();
    assert_eq!(machine.cache.load().await.unwrap().pauses().count(), 1);
    turn(config, leases.clone(), false).await.unwrap();
    assert_eq!(machine.cache.load().await.unwrap().pauses().count(), 0);
}

/// A machine that isn't paired is never paused, so turning maintenance on
/// notes nothing.
#[tokio::test]
async fn turning_maintenance_on_unpaired_notes_nothing() {
    let machine = machine(Arc::new(InMemoryIdentity::default()));
    let leases = Arc::new(machine.leases);
    let config = photos(domain::planning::Maintenance::None).await;
    turn(config, leases.clone(), true).await.unwrap();
    assert_eq!(machine.cache.load().await.unwrap().pauses().count(), 0);
}

/// A pause the toggle can't save leaves maintenance on, and the log says
/// so by the failure's code alone (I4); the next watch notes it.
#[tokio::test]
async fn a_pause_the_toggle_cant_note_leaves_maintenance_on_and_says_so() {
    let leases = Arc::new(MaintenanceLeases::new(
        paired().await,
        Arc::new(PausesUnwritable),
        Arc::new(FakeClock::new(start())),
    ));
    let config = photos(domain::planning::Maintenance::None).await;
    crate::test_log::capture_the_log();
    assert_eq!(
        turn(config.clone(), leases, true).await,
        Ok(domain::planning::Maintenance::Local)
    );
    let saved = config.load().await.unwrap();
    assert_eq!(
        saved
            .repository(&RepositoryAlias::parse("photos").unwrap())
            .map(|entry| entry.maintenance),
        Some(domain::planning::Maintenance::Local)
    );
    assert_eq!(
        crate::test_log::logged_by("app::repositories::maintainer"),
        vec![(
            log::Level::Warn,
            "maintenance was turned on or off, but its pause couldn't be noted \
             (store-unavailable); the next maintenance watch notes it"
                .to_owned()
        )]
    );
}

/// A cache that can't be read at all, whose failure names a value nobody
/// checked (I4).
struct Unreadable;

#[async_trait::async_trait]
impl LeaseCache for Unreadable {
    async fn load(&self) -> Result<HeldLeases, StoreError> {
        Err(StoreError::Unavailable {
            detail: "demo-canary-detail".to_owned(),
        })
    }

    async fn save(&self, _leases: &HeldLeases) -> Result<(), StoreError> {
        self.load().await.map(drop)
    }

    async fn save_pauses(&self, _leases: &HeldLeases) -> Result<(), StoreError> {
        self.load().await.map(drop)
    }
}

/// A configuration that can't be read.
struct ConfigUnreadable;

fn config_error() -> crate::ports::ConfigError {
    crate::ports::ConfigError::Unavailable {
        file: "config.toml".to_owned(),
        detail: "demo-canary-detail".to_owned(),
    }
}

#[async_trait::async_trait]
impl ConfigStore for ConfigUnreadable {
    async fn load(&self) -> Result<domain::planning::AgentConfig, crate::ports::ConfigError> {
        Err(config_error())
    }

    async fn save(
        &self,
        _config: &domain::planning::AgentConfig,
    ) -> Result<(), crate::ports::ConfigError> {
        Err(config_error())
    }

    async fn record_id(
        &self,
        _alias: &RepositoryAlias,
        _read: &domain::planning::RepositoryConfig,
        _id: &RepositoryId,
    ) -> Result<(), crate::ports::RecordError> {
        Err(crate::ports::RecordError::Store(config_error()))
    }
}

/// A configuration that keeps `photos` (ID `a`), asking `maintenance` of
/// it.
async fn photos(
    maintenance: domain::planning::Maintenance,
) -> Arc<crate::fakes::InMemoryConfigStore> {
    photos_with(maintenance, Some(repository('a'))).await
}

/// A configuration that keeps `photos`, of the ID `id` when it records
/// one, asking `maintenance` of it.
async fn photos_with(
    maintenance: domain::planning::Maintenance,
    id: Option<RepositoryId>,
) -> Arc<crate::fakes::InMemoryConfigStore> {
    use std::collections::BTreeMap;

    use domain::planning::{
        AgentConfig, AgentSettings, GroupBy, Hooks, RepositoryConfig, RepositorySettings,
    };
    use domain::secrets::SecretRef;
    let entry = RepositoryConfig {
        locations: Default::default(),
        id,
        settings: RepositorySettings {
            repository: "/srv/photos".to_owned(),
            repo_hot: None,
            password: SecretRef::Env("DEMO_PASSWORD".to_owned()),
            options: BTreeMap::new(),
            options_hot: BTreeMap::new(),
            options_cold: BTreeMap::new(),
        },
        maintenance,
        group_by: GroupBy::default(),
        hooks: Hooks::default(),
    };
    let config = AgentConfig::new(
        AgentSettings::default(),
        Hooks::default(),
        BTreeMap::from([(RepositoryAlias::parse("photos").unwrap(), entry)]),
        vec![],
    )
    .unwrap();
    let store = crate::fakes::InMemoryConfigStore::new();
    store.save(&config).await.unwrap();
    Arc::new(store)
}

/// The pause watch of a paired machine over `config` and `cache`.
async fn watching(config: Arc<dyn ConfigStore>, cache: Arc<dyn LeaseCache>) -> WatchPauses {
    let leases = Arc::new(MaintenanceLeases::new(
        paired().await,
        cache,
        Arc::new(FakeClock::new(start())),
    ));
    let notify = Arc::new(Notify::new(
        config.clone(),
        Arc::new(crate::fakes::InMemoryOperationStore::new()),
        Arc::new(crate::fakes::FakeSecrets::default()),
        Arc::new(crate::fakes::RecordingNotifier::new()),
    ));
    WatchPauses::new(config, leases, notify)
}

/// A watch whose lease cache can't be read notes nothing, and says so in
/// the log by the failure's code alone (#1671, I4).
#[tokio::test]
async fn a_watch_whose_cache_cant_be_read_says_so_in_the_log() {
    let watch = watching(
        photos(domain::planning::Maintenance::Local).await,
        Arc::new(Unreadable),
    )
    .await;
    crate::test_log::capture_the_log();
    assert_eq!(watch.watch().await, vec![]);
    assert_eq!(
        logged(),
        vec![(
            log::Level::Warn,
            "the maintenance watch noted no pause: the lease cache can't be read or written \
             (store-unavailable)"
                .to_owned()
        )]
    );
}

/// A watch that finds repositories maintained here that record no ID,
/// whose pause it can't note, says so in the log (#1671).
#[tokio::test]
async fn a_watch_says_in_the_log_what_it_maintains_with_no_id() {
    let watch = watching(
        photos_with(domain::planning::Maintenance::Local, None).await,
        Arc::new(InMemoryLeaseCache::default()),
    )
    .await;
    crate::test_log::capture_the_log();
    assert_eq!(watch.watch().await, vec![]);
    assert_eq!(
        logged(),
        vec![
            (
                log::Level::Warn,
                "the maintenance watch can't note a pause for 1 of the repositories maintained \
                 here: their entries record no repository ID"
                    .to_owned()
            ),
            (
                log::Level::Info,
                "the maintenance watch found 0 of the 0 repositories this machine asks to \
                 maintain paused, for want of a lease from its hub"
                    .to_owned()
            ),
        ]
    );
}

/// A machine that isn't paired is never paused, so a watch of an entry
/// that records no ID says nothing of it.
#[tokio::test]
async fn an_unpaired_watch_says_nothing_of_what_it_maintains_with_no_id() {
    let config: Arc<dyn ConfigStore> =
        photos_with(domain::planning::Maintenance::Local, None).await;
    let leases = Arc::new(MaintenanceLeases::new(
        Arc::new(InMemoryIdentity::default()),
        Arc::new(InMemoryLeaseCache::default()),
        Arc::new(FakeClock::new(start())),
    ));
    let notify = Arc::new(Notify::new(
        config.clone(),
        Arc::new(crate::fakes::InMemoryOperationStore::new()),
        Arc::new(crate::fakes::FakeSecrets::default()),
        Arc::new(crate::fakes::RecordingNotifier::new()),
    ));
    let watch = WatchPauses::new(config, leases, notify);
    crate::test_log::capture_the_log();
    assert_eq!(watch.watch().await, vec![]);
    assert_eq!(logged(), vec![]);
}

/// A watch whose configuration can't be read notes nothing, and says so in
/// the log (#1671).
#[tokio::test]
async fn a_watch_whose_configuration_cant_be_read_says_so_in_the_log() {
    let watch = watching(
        Arc::new(ConfigUnreadable),
        Arc::new(InMemoryLeaseCache::default()),
    )
    .await;
    crate::test_log::capture_the_log();
    assert_eq!(watch.watch().await, vec![]);
    assert_eq!(
        logged(),
        vec![(
            log::Level::Warn,
            "the maintenance watch noted no pause: the configuration can't be read".to_owned()
        )]
    );
}

#[tokio::test]
async fn i1_a_watch_keeps_the_lease_a_grant_took_and_the_grant_keeps_the_pauses() {
    let machine = machine(paired().await);
    kept_pause(&machine.cache).await;
    let refused = machine
        .leases
        .take(&grants(start()), 0, start())
        .await
        .unwrap();
    assert_eq!(refused, vec![]);
    assert_eq!(machine.cache.load().await.unwrap().pauses().count(), 1);
    // The grant clears the pause at once; the next watch drops it.
    assert_eq!(machine.leases.paused(Some(&repository('a'))).await, None);
    assert_eq!(machine.leases.watch(&[repository('a')]).await, Ok(vec![]));
    let kept = machine.cache.load().await.unwrap();
    assert_eq!(kept.pauses().count(), 0);
    assert_eq!(kept.leases().count(), 1);
}

/// A machine that isn't paired has no hub to hold its prune; a paired one
/// that heard no answer yet, or whose pin can't be read, holds them all
/// (§16, decision 166).
#[tokio::test]
async fn a_prune_is_held_by_a_hub_only_and_by_one_never_heard() {
    let lone = machine(Arc::new(InMemoryIdentity::default()));
    lone.leases.heard_holds(None);
    assert!(!lone.leases.prune_held(Some(&repository('a'))).await);
    assert!(!lone.leases.prune_held(None).await);

    let fresh = machine(paired().await);
    assert!(fresh.leases.prune_held(Some(&repository('a'))).await);

    let unreadable = machine(Arc::new(InMemoryIdentity::unreadable(
        IdentityError::Damaged {
            file: KeyFile::HubPin,
        },
    )));
    unreadable.leases.heard_holds(Some(Vec::new()));
    assert!(unreadable.leases.prune_held(Some(&repository('a'))).await);
}

/// The hub's latest answer holds the prunes it names, every prune when it
/// didn't say, and stands 90 seconds on either clock; a repository whose
/// ID isn't known is held.
#[tokio::test]
async fn the_hubs_latest_answer_holds_a_prune_while_it_stands() {
    let machine = machine(paired().await);
    machine.leases.heard_holds(Some(vec![repository('b')]));
    assert!(!machine.leases.prune_held(Some(&repository('a'))).await);
    assert!(machine.leases.prune_held(Some(&repository('b'))).await);
    assert!(machine.leases.prune_held(None).await);

    machine.leases.heard_holds(None);
    assert!(machine.leases.prune_held(Some(&repository('a'))).await);

    machine.leases.heard_holds(Some(Vec::new()));
    machine.clock.advance(SignedDuration::from_secs(89));
    assert!(!machine.leases.prune_held(Some(&repository('a'))).await);
    machine.clock.advance(SignedDuration::from_secs(1));
    assert!(machine.leases.prune_held(Some(&repository('a'))).await);

    machine.leases.heard_holds(Some(Vec::new()));
    machine.clock.suspend(SignedDuration::from_secs(90));
    assert!(
        machine.leases.prune_held(Some(&repository('a'))).await,
        "the wall clock passed 90 seconds while asleep"
    );

    machine.leases.heard_holds(Some(Vec::new()));
    machine.clock.set_back(SignedDuration::from_secs(1));
    assert!(
        machine.leases.prune_held(Some(&repository('a'))).await,
        "the wall clock went back"
    );
}
