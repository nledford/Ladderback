//! Steps for `features/execution/full_disk_access.feature`: the agent's Full
//! Disk Access check over a fake probe, and backups whose engine is refused
//! the items a scenario names, run through `RunOperation` with that check.

use std::collections::BTreeMap;
use std::sync::Arc;

use app::execution::RunOperation;
use app::fakes::FakeFullDiskAccessProbe;
use app::full_disk_access::{FullDiskAccessCheck, StartNote};
use app::ports::{
    BackupJob, BackupSummary, Engine, EngineError, EngineEvent, EngineResult, Job, JobControl,
    RepositoryAccess, RunningJob,
};
use async_trait::async_trait;
use cucumber::{given, then, when};
use domain::execution::full_disk_access::FullDiskAccess;
use domain::execution::plan_tags::PlanTags;
use domain::execution::{Trigger, Warning, WarningKind};
use domain::planning::SourceFilter;
use domain::secrets::SecretValue;
use domain::{
    AgentId, OperationId, PlanId, RepositoryId, SignedDuration, SnapshotId, Timestamp, Uuid,
};
use futures::StreamExt;

use super::Agent;

/// The scenario's plan, the probe and the check over it, and the items the
/// engine is refused. The probe and the check are made by the steps that
/// say what the probe finds.
#[derive(Default)]
pub struct FullDiskAccessWorld {
    plan: Option<(String, String)>,
    probe: Option<Arc<FakeFullDiskAccessProbe>>,
    check: Option<Arc<FullDiskAccessCheck>>,
    /// What the log said when the agent started; `None` before it started.
    started: Option<Option<StartNote>>,
    /// Each item the engine is refused, with the engine's message for it.
    refused: Vec<(String, String)>,
    backups: u128,
}

impl FullDiskAccessWorld {
    fn check(&self) -> Arc<FullDiskAccessCheck> {
        Arc::clone(
            self.check
                .as_ref()
                .expect("the scenario says what the Full Disk Access probe finds"),
        )
    }

    /// The probe finds `found` from now on: the first time, the agent's
    /// check is made over it.
    fn finds(&mut self, found: FullDiskAccess) {
        match &self.probe {
            Some(probe) => probe.set(found),
            None => {
                let probe = Arc::new(FakeFullDiskAccessProbe::new(found));
                self.check = Some(Arc::new(FullDiskAccessCheck::new(probe.clone())));
                self.probe = Some(probe);
            }
        }
    }
}

fn found_named(name: &str) -> FullDiskAccess {
    match name {
        "granted" => FullDiskAccess::Granted,
        "missing" => FullDiskAccess::Missing,
        "unknown" => FullDiskAccess::Unknown,
        other => panic!("no Full Disk Access state is named {other}"),
    }
}

fn at() -> Timestamp {
    Timestamp::from_second(1_800_000_000).expect("a time")
}

fn agent_id() -> AgentId {
    AgentId::from_uuid(Uuid::from_u128(0x018f_0000_0000_7000_8000_0000_0000_0042))
}

#[given(expr = "a plan {string} backs up {string}")]
fn a_plan(agent: &mut Agent, plan: String, source: String) {
    agent.full_disk_access.plan = Some((plan, source));
}

#[given(
    regex = r"^the Full Disk Access probe (?:now )?(finds the grant|finds the grant missing|can't tell)$"
)]
fn the_probe_finds(agent: &mut Agent, finds: String) {
    let found = match finds.as_str() {
        "finds the grant" => FullDiskAccess::Granted,
        "finds the grant missing" => FullDiskAccess::Missing,
        _ => FullDiskAccess::Unknown,
    };
    agent.full_disk_access.finds(found);
}

#[given("the agent has started")]
async fn the_agent_has_started(agent: &mut Agent) {
    the_agent_starts(agent).await;
}

#[when("the agent starts")]
async fn the_agent_starts(agent: &mut Agent) {
    let note = agent.full_disk_access.check().at_start().await;
    agent.full_disk_access.started = Some(note);
}

#[given(expr = "the engine is refused {string} with {string}")]
fn the_engine_is_refused(agent: &mut Agent, item: String, detail: String) {
    agent.full_disk_access.refused.push((item, detail));
}

#[when(expr = "the plan {string} backs up")]
async fn the_plan_backs_up(agent: &mut Agent, plan: String) {
    let world = &mut agent.full_disk_access;
    let (named, source) = world.plan.clone().expect("the scenario has a plan");
    assert_eq!(named, plan, "the scenario's plan");
    world.backups += 1;
    let id = OperationId::from_uuid(Uuid::from_u128(0x0500 + world.backups));
    let job = Job::Backup(BackupJob {
        repository: RepositoryAccess {
            repository: "/srv/demo".to_owned(),
            repo_hot: None,
            password: SecretValue::new("demo-password"),
            options: BTreeMap::new(),
            options_hot: BTreeMap::new(),
            options_cold: BTreeMap::new(),
            secrets: Vec::new(),
            id: None,
            entry: None,
        },
        sources: vec![source],
        tags: PlanTags::new(PlanId::parse(&plan).expect("a plan ID"), agent_id()),
        extra_tags: Vec::new(),
        label: None,
        host: None,
        filter: SourceFilter::default(),
        deadline_budget: SignedDuration::from_hours(1),
    });
    let engine = Arc::new(Refusing {
        refused: world.refused.clone(),
    });
    let check = world.check();
    let finished = RunOperation::new(engine, agent.clock.clone(), agent.store.clone())
        .with_full_disk_access(check)
        .run(id, Trigger::Manual, job, futures::future::pending())
        .await;
    agent.notifications.tell_end(&finished.operation).await;
    agent.finished = Some(finished);
}

#[when("a person asks the agent to check Full Disk Access again")]
async fn check_again(agent: &mut Agent) {
    agent.full_disk_access.check().check().await;
}

#[then("the log says nothing of Full Disk Access")]
fn the_log_says_nothing(agent: &mut Agent) {
    assert_eq!(
        agent.full_disk_access.started,
        Some(None),
        "the agent started and its log says nothing of Full Disk Access"
    );
}

#[then("the log says once that Full Disk Access is missing")]
fn the_log_says_missing(agent: &mut Agent) {
    assert_eq!(
        agent.full_disk_access.started,
        Some(Some(StartNote::FullDiskAccessMissing))
    );
}

#[then(expr = "the agent reports Full Disk Access as {word}")]
fn the_agent_reports(agent: &mut Agent, state: String) {
    assert_eq!(
        agent.full_disk_access.check().found(),
        Some(found_named(&state))
    );
}

#[then(expr = "the operation has a {string} warning for {string}")]
fn has_a_warning_for(agent: &mut Agent, code: String, item: String) {
    let kind = super::operation_steps::kind_named(&code);
    let warnings: Vec<_> = agent
        .operation()
        .warnings()
        .iter()
        .map(|warning| (warning.kind, warning.item.as_deref()))
        .collect();
    assert_eq!(warnings, [(kind, Some(item.as_str()))]);
}

#[then(expr = "the operation has no {string} warning")]
fn has_no_warning(agent: &mut Agent, code: String) {
    let kind = super::operation_steps::kind_named(&code);
    assert!(
        agent
            .operation()
            .warnings()
            .iter()
            .all(|warning| warning.kind != kind),
        "{:?}",
        agent.operation().warnings()
    );
}

/// An engine whose backup starts, is refused each item the scenario names,
/// as rustic is while it reads the sources, and succeeds.
struct Refusing {
    refused: Vec<(String, String)>,
}

#[async_trait]
impl Engine for Refusing {
    async fn run(&self, _op: OperationId, job: Job) -> Result<RunningJob, EngineError> {
        assert!(matches!(job, Job::Backup(_)), "a backup");
        let mut events = vec![EngineEvent::Started {
            index_read_at: at(),
            repository: None,
        }];
        events.extend(self.refused.iter().map(|(item, detail)| {
            EngineEvent::Warning(Warning {
                kind: WarningKind::SourceUnreadable,
                item: Some(item.clone()),
                during: "scan".to_owned(),
                detail: detail.clone(),
            })
        }));
        events.push(EngineEvent::Succeeded(EngineResult::Backup(
            BackupSummary {
                repository: RepositoryId::parse(&"1".repeat(64)).expect("a repository ID"),
                snapshot: SnapshotId::parse(&"2".repeat(64)).expect("a snapshot ID"),
                parent: None,
                files_new: 1,
                files_changed: 0,
                files_unmodified: 0,
                bytes_processed: Some(100),
                data_added: 10,
                data_added_packed: 5,
                error_count: u64::try_from(self.refused.len()).expect("a few items"),
            },
        )));
        Ok(RunningJob {
            events: futures::stream::iter(events).boxed(),
            control: Box::new(Stop),
        })
    }
}

struct Stop;

#[async_trait]
impl JobControl for Stop {
    async fn stop(&mut self) {}
}
