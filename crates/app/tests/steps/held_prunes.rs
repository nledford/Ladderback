//! Steps for the scenario of `features/maintenance/prune_guard.feature`
//! that runs the guard through the machines' own syncs: two machines
//! paired with one hub, each running the daemon's sync loop on a clock of
//! its own. The other machine's heartbeats name the backups its
//! `RunningBackups` holds; this machine's take the hub's leases and keep
//! the hub's answer in the `MaintenanceLeases` its prune reads (§7.3,
//! layer 2; §16, decision 166). Nothing here hands the hub or the leases
//! what a heartbeat would carry: a loop that drops its backups or the
//! hub's answer leaves the prune unheld.

use std::collections::BTreeMap;
use std::sync::Arc;

use api::sync::AgentFrame;
use app::fakes::{
    FakeClock, FakeRetention, FakeSecrets, FakeSources, InMemoryConfigStore, InMemoryLeaseCache,
    InMemoryOperationStore, InMemoryOutbox, LoopbackHubConnector,
};
use app::leases::MaintenanceLeases;
use app::maintenance::{MaintenanceError, MaintenanceKind, RunMaintenance};
use app::others_snapshots::OthersSnapshots;
use app::ports::{
    AgentRegistry as _, Clock as _, ConfigStore as _, Engine, EngineError, EngineEvent,
    EngineResult, Job, JobControl, RunningJob,
};
use app::running::{BackupRunning, RunningBackups};
use app::secrets::ResolveRepository;
use app::sync::SyncWithHub;
use async_trait::async_trait;
use cucumber::{given, then, when};
use domain::fleet::OutboxEpoch;
use domain::planning::{
    AgentConfig, AgentSettings, GroupBy, Hooks, Maintenance, RepositoryConfig, RepositorySettings,
};
use domain::secrets::{SecretRef, SecretValue};
use domain::{OperationId, RepositoryAlias, RepositoryId, Timestamp, Uuid};
use futures::StreamExt as _;

use super::sync_replay::Running;
use super::{Agent, Machine};

/// How many of a loop's waits a step moves through for one heartbeat: one
/// is due every 30 seconds, and the loop wakes every 2 between them.
const WAKES_PER_HEARTBEAT: usize = 40;

/// The repository the other machine backs up to, and the one it doesn't,
/// by alias, path and ID.
const REPOSITORIES: [(&str, &str, &str); 2] = [
    ("demo", "/srv/demo/repo", "07"),
    ("other", "/srv/other/repo", "08"),
];

fn alias(name: &str) -> RepositoryAlias {
    RepositoryAlias::parse(name).expect("an alias")
}

fn repository(name: &str) -> RepositoryId {
    let (_, _, digits) = REPOSITORIES
        .iter()
        .find(|(alias, _, _)| *alias == name)
        .expect("a repository of the scenario");
    RepositoryId::parse(&digits.repeat(32)).expect("a repository ID")
}

/// The two machines' syncs, this machine's leases and configuration, and
/// the other machine's backups.
#[derive(Default)]
pub(super) struct HeldWorld {
    this: Option<Syncing>,
    other: Option<Syncing>,
    leases: Option<Arc<MaintenanceLeases>>,
    config: Option<Arc<InMemoryConfigStore>>,
    /// The backups the other machine runs, which its heartbeats name.
    running: Arc<RunningBackups>,
    /// The other machine's backup, until it finishes.
    backup: Option<BackupRunning>,
}

/// One machine's sync loop and the connector it reaches the hub through.
struct Syncing {
    machine: Machine,
    connector: Arc<LoopbackHubConnector>,
    running: Running,
}

impl Syncing {
    fn heartbeats(&self) -> usize {
        self.connector
            .sent()
            .iter()
            .filter(|text| {
                matches!(
                    AgentFrame::from_text(text),
                    Ok(AgentFrame::Signed { kind, .. }) if kind == "Heartbeat"
                )
            })
            .count()
    }

    /// Moves the loop on to its next heartbeat, and until it has taken the
    /// hub's answer and waits again.
    async fn next_heartbeat(&self) {
        let before = self.heartbeats();
        for _ in 0..WAKES_PER_HEARTBEAT {
            if self.heartbeats() > before {
                break;
            }
            assert!(
                self.running.wake().await,
                "{}'s loop stopped",
                self.machine.hostname.as_str()
            );
        }
        self.running.settle().await;
        assert!(
            self.heartbeats() > before,
            "{} sent no heartbeat",
            self.machine.hostname.as_str()
        );
    }
}

/// This machine's configuration: both repositories, maintained here.
fn config() -> AgentConfig {
    let repositories = REPOSITORIES.map(|(name, path, _)| {
        (
            alias(name),
            RepositoryConfig {
                locations: Default::default(),
                id: Some(repository(name)),
                settings: RepositorySettings {
                    repository: path.to_owned(),
                    repo_hot: None,
                    password: SecretRef::Inline(SecretValue::new("demo-password")),
                    options: BTreeMap::new(),
                    options_hot: BTreeMap::new(),
                    options_cold: BTreeMap::new(),
                },
                maintenance: Maintenance::Local,
                group_by: GroupBy::default(),
                hooks: Hooks::default(),
            },
        )
    });
    AgentConfig::new(
        AgentSettings::default(),
        Hooks::default(),
        BTreeMap::from(repositories),
        Vec::new(),
    )
    .expect("a valid configuration")
}

#[given("this machine and another one are approved by their hub and keep syncing with it")]
async fn both_syncing(agent: &mut Agent) {
    let world = &mut agent.pairing;
    let this = Machine::new(1, "laptop");
    let other = Machine::new(2, "desk");
    for machine in [&this, &other] {
        world.new_token().await;
        world.pair_and_keep(machine).await;
        assert!(matches!(world.outcome, Some(Ok(_))), "{:?}", world.outcome);
        let hub = world.hub();
        let waiting = hub.registry.agent(machine.agent_id).await.unwrap().unwrap();
        hub.registry
            .approve(machine.agent_id, waiting.key, hub.clock.now())
            .await
            .unwrap()
            .unwrap();
    }

    let start = Timestamp::from_second(1_800_000_000).unwrap();
    let clock = Arc::new(FakeClock::new(start));
    let configs = Arc::new(InMemoryConfigStore::new());
    configs
        .save(&config())
        .await
        .expect("the configuration saves");
    let leases = Arc::new(MaintenanceLeases::new(
        this.identity.clone(),
        Arc::new(InMemoryLeaseCache::default()),
        clock.clone(),
    ));
    // Its listings found no other machine's snapshots, so the hub grants
    // it both repositories.
    let others = Arc::new(OthersSnapshots::default());
    for (name, _, _) in REPOSITORIES {
        others.listed(repository(name), this.agent_id, start, &[]);
    }
    let connector = world.connector_to(world.hub());
    let sync = SyncWithHub::new(
        this.connecting(connector.clone()),
        Arc::new(InMemoryOutbox::new(OutboxEpoch::from_uuid(
            Uuid::from_u128(0xf1),
        ))),
        clock.clone(),
        this.agent_id,
        this.hostname.clone(),
        this.zone.clone(),
    )
    .with_leases(
        configs.clone(),
        leases.clone(),
        Arc::new(InMemoryOperationStore::default()),
        others,
        Arc::new(app::fakes::InMemoryDriveFlush::default()),
    )
    .with_running(Arc::new(RunningBackups::new()));
    world.held.this = Some(Syncing {
        machine: this,
        connector,
        running: Running {
            clock: clock.clone(),
            task: tokio::spawn(async move { sync.run().await }),
        },
    });
    world.held.leases = Some(leases);
    world.held.config = Some(configs);

    let clock = Arc::new(FakeClock::new(start));
    let connector = world.connector_to(world.hub());
    let sync = SyncWithHub::new(
        other.connecting(connector.clone()),
        Arc::new(InMemoryOutbox::new(OutboxEpoch::from_uuid(
            Uuid::from_u128(0xf2),
        ))),
        clock.clone(),
        other.agent_id,
        other.hostname.clone(),
        other.zone.clone(),
    )
    .with_running(Arc::clone(&world.held.running));
    world.held.other = Some(Syncing {
        machine: other,
        connector,
        running: Running {
            clock,
            task: tokio::spawn(async move { sync.run().await }),
        },
    });
}

#[given("this machine's sync took the hub's leases on the repository and on another repository")]
async fn took_leases(agent: &mut Agent) {
    let held = &agent.pairing.held;
    let leases = held.leases.as_ref().expect("this machine syncs");
    let this = held.this.as_ref().expect("this machine syncs");
    for _ in 0..WAKES_PER_HEARTBEAT {
        this.running.settle().await;
        let mut granted = true;
        for (name, _, _) in REPOSITORIES {
            granted &= matches!(leases.permits(Some(&repository(name))).await, Ok(Some(_)));
        }
        if granted {
            return;
        }
        assert!(this.running.wake().await, "this machine's loop stopped");
    }
    panic!("this machine's sync never took both leases");
}

#[given("the other machine is backing up to the repository")]
async fn other_backing_up(agent: &mut Agent) {
    let held = &mut agent.pairing.held;
    held.backup = Some(held.running.begin(
        OperationId::from_uuid(Uuid::from_u128(0xbb)),
        Some(repository("demo")),
    ));
}

#[when("the other machine's backup finishes")]
async fn other_finishes(agent: &mut Agent) {
    agent.pairing.held.backup = None;
}

#[when("each machine's next heartbeat reaches the hub")]
async fn next_heartbeats(agent: &mut Agent) {
    let held = &agent.pairing.held;
    // The other machine's first, so this machine's answer follows it.
    held.other
        .as_ref()
        .expect("the other machine syncs")
        .next_heartbeat()
        .await;
    held.this
        .as_ref()
        .expect("this machine syncs")
        .next_heartbeat()
        .await;
}

/// This machine's prune of the repository `name` assessed, as a person
/// starts one: refused, or not.
async fn prune(agent: &mut Agent, name: &str) -> Result<(), MaintenanceError> {
    let held = &agent.pairing.held;
    let this = held.this.as_ref().expect("this machine syncs");
    let use_case = RunMaintenance::new(
        Arc::new(Identifies),
        this.running.clock.clone(),
        Arc::new(InMemoryOperationStore::default()),
        ResolveRepository::new(Arc::new(FakeSecrets::default())),
        Arc::new(FakeRetention),
        held.config.clone().expect("this machine's configuration"),
        Arc::default(),
        this.machine.agent_id,
        "laptop".to_owned(),
        Arc::new(FakeSources),
    )
    .with_leases(held.leases.clone().expect("this machine's leases"));
    use_case
        .assess(
            OperationId::from_uuid(Uuid::from_u128(0xcc)),
            &alias(name),
            MaintenanceKind::Prune,
        )
        .await
        .map(|_| ())
}

#[then(
    "this machine's prune of the repository is held because another machine may be backing up to it"
)]
async fn held(agent: &mut Agent) {
    let assessed = prune(agent, "demo").await;
    assert!(
        matches!(assessed, Err(MaintenanceError::PruneHeld)),
        "the prune wasn't held: {assessed:?}"
    );
}

#[then(regex = r"^this machine's prune of the (repository|other repository) isn't held$")]
async fn not_held(agent: &mut Agent, which: String) {
    let name = if which == "repository" {
        "demo"
    } else {
        "other"
    };
    let assessed = prune(agent, name).await;
    assert!(assessed.is_ok(), "the prune was refused: {assessed:?}");
}

/// An engine that names each repository by its path, and lists no
/// snapshots in it.
struct Identifies;

struct Nothing;

#[async_trait]
impl JobControl for Nothing {
    async fn stop(&mut self) {}
}

#[async_trait]
impl Engine for Identifies {
    async fn run(&self, _op: OperationId, job: Job) -> Result<RunningJob, EngineError> {
        let result = match job {
            Job::Identify(access) => {
                let (name, _, _) = REPOSITORIES
                    .iter()
                    .find(|(_, path, _)| *path == access.repository)
                    .expect("a repository of the scenario");
                EngineResult::Repository {
                    id: repository(name),
                }
            }
            Job::Ls(_) => EngineResult::Snapshots(Vec::new()),
            other => {
                return Err(EngineError::Unavailable {
                    detail: format!("a prune's assessment runs no {other:?}"),
                });
            }
        };
        Ok(RunningJob {
            events: futures::stream::iter([EngineEvent::Succeeded(result)]).boxed(),
            control: Box::new(Nothing),
        })
    }
}
