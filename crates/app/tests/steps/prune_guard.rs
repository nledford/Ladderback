//! Steps for `features/maintenance/prune_guard.feature`: a maintenance run
//! prunes against an engine that plays the worker's part. It works out its
//! plan and repacks, each taking the scenario's time on the clock, asks at
//! its first delete, and deletes only on yes (§7.3; §16, decision 161).
//! A paired machine's hub is a hub's pairing use case holding a connection
//! of this machine and, in some scenarios, of another one, whose
//! heartbeats say what it runs; the hub's answer to this machine's
//! heartbeat is what its leases heard (§7.3, layer 2; §16, decision 166).

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use async_trait::async_trait;
use cucumber::{given, then, when};
use domain::execution::{FailureReason, OperationStatus};
use domain::fleet::{
    AgentRecord, AgentStatus, Hostname, HubAddress, HubPin, PublicKey, Signature, TokenTls,
};
use domain::maintenance::{Grant, HeldLeases, LeaseEpoch, RepoGrants, RunningBackup};
use domain::planning::{
    AgentConfig, AgentSettings, GroupBy, Hooks, Maintenance, RepositoryConfig, RepositorySettings,
};
use domain::secrets::{SecretRef, SecretValue};
use domain::{
    AgentId, OperationId, RepositoryAlias, RepositoryId, SignedDuration, Timestamp, Uuid,
};
use futures::channel::oneshot;
use futures::future::BoxFuture;
use futures::{FutureExt, StreamExt};

use super::Agent;
use app::execution::Finished;
use app::fakes::{
    FakeClock, FakeRetention, FakeSecrets, FakeSigning, FakeSources, InMemoryAgentRegistry,
    InMemoryConfigStore, InMemoryIdentity, InMemoryLeaseCache, InMemoryOperationStore,
    SeededRandom,
};
use app::leases::MaintenanceLeases;
use app::maintenance::{MaintenanceError, MaintenanceKind, RunMaintenance};
use app::pairing::HubPairing;
use app::ports::{
    Clock, ConfigStore, Deletion, DeletionAsk, Engine, EngineError, EngineEvent, EngineResult,
    Identity, Job, JobControl, LeaseCache, OperationStore, PruneSummary, RunningJob, StoreError,
};
use app::secrets::ResolveRepository;

/// When every scenario starts.
fn start() -> Timestamp {
    Timestamp::from_second(1_800_000_000).expect("a time")
}

/// The repository every scenario prunes.
fn repository() -> RepositoryId {
    RepositoryId::parse(&"07".repeat(32)).expect("a repository ID")
}

fn alias() -> RepositoryAlias {
    RepositoryAlias::parse("demo").expect("an alias")
}

/// This machine, and the other one a scenario's hub holds a connection of.
fn this_machine() -> AgentId {
    AgentId::from_uuid(Uuid::from_u128(1))
}

fn other_machine() -> AgentId {
    AgentId::from_uuid(Uuid::from_u128(2))
}

/// The other machine's backup, to `repository` when it knows its ID.
fn backup(repository: Option<RepositoryId>) -> Vec<RunningBackup> {
    vec![RunningBackup {
        op_id: OperationId::from_uuid(Uuid::from_u128(0xbb)),
        repository,
    }]
}

/// What the other machine does, as its heartbeats tell the hub.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Other {
    /// Is backing up to the repository.
    BackingUp,
    /// Was backing up to it, and its backup finished.
    Finished,
    /// Was backing up to it, and went offline.
    WentOffline,
    /// Is backing up to another repository.
    BackingUpElsewhere,
    /// Is online, and its heartbeat doesn't say what it runs.
    Silent,
    /// Is backing up to a repository whose ID it doesn't know yet.
    BackingUpUnknown,
    /// Starts backing up to the repository while the prune repacks.
    StartsDuringRepack,
}

/// What a scenario sets up, and what its prune did.
#[derive(Default)]
pub struct PruneGuardWorld {
    /// Whether this machine is paired with a hub that granted it a lease
    /// on the repository for a week.
    paired: bool,
    /// The lease's length when it is shorter than a week.
    lease_lasts: Option<SignedDuration>,
    /// How long the prune takes to work out its plan.
    planning_takes: SignedDuration,
    /// How long the prune then takes to warm up and repack, before its
    /// first delete.
    repacking_takes: SignedDuration,
    /// Whether the run is canceled just as the prune asks.
    cancel_as_it_asks: bool,
    /// Whether the run is canceled while the daemon checks the lease, once
    /// the prune asked.
    cancel_during_check: bool,
    /// Whether the hub grants the repository again, at a new epoch, while
    /// the prune repacks.
    granted_again: bool,
    /// What another machine the hub holds a connection of does, if one.
    other: Option<Other>,
    /// How long ago this machine last heard its hub answer a heartbeat,
    /// when not just now.
    last_heard_ago: Option<SignedDuration>,
    deleted: Arc<AtomicBool>,
    finished: Option<Finished>,
    /// Why the prune was refused before it ran, if it was.
    refused: Option<MaintenanceError>,
    /// What the run stored.
    operations: Arc<InMemoryOperationStore>,
    operation: Option<OperationId>,
}

impl PruneGuardWorld {
    fn finished(&self) -> &Finished {
        self.finished.as_ref().expect("the prune has run")
    }
}

#[given("this machine is paired with a hub and holds the lease on the repository")]
fn paired_and_leased(agent: &mut Agent) {
    agent.prune_guard.paired = true;
}

#[given("the lease runs out while the prune repacks, after it worked out its plan")]
fn lease_runs_out(agent: &mut Agent) {
    agent.prune_guard.lease_lasts = Some(SignedDuration::from_mins(30));
    agent.prune_guard.planning_takes = SignedDuration::from_mins(10);
    agent.prune_guard.repacking_takes = SignedDuration::from_hours(3);
}

#[given("this machine isn't paired with a hub")]
fn not_paired(agent: &mut Agent) {
    agent.prune_guard.paired = false;
}

#[given("the prune is cancelled just as it asks whether it may delete")]
fn canceled_as_it_asks(agent: &mut Agent) {
    agent.prune_guard.cancel_as_it_asks = true;
}

#[given("the prune is cancelled while the daemon checks whether it may delete")]
fn canceled_during_check(agent: &mut Agent) {
    agent.prune_guard.cancel_during_check = true;
}

#[given(
    "the lease runs out while the prune repacks, and the hub grants this machine a new one before the prune asks"
)]
fn granted_again(agent: &mut Agent) {
    lease_runs_out(agent);
    agent.prune_guard.granted_again = true;
}

#[given("another machine is backing up to the repository")]
fn other_backing_up(agent: &mut Agent) {
    agent.prune_guard.other = Some(Other::BackingUp);
}

#[given("another machine was backing up to the repository, and its backup finished")]
fn other_finished(agent: &mut Agent) {
    agent.prune_guard.other = Some(Other::Finished);
}

#[given("another machine was backing up to the repository, and went offline")]
fn other_went_offline(agent: &mut Agent) {
    agent.prune_guard.other = Some(Other::WentOffline);
}

#[given("another machine is backing up to another repository")]
fn other_backing_up_elsewhere(agent: &mut Agent) {
    agent.prune_guard.other = Some(Other::BackingUpElsewhere);
}

#[given("another machine is online, and its heartbeat doesn't say what it runs")]
fn other_silent(agent: &mut Agent) {
    agent.prune_guard.other = Some(Other::Silent);
}

#[given("another machine is backing up to a repository whose ID it doesn't know yet")]
fn other_backing_up_unknown(agent: &mut Agent) {
    agent.prune_guard.other = Some(Other::BackingUpUnknown);
}

#[given("this machine last heard its hub answer a heartbeat 90 seconds ago")]
fn heard_long_ago(agent: &mut Agent) {
    agent.prune_guard.last_heard_ago = Some(SignedDuration::from_secs(90));
}

#[given("another machine starts backing up to the repository while the prune repacks")]
fn other_starts_during_repack(agent: &mut Agent) {
    agent.prune_guard.other = Some(Other::StartsDuringRepack);
    agent.prune_guard.repacking_takes = SignedDuration::from_mins(10);
}

/// A hub holding a connection of this machine, and of the other one when
/// the scenario has one, each heartbeat of which said what `other` runs;
/// approved machines both, as a hub lets go of any other.
async fn hub_with(other: Option<Other>, clock: Arc<FakeClock>) -> Arc<HubPairing> {
    let registry = Arc::new(InMemoryAgentRegistry::new());
    let now = clock.now();
    for (n, agent_id) in [(1, this_machine()), (2, other_machine())] {
        registry.hold(AgentRecord {
            agent_id,
            key: PublicKey::from_bytes([n; 32]).expect("a key"),
            hostname: Hostname::parse(&format!("machine-{n}")).expect("a hostname"),
            status: AgentStatus::Approved { at: now },
            counter: 1,
            last_seen: now,
            zone: None,
        });
    }
    let hub = Arc::new(HubPairing::new(
        registry,
        Arc::new(InMemoryIdentity::default()),
        Arc::new(FakeSigning),
        Arc::new(SeededRandom::new(1)),
        clock,
        "0.5.0-hub".to_owned(),
    ));
    hub.went_live(this_machine(), 1, Box::new(|| {})).await;
    hub.heard_running(this_machine(), Some(Vec::new()));
    let Some(other) = other else {
        return hub;
    };
    hub.went_live(other_machine(), 2, Box::new(|| {})).await;
    let running = match other {
        Other::BackingUp | Other::Finished | Other::WentOffline => Some(backup(Some(repository()))),
        Other::BackingUpElsewhere => Some(backup(Some(
            RepositoryId::parse(&"08".repeat(32)).expect("a repository ID"),
        ))),
        Other::BackingUpUnknown => Some(backup(None)),
        Other::Silent => None,
        Other::StartsDuringRepack => Some(Vec::new()),
    };
    if running.is_some() {
        hub.heard_running(other_machine(), running);
    }
    match other {
        Other::Finished => hub.heard_running(other_machine(), Some(Vec::new())),
        Other::WentOffline => hub.went_away(other_machine(), 2).await,
        _ => {}
    }
    hub
}

#[when("this machine prunes the repository")]
async fn prune(agent: &mut Agent) {
    let world = &mut agent.prune_guard;
    let clock = Arc::new(FakeClock::new(start()));
    let configs = Arc::new(InMemoryConfigStore::new());
    configs
        .save(&config())
        .await
        .expect("the configuration saves");
    let identity = Arc::new(InMemoryIdentity::default());
    let asked = Arc::new(AtomicBool::new(false));
    let (fire, cancel) = oneshot::channel::<()>();
    let (fire_as_it_asks, fire_during_check) = if world.cancel_during_check {
        (None, Some(fire))
    } else {
        (world.cancel_as_it_asks.then_some(fire), None)
    };
    let leases = Arc::new(MaintenanceLeases::new(
        identity.clone(),
        Arc::new(CanceledDuringCheck {
            kept: InMemoryLeaseCache::default(),
            asked: Arc::clone(&asked),
            fire: Mutex::new(fire_during_check),
        }),
        clock.clone(),
    ));
    if world.paired {
        let key = |byte| PublicKey::from_bytes([byte; 32]).expect("a key");
        identity
            .write_pin(&HubPin::new(
                key(7),
                TokenTls::Ca,
                key(9),
                HubAddress::parse("https://hub.example:9898").expect("an address"),
            ))
            .await
            .expect("the pin is kept");
        let lasts = world
            .lease_lasts
            .unwrap_or(SignedDuration::from_hours(7 * 24));
        leases
            .take(&granted(1, 1, start(), lasts), 0, start())
            .await
            .expect("the grants are kept");
        // The hub answers this machine's heartbeat, now or as long ago as
        // the scenario says.
        let hub = hub_with(world.other, Arc::clone(&clock)).await;
        leases.heard_holds(hub.prunes_held_for(this_machine()));
        if let Some(ago) = world.last_heard_ago {
            clock.advance(ago);
        }
    }
    // Past the first lease's end and the hub's hour of grace, the hub
    // grants the repository again, at the next epoch but one: another
    // machine may have held it meanwhile.
    let during_repack = if world.granted_again {
        let (leases, clock) = (Arc::clone(&leases), Arc::clone(&clock));
        Some(
            async move {
                let now = clock.now();
                leases
                    .take(
                        &granted(2, 3, now, SignedDuration::from_hours(7 * 24)),
                        0,
                        now,
                    )
                    .await
                    .expect("the new grant is kept");
            }
            .boxed(),
        )
    } else if world.other == Some(Other::StartsDuringRepack) {
        // The other machine's next heartbeat names its backup, and this
        // machine's next one hears the hub hold the prune it runs.
        let (leases, clock) = (Arc::clone(&leases), Arc::clone(&clock));
        Some(
            async move {
                let hub = hub_with(Some(Other::BackingUp), clock).await;
                leases.heard_holds(hub.prunes_held_for(this_machine()));
            }
            .boxed(),
        )
    } else {
        None
    };
    let use_case = RunMaintenance::new(
        Arc::new(Pruning {
            clock: clock.clone(),
            planning_takes: world.planning_takes,
            repacking_takes: world.repacking_takes,
            deleted: Arc::clone(&world.deleted),
            cancel_as_it_asks: Mutex::new(fire_as_it_asks),
            asked,
            during_repack: Mutex::new(during_repack),
        }),
        clock,
        Arc::clone(&world.operations) as Arc<dyn OperationStore>,
        ResolveRepository::new(Arc::new(FakeSecrets::default())),
        Arc::new(FakeRetention),
        configs,
        Arc::default(),
        this_machine(),
        "demo-host".to_owned(),
        Arc::new(FakeSources),
    )
    .with_leases(leases);
    let operation = OperationId::from_uuid(Uuid::from_u128(0xcc));
    world.operation = Some(operation);
    let assessed = match use_case
        .assess(operation, &alias(), MaintenanceKind::Prune)
        .await
    {
        Ok(assessed) => assessed,
        Err(refused) => {
            world.refused = Some(refused);
            return;
        }
    };
    let prepared = match use_case.start(assessed).await {
        Ok(prepared) => prepared,
        Err(refused) => {
            world.refused = Some(refused);
            return;
        }
    };
    // A cancel the prune fires as it asks; without one, the run is never
    // canceled.
    let cancel = async move {
        if cancel.await.is_err() {
            futures::future::pending::<()>().await;
        }
    };
    world.finished = Some(use_case.execute(prepared, cancel).await);
}

/// The hub's grants of `generation`: the repository at `epoch`, signed at
/// `at` and lasting `lasts`.
fn granted(generation: u64, epoch: u64, at: Timestamp, lasts: SignedDuration) -> RepoGrants {
    RepoGrants {
        generation,
        signed_at: at,
        grants: vec![Grant {
            repository: repository(),
            epoch: LeaseEpoch::new(epoch).expect("an epoch"),
            expires_at: at.checked_add(lasts).expect("a time"),
            signature: Signature::from_bytes([0; 64]).expect("a signature"),
        }],
    }
}

#[then("the prune deleted nothing")]
fn deleted_nothing(agent: &mut Agent) {
    assert!(
        !agent.prune_guard.deleted.load(Ordering::SeqCst),
        "the prune deleted packs"
    );
}

#[then("the prune failed because the lease lapsed before it deleted anything")]
fn failed_lease_lapsed(agent: &mut Agent) {
    assert_eq!(
        agent.prune_guard.finished().operation.status(),
        &OperationStatus::Failed {
            reason: FailureReason::LeaseLapsed
        }
    );
}

#[then("the prune was cancelled")]
fn was_canceled(agent: &mut Agent) {
    assert_eq!(
        agent.prune_guard.finished().operation.status(),
        &OperationStatus::Canceled
    );
}

#[then("the prune deleted what it planned")]
fn deleted_as_planned(agent: &mut Agent) {
    let world = &agent.prune_guard;
    assert!(
        world.deleted.load(Ordering::SeqCst),
        "the prune deleted nothing"
    );
    assert_eq!(
        world.finished().operation.status(),
        &OperationStatus::Succeeded
    );
}

#[then("the prune is held because another machine may be backing up to the repository")]
fn held(agent: &mut Agent) {
    assert!(
        matches!(agent.prune_guard.refused, Some(MaintenanceError::PruneHeld)),
        "the prune wasn't held: {:?}",
        agent.prune_guard.refused
    );
}

#[then("nothing ran")]
async fn nothing_ran(agent: &mut Agent) {
    let world = &agent.prune_guard;
    assert!(world.finished.is_none(), "the prune ran");
    assert!(
        !world.deleted.load(Ordering::SeqCst),
        "the prune deleted packs"
    );
    let operation = world.operation.expect("the prune was asked for");
    assert!(
        world
            .operations
            .get(operation)
            .await
            .expect("the store reads")
            .is_none(),
        "the prune was stored"
    );
}

/// The configuration: the one repository, maintained by this machine.
fn config() -> AgentConfig {
    let repository = RepositoryConfig {
        locations: Default::default(),
        id: Some(repository()),
        settings: RepositorySettings {
            repository: "/srv/demo/repo".to_owned(),
            repo_hot: None,
            password: SecretRef::Inline(SecretValue::new("demo-password")),
            options: BTreeMap::new(),
            options_hot: BTreeMap::new(),
            options_cold: BTreeMap::new(),
        },
        maintenance: Maintenance::Local,
        group_by: GroupBy::default(),
        hooks: Hooks::default(),
    };
    AgentConfig::new(
        AgentSettings::default(),
        Hooks::default(),
        BTreeMap::from([(alias(), repository)]),
        Vec::new(),
    )
    .expect("a valid configuration")
}

/// An engine that identifies the repository, lists no snapshots, and prunes
/// as the worker does: it works out its plan, repacks, asks at its first
/// delete, and deletes only on yes.
struct Pruning {
    clock: Arc<FakeClock>,
    planning_takes: SignedDuration,
    repacking_takes: SignedDuration,
    deleted: Arc<AtomicBool>,
    /// Fired as the prune asks, when the scenario cancels then.
    cancel_as_it_asks: Mutex<Option<oneshot::Sender<()>>>,
    /// Set as the prune asks.
    asked: Arc<AtomicBool>,
    /// What happens on the hub's side while the prune repacks.
    during_repack: Mutex<Option<BoxFuture<'static, ()>>>,
}

/// A lease cache that fires the run's cancel when it is read once the
/// prune asked: a cancel that comes while the daemon checks the lease.
struct CanceledDuringCheck {
    kept: InMemoryLeaseCache,
    asked: Arc<AtomicBool>,
    fire: Mutex<Option<oneshot::Sender<()>>>,
}

#[async_trait]
impl LeaseCache for CanceledDuringCheck {
    async fn load(&self) -> Result<HeldLeases, StoreError> {
        if self.asked.load(Ordering::SeqCst)
            && let Some(fire) = self
                .fire
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .take()
        {
            let _ = fire.send(());
        }
        self.kept.load().await
    }

    async fn save(&self, leases: &HeldLeases) -> Result<(), StoreError> {
        self.kept.save(leases).await
    }

    async fn save_pauses(&self, leases: &HeldLeases) -> Result<(), StoreError> {
        self.kept.save_pauses(leases).await
    }
}

struct Nothing;

#[async_trait]
impl JobControl for Nothing {
    async fn stop(&mut self) {}
}

#[async_trait]
impl Engine for Pruning {
    async fn run(&self, _op: OperationId, job: Job) -> Result<RunningJob, EngineError> {
        let events = match job {
            Job::Identify(_) => {
                futures::stream::iter([EngineEvent::Succeeded(EngineResult::Repository {
                    id: repository(),
                })])
                .boxed()
            }
            Job::Ls(_) => {
                futures::stream::iter([EngineEvent::Succeeded(EngineResult::Snapshots(Vec::new()))])
                    .boxed()
            }
            Job::Prune(_) => {
                let (clock, planning_takes, repacking_takes, deleted, asked) = (
                    Arc::clone(&self.clock),
                    self.planning_takes,
                    self.repacking_takes,
                    Arc::clone(&self.deleted),
                    Arc::clone(&self.asked),
                );
                let cancel = self
                    .cancel_as_it_asks
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .take();
                let during_repack = self
                    .during_repack
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .take();
                futures::stream::once(async move {
                    // The plan and the repack take their time; then the
                    // prune asks.
                    clock.advance(planning_takes);
                    clock.advance(repacking_takes);
                    if let Some(during_repack) = during_repack {
                        during_repack.await;
                    }
                    if let Some(cancel) = cancel {
                        let _ = cancel.send(());
                    }
                    asked.store(true, Ordering::SeqCst);
                    let (ask, answer) = DeletionAsk::new();
                    let asked = futures::stream::iter([EngineEvent::DeletionAsked(ask)]);
                    let ended = futures::stream::once(async move {
                        match answer.await {
                            Deletion::Permitted => {
                                deleted.store(true, Ordering::SeqCst);
                                EngineEvent::Succeeded(EngineResult::Prune(PruneSummary {
                                    repository: repository(),
                                    packs_marked: 1,
                                    packs_deleted: 1,
                                    packs_recovered: 0,
                                }))
                            }
                            Deletion::Refused => EngineEvent::Failed(FailureReason::LeaseLapsed),
                        }
                    });
                    asked.chain(ended)
                })
                .flatten()
                .boxed()
            }
            other => {
                return Err(EngineError::Unavailable {
                    detail: format!("the prune scenarios run no {other:?}"),
                });
            }
        };
        Ok(RunningJob {
            events,
            control: Box::new(Nothing),
        })
    }
}
