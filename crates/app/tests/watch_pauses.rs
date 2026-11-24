//! A paired machine watches its maintenance (technical design §7.3, §11;
//! §16, decision 158): each repository it is asked to maintain and holds no
//! lease on is paused, and once a pause has lasted 14 days the rules that
//! send `repo.maintenance_paused` are told, once, of the repository and
//! since when, with no operation.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use app::fakes::{
    FakeClock, FakeSecrets, InMemoryConfigStore, InMemoryIdentity, InMemoryLeaseCache,
    InMemoryOperationStore, RecordingNotifier,
};
use app::leases::{MaintenanceLeases, WatchPauses};
use app::notifications::Notify;
use app::ports::{ConfigStore, Identity};
use domain::fleet::{HubAddress, HubPin, PublicKey, TokenTls};
use domain::maintenance::{Grant, LeaseEpoch, RepoGrants};
use domain::notifications::{
    Channel, ChannelKind, ChannelName, EventKind, Notifications, Rule, RuleName,
};
use domain::planning::{
    AgentConfig, AgentSettings, GroupBy, Hooks, Maintenance, RepositoryConfig, RepositorySettings,
};
use domain::secrets::SecretRef;
use domain::{RepositoryAlias, RepositoryId, SignedDuration, Timestamp};

const DAY: SignedDuration = SignedDuration::from_hours(24);

fn start() -> Timestamp {
    Timestamp::from_second(1_800_000_000).expect("a time")
}

fn days(count: i32) -> SignedDuration {
    SignedDuration::from_hours(24 * i64::from(count))
}

fn id(digit: char) -> RepositoryId {
    RepositoryId::parse(&digit.to_string().repeat(64)).expect("an ID")
}

fn alias(name: &str) -> RepositoryAlias {
    RepositoryAlias::parse(name).expect("an alias")
}

/// A repository at `path`, of ID `id` when known, maintained as `maintenance`.
fn repository(path: &str, id: Option<RepositoryId>, maintenance: Maintenance) -> RepositoryConfig {
    RepositoryConfig {
        locations: Default::default(),
        id,
        settings: RepositorySettings {
            repository: path.to_owned(),
            repo_hot: None,
            password: SecretRef::Env("DEMO_PASSWORD".to_owned()),
            options: BTreeMap::new(),
            options_hot: BTreeMap::new(),
            options_cold: BTreeMap::new(),
        },
        maintenance,
        group_by: GroupBy::default(),
        hooks: Hooks::default(),
    }
}

/// `photos` (ID `a`) and `music` (ID `b`) maintained here, `films` (ID `c`)
/// left to the hub and `notes` with no ID maintained here, and a rule that
/// sends `repo.maintenance_paused` to the webhook "ops".
fn config() -> AgentConfig {
    let channel = ChannelName::parse("ops").expect("a channel name");
    let notifications = Notifications::new(
        BTreeMap::from([(
            channel.clone(),
            Channel::new(
                ChannelKind::Webhook,
                SecretRef::Env("DEMO_OPS_URL".to_owned()),
                None,
            )
            .expect("a channel"),
        )]),
        BTreeMap::from([(
            RuleName::parse("ops").expect("a rule name"),
            Rule::new(
                BTreeSet::from([EventKind::RepoMaintenancePaused]),
                channel,
                BTreeSet::new(),
                None,
            )
            .expect("a rule"),
        )]),
    )
    .expect("the rule sends to its channel");
    AgentConfig::new(
        AgentSettings::default(),
        Hooks::default(),
        BTreeMap::from([
            (
                alias("photos"),
                repository("/srv/photos", Some(id('a')), Maintenance::Local),
            ),
            (
                alias("music"),
                repository("/srv/music", Some(id('b')), Maintenance::Local),
            ),
            (
                alias("films"),
                repository("/srv/films", Some(id('c')), Maintenance::Hub),
            ),
            (
                alias("notes"),
                repository("/srv/notes", None, Maintenance::Local),
            ),
        ]),
        vec![],
    )
    .and_then(|config| config.with_notifications(notifications))
    .expect("a valid configuration")
}

struct Machine {
    watch: WatchPauses,
    leases: Arc<MaintenanceLeases>,
    clock: Arc<FakeClock>,
    notifier: Arc<RecordingNotifier>,
}

/// A paired machine on [`config`] that took the hub's grant of `photos` for
/// 7 days at the start.
async fn machine() -> Machine {
    let identity = Arc::new(InMemoryIdentity::default());
    let key = |byte| PublicKey::from_bytes([byte; 32]).expect("a key");
    let pin = HubPin::new(
        key(7),
        TokenTls::Ca,
        key(9),
        HubAddress::parse("https://hub.example:9898").expect("an address"),
    );
    identity.write_pin(&pin).await.expect("pinned");
    let clock = Arc::new(FakeClock::new(start()));
    let leases = Arc::new(MaintenanceLeases::new(
        identity,
        Arc::new(InMemoryLeaseCache::default()),
        clock.clone(),
    ));
    let grants = RepoGrants {
        generation: 1,
        signed_at: start(),
        grants: vec![Grant {
            repository: id('a'),
            epoch: LeaseEpoch::new(1).expect("an epoch"),
            expires_at: start().checked_add(days(7)).expect("a time"),
        }],
    };
    assert_eq!(
        leases.take(&grants, 0, start()).await.expect("taken"),
        vec![]
    );
    let store = InMemoryConfigStore::new();
    store.save(&config()).await.expect("saved");
    let secrets = Arc::new(FakeSecrets::new());
    secrets.set_env("DEMO_OPS_URL", "https://ops.example/hook");
    let notifier = Arc::new(RecordingNotifier::new());
    let config: Arc<dyn ConfigStore> = Arc::new(store);
    let notify = Arc::new(Notify::new(
        config.clone(),
        Arc::new(InMemoryOperationStore::new()),
        secrets,
        notifier.clone(),
    ));
    Machine {
        watch: WatchPauses::new(config, leases.clone(), notify),
        leases,
        clock,
        notifier,
    }
}

/// The repository and since when of each paused event delivered.
fn told(notifier: &RecordingNotifier) -> Vec<(Option<RepositoryAlias>, Timestamp)> {
    notifier
        .delivered()
        .into_iter()
        .map(|sent| {
            assert_eq!(sent.event.kind, EventKind::RepoMaintenancePaused);
            assert_eq!(sent.event.operation, None);
            (sent.event.repository, sent.event.at)
        })
        .collect()
}

#[tokio::test]
async fn each_repository_maintained_here_is_paused_while_it_holds_no_lease() {
    let machine = machine().await;
    assert_eq!(machine.watch.watch().await, vec![]);
    assert_eq!(machine.leases.paused(Some(&id('a'))).await, None);
    assert_eq!(machine.leases.paused(Some(&id('b'))).await, Some(start()));
    // Left to the hub: not this machine's to maintain.
    assert_eq!(machine.leases.paused(Some(&id('c'))).await, None);
    assert_eq!(machine.leases.paused(None).await, None);
    machine.clock.advance(days(9));
    assert_eq!(
        machine.leases.paused(Some(&id('a'))).await,
        Some(start().checked_add(days(7)).expect("a time"))
    );
}

#[tokio::test]
async fn a_pause_is_told_once_when_it_has_lasted_14_days() {
    let machine = machine().await;
    assert_eq!(machine.watch.watch().await, vec![]);
    machine
        .clock
        .advance(days(14) - SignedDuration::from_millis(1));
    assert_eq!(machine.watch.watch().await, vec![]);
    assert_eq!(told(&machine.notifier), vec![]);
    machine.clock.advance(SignedDuration::from_millis(1));
    assert_eq!(machine.watch.watch().await, vec![]);
    assert_eq!(
        told(&machine.notifier),
        vec![(Some(alias("music")), start())]
    );
    machine.clock.advance(days(7));
    assert_eq!(machine.watch.watch().await, vec![]);
    assert_eq!(
        told(&machine.notifier),
        vec![
            (Some(alias("music")), start()),
            (
                Some(alias("photos")),
                start().checked_add(days(7)).expect("a time")
            ),
        ]
    );
    machine.clock.advance(DAY);
    assert_eq!(machine.watch.watch().await, vec![]);
    assert_eq!(told(&machine.notifier).len(), 2);
}
