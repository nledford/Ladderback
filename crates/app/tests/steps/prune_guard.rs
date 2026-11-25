//! Steps for `features/maintenance/prune_guard.feature`: a maintenance run
//! prunes against an engine that plays the worker's part. It works out its
//! plan and repacks, each taking the scenario's time on the clock, asks at
//! its first delete, and deletes only on yes (§7.3; §16, decision 161).

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use async_trait::async_trait;
use cucumber::{given, then, when};
use domain::execution::{FailureReason, OperationStatus};
use domain::fleet::{HubAddress, HubPin, PublicKey, Signature, TokenTls};
use domain::maintenance::{Grant, LeaseEpoch, RepoGrants};
use domain::planning::{
    AgentConfig, AgentSettings, GroupBy, Hooks, Maintenance, RepositoryConfig, RepositorySettings,
};
use domain::secrets::{SecretRef, SecretValue};
use domain::{
    AgentId, OperationId, RepositoryAlias, RepositoryId, SignedDuration, Timestamp, Uuid,
};
use futures::StreamExt;

use super::Agent;
use app::execution::Finished;
use app::fakes::{
    FakeClock, FakeRetention, FakeSecrets, FakeSources, InMemoryConfigStore, InMemoryIdentity,
    InMemoryLeaseCache, InMemoryOperationStore,
};
use app::leases::MaintenanceLeases;
use app::maintenance::{MaintenanceKind, RunMaintenance};
use app::ports::{
    ConfigStore, Deletion, DeletionAsk, Engine, EngineError, EngineEvent, EngineResult, Identity,
    Job, JobControl, PruneSummary, RunningJob,
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
    deleted: Arc<AtomicBool>,
    finished: Option<Finished>,
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
    let leases = Arc::new(MaintenanceLeases::new(
        identity.clone(),
        Arc::new(InMemoryLeaseCache::default()),
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
        let grants = RepoGrants {
            generation: 1,
            signed_at: start(),
            grants: vec![Grant {
                repository: repository(),
                epoch: LeaseEpoch::new(1).expect("an epoch"),
                expires_at: start().checked_add(lasts).expect("a time"),
                signature: Signature::from_bytes([0; 64]).expect("a signature"),
            }],
        };
        leases
            .take(&grants, 0, start())
            .await
            .expect("the grants are kept");
    }
    let use_case = RunMaintenance::new(
        Arc::new(Pruning {
            clock: clock.clone(),
            planning_takes: world.planning_takes,
            repacking_takes: world.repacking_takes,
            deleted: Arc::clone(&world.deleted),
        }),
        clock,
        Arc::new(InMemoryOperationStore::new()),
        ResolveRepository::new(Arc::new(FakeSecrets::default())),
        Arc::new(FakeRetention),
        configs,
        Arc::default(),
        AgentId::from_uuid(Uuid::from_u128(1)),
        "demo-host".to_owned(),
        Arc::new(FakeSources),
    )
    .with_leases(leases);
    let operation = OperationId::from_uuid(Uuid::from_u128(0xcc));
    let assessed = use_case
        .assess(operation, &alias(), MaintenanceKind::Prune)
        .await
        .expect("the prune is assessed");
    let prepared = use_case
        .start(assessed)
        .await
        .expect("the prune starts while the lease is live");
    world.finished = Some(use_case.execute(prepared, futures::future::pending()).await);
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
                let (clock, planning_takes, repacking_takes, deleted) = (
                    Arc::clone(&self.clock),
                    self.planning_takes,
                    self.repacking_takes,
                    Arc::clone(&self.deleted),
                );
                futures::stream::once(async move {
                    // The plan and the repack take their time; then the
                    // prune asks.
                    clock.advance(planning_takes);
                    clock.advance(repacking_takes);
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
