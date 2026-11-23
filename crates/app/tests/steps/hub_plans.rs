//! Steps for `features/fleet/plans_and_progress.feature`: an approved
//! agent tells its hub its plans when they change and the progress of the
//! operation it runs, at most every 5 seconds, and the hub's dashboard
//! reads them (technical design §7.2). One sync, kept for the scenario,
//! remembers what it told the hub, as the daemon's does; the agent's clock
//! is a fake the steps move.

use std::sync::Arc;

use api::sync::AgentFrame;
use api::{OperationStep, ProgressUnit, UiEvent};
use app::fakes::{
    FakeClock, InMemoryConfigStore, InMemoryOperationStore, InMemoryOutbox, InMemoryScheduleStore,
    LoopbackHubConnector,
};
use app::ports::{Clock, ConfigStore, OperationStore, PlannedRun, ScheduleStore};
use app::status::ReadStatus;
use app::sync::{LiveProgress, ReadPlans, RecordReports, SyncWithHub, Synced};
use cucumber::{given, then, when};
use domain::execution::{Operation, OperationKind, Target, Trigger};
use domain::fleet::{
    EpochSeconds, LastRunOutcome, OutboxEpoch, PlansReport, ReportedPlan, ReportedProgress,
};
use domain::{OperationId, PlanId, RepositoryId, SignedDuration, TimeZone, Timestamp, Uuid};

use super::Agent;
use crate::scheduler_steps::config_with_plans;

/// The agent's side: its configuration, log, planned runs, progress and
/// outbox, the one sync that tells the hub, and its backup.
#[derive(Default)]
pub(super) struct LiveWorld {
    side: Option<Side>,
}

struct Side {
    config: Arc<InMemoryConfigStore>,
    operations: Arc<InMemoryOperationStore>,
    schedules: Arc<InMemoryScheduleStore>,
    progress: Arc<LiveProgress>,
    outbox: Arc<InMemoryOutbox>,
    record: RecordReports,
    clock: Arc<FakeClock>,
    connector: Arc<LoopbackHubConnector>,
    sync: Option<SyncWithHub>,
    backup: Option<OperationId>,
    made: u128,
}

fn side(agent: &mut Agent) -> &mut Side {
    let world = &mut agent.pairing;
    if world.live.side.is_none() {
        let connector = world.connector_to(world.hub());
        let outbox = Arc::new(InMemoryOutbox::new(OutboxEpoch::from_uuid(
            Uuid::from_u128(0xe1),
        )));
        world.live.side = Some(Side {
            config: Arc::new(InMemoryConfigStore::new()),
            operations: Arc::new(InMemoryOperationStore::new()),
            schedules: Arc::new(InMemoryScheduleStore::new()),
            progress: Arc::new(LiveProgress::new()),
            record: RecordReports::new(outbox.clone()),
            outbox,
            clock: Arc::new(FakeClock::new(
                Timestamp::from_second(1_800_000_000).unwrap(),
            )),
            connector,
            sync: None,
            backup: None,
            made: 0,
        });
    }
    world.live.side.as_mut().expect("the agent's side")
}

fn plan_id(name: &str) -> PlanId {
    PlanId::parse(name).expect("the scenario's plan ID is valid")
}

fn repository_id(alias: &str) -> RepositoryId {
    let folded = alias.bytes().fold(0x5748_u128, |acc, byte| {
        acc.wrapping_mul(257).wrapping_add(u128::from(byte))
    });
    RepositoryId::parse(&format!("{folded:064x}")).expect("64 hex digits are a repository ID")
}

fn hour() -> SignedDuration {
    SignedDuration::from_hours(1)
}

impl Side {
    /// One sync session, by the sync the scenario keeps.
    async fn sync(&mut self, agent: &super::Machine) {
        if self.sync.is_none() {
            let status = ReadStatus::new(
                self.config.clone(),
                self.operations.clone(),
                self.clock.clone(),
                TimeZone::UTC,
            );
            self.sync = Some(
                SyncWithHub::new(
                    agent.connecting(self.connector.clone()),
                    self.outbox.clone(),
                    self.clock.clone(),
                    agent.agent_id,
                    agent.hostname.clone(),
                    agent.zone.clone(),
                )
                .with_plans(ReadPlans::new(status, self.schedules.clone()))
                .with_progress(self.progress.clone()),
            );
        }
        let outcome = self.sync.as_ref().expect("the sync").sync_once().await;
        assert!(
            matches!(outcome, Ok(Synced::Replayed { .. })),
            "{outcome:?}"
        );
    }

    /// The plans snapshots the hub was sent.
    fn plans_sent(&self) -> usize {
        self.connector
            .sent()
            .iter()
            .filter(|text| {
                matches!(
                    AgentFrame::from_text(text),
                    Ok(AgentFrame::Signed { kind, .. }) if kind == "PlansSnapshot"
                )
            })
            .count()
    }

    /// The agent's event `event`, heard as the daemon hears it: kept in its
    /// outbox when it is a report, and as the progress when it is one.
    async fn hear(&self, event: &UiEvent) {
        self.record.record(event).await.expect("recorded");
        self.progress.hear(event);
    }

    fn next_id(&mut self) -> OperationId {
        self.made += 1;
        OperationId::from_uuid(Uuid::from_u128((0x0190_u128 << 112) | self.made))
    }
}

/// The machine the hub shows of the first agent.
async fn shown(agent: &Agent) -> app::pairing::Machine {
    let first = agent.pairing.first.as_ref().expect("the first agent");
    agent
        .pairing
        .hub()
        .pairing
        .machines()
        .await
        .expect("the hub's machines")
        .into_iter()
        .find(|machine| machine.record.agent_id == first.agent_id)
        .expect("the hub shows the agent")
}

async fn shown_plan(agent: &Agent, name: &str) -> ReportedPlan {
    let plans: PlansReport = shown(agent).await.plans.expect("the hub shows plans");
    plans
        .plans()
        .iter()
        .find(|plan| plan.plan == plan_id(name))
        .cloned()
        .expect("the hub shows the plan")
}

#[given(expr = "the agent has the plans {string} and {string}")]
async fn has_plans(agent: &mut Agent, first: String, second: String) {
    let config = config_with_plans(&[(&first, "nas"), (&second, "nas")], repository_id);
    side(agent)
        .config
        .save(&config)
        .await
        .expect("the configuration is kept");
}

#[given(expr = "the plan {string} last backed up successfully and runs next in an hour")]
#[when(expr = "the plan {string} last backed up successfully and runs next in an hour")]
async fn backed_up(agent: &mut Agent, plan: String) {
    let side = side(agent);
    let now = side.clock.now();
    let id = side.next_id();
    let mut backup = Operation::queue(
        id,
        OperationKind::Backup,
        Trigger::Schedule,
        Target {
            plan: Some(plan_id(&plan)),
            repository: None,
        },
        now - SignedDuration::from_mins(2),
    );
    backup
        .start(now - SignedDuration::from_mins(2))
        .expect("a queued backup starts");
    backup
        .succeed(now - SignedDuration::from_mins(1))
        .expect("a running backup succeeds");
    side.operations
        .save(&backup, None)
        .await
        .expect("the log keeps the backup");
    side.schedules
        .plan_run(&PlannedRun {
            plan: plan_id(&plan),
            at: now + hour(),
        })
        .await
        .expect("the run is planned");
}

#[given("the agent's sync with its hub ran")]
#[when("the agent's sync with its hub runs")]
async fn sync_runs(agent: &mut Agent) {
    let machine = agent.pairing.first.take().expect("the first agent");
    side(agent).sync(&machine).await;
    agent.pairing.first = Some(machine);
}

#[given("the agent started a backup")]
async fn started(agent: &mut Agent) {
    let side = side(agent);
    let id = side.next_id();
    side.backup = Some(id);
    let event = UiEvent::OperationStarted {
        operation: id.as_uuid().to_string(),
        repository: Some("nas".to_owned()),
        started_at: side.clock.now().to_string(),
    };
    side.hear(&event).await;
}

#[given(regex = r"^the backup has backed up (\d+) of (\d+) bytes$")]
async fn progressed(agent: &mut Agent, current: u64, total: u64) {
    let side = side(agent);
    let operation = side.backup.expect("a backup").as_uuid().to_string();
    let event = UiEvent::OperationProgress {
        operation,
        step: Some(OperationStep::BackingUp),
        unit: ProgressUnit::Bytes,
        current,
        total: Some(total),
        at: Some(side.clock.now().to_string()),
    };
    side.hear(&event).await;
}

#[when(
    regex = r"^the agent's clock moves on (\d+) seconds and the backup has backed up (\d+) of (\d+) bytes$"
)]
async fn later_progressed(agent: &mut Agent, seconds: i64, current: u64, total: u64) {
    seconds_pass(agent, seconds).await;
    progressed(agent, current, total).await;
}

#[when(regex = r"^the agent's clock moves on (\d+) seconds$")]
async fn seconds_pass(agent: &mut Agent, seconds: i64) {
    side(agent)
        .clock
        .advance(SignedDuration::from_secs(seconds));
}

#[when("the backup ends")]
async fn ends(agent: &mut Agent) {
    let side = side(agent);
    let event = UiEvent::OperationSucceeded {
        operation: side.backup.expect("a backup").as_uuid().to_string(),
        finished_at: side.clock.now().to_string(),
    };
    side.hear(&event).await;
}

#[then(expr = "the hub shows the agent's plans {string} and {string}, in that order")]
async fn shows_plans(agent: &mut Agent, first: String, second: String) {
    let plans = shown(agent).await.plans.expect("the hub shows plans");
    let names: Vec<_> = plans.plans().iter().map(|plan| plan.plan.clone()).collect();
    assert_eq!(names, [plan_id(&first), plan_id(&second)]);
    assert_eq!(plans.left_out(), 0);
}

#[then(expr = "the hub shows the plan {string} last succeeded and runs next in an hour")]
async fn shows_succeeded(agent: &mut Agent, plan: String) {
    let now = side(agent).clock.now();
    let shown = shown_plan(agent, &plan).await;
    let last = shown.last.expect("a last run");
    assert_eq!(last.outcome, LastRunOutcome::Succeeded);
    assert_eq!(
        last.at,
        EpochSeconds::from_timestamp(now - SignedDuration::from_mins(1)).unwrap()
    );
    assert_eq!(shown.next_run, EpochSeconds::from_timestamp(now + hour()));
}

#[then(expr = "the hub shows the plan {string} never ran and has no next run")]
async fn shows_never_ran(agent: &mut Agent, plan: String) {
    let shown = shown_plan(agent, &plan).await;
    assert_eq!((shown.last, shown.next_run), (None, None));
}

#[then(regex = r"^the hub was sent the agent's plans (once|twice)$")]
async fn plans_sent(agent: &mut Agent, times: String) {
    let expected = if times == "once" { 1 } else { 2 };
    assert_eq!(side(agent).plans_sent(), expected);
}

#[then(regex = r"^the hub shows the agent's backup at (\d+) of (\d+) bytes$")]
async fn shows_progress(agent: &mut Agent, current: u64, total: u64) {
    let backup = side(agent).backup.expect("a backup");
    let progress: Option<ReportedProgress> = shown(agent).await.progress;
    let progress = progress.expect("the hub shows a progress");
    assert_eq!(progress.op_id, backup);
    assert_eq!((progress.current, progress.total), (current, Some(total)));
    assert_eq!(progress.unit.as_str(), "bytes");
    assert_eq!(
        progress
            .step
            .map(|step| step.as_str().to_owned())
            .as_deref(),
        Some("backing-up")
    );
}

#[then("the hub shows no progress of the agent")]
async fn shows_no_progress(agent: &mut Agent) {
    assert_eq!(shown(agent).await.progress, None);
}
