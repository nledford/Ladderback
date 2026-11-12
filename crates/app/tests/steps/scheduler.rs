//! Steps for `features/execution/scheduler.feature`, `overlap.feature` and
//! `retry.feature`: the scheduler's loop, run as `serve` runs it, on a manual
//! fake clock the steps move from one of its sleeps to the next, with the
//! in-memory configuration and schedule stores, a scripted power supply and
//! network, the agent's one queue of backups, and in place of the backups a
//! recording of the runs it starts, each of which takes its turn in the
//! queue, then as long as the scenario says, and ends as it says.

use std::collections::{BTreeMap, VecDeque};
use std::sync::{Arc, Mutex, PoisonError};

use api::UiEvent;
use app::fakes::{
    FakeClock, FakeNetwork, FakePower, InMemoryConfigStore, InMemoryScheduleStore, SeededRandom,
};
use app::ports::{Clock, ConfigStore, EventBus, PendingRetry, PlannedRun, ScheduleStore};
use app::queue::{Place, RunQueue};
use app::retry::{RunEnd, RunEndSender, RunEnds, RunOutcome, run_ends};
use app::schedule::{PlanRuns, ScheduleReport, Scheduler};
use async_trait::async_trait;
use cucumber::{given, then, when};
use domain::execution::conditions::{PowerSource, UnmetCondition};
use domain::execution::queue::Overlapping;
use domain::execution::retry::{NoRetry, RetryScheduled};
use domain::execution::{Operation, Trigger};
use domain::planning::{
    AgentConfig, AgentSettings, Concurrency, CronExpression, Hooks, Overlap, Plan,
    RepositoryConfig, RepositorySettings, RetryPolicy, Schedule, ScheduleZone, Source,
};
use domain::secrets::{FileRead, SecretRef};
use domain::{
    OperationId, PlanId, RepositoryAlias, RepositoryId, SignedDuration, TimeZone, Timestamp,
};
use futures::FutureExt;
use futures::future::BoxFuture;
use futures::stream::BoxStream;

use super::Agent;

/// More turns of the loop than any scenario's stretch of time takes: a loop
/// that spins without sleeping fails instead of hanging.
const MOST_TURNS: u32 = 100_000;

/// What the scheduler announced, in order.
#[derive(Default)]
pub struct RecordingEvents(Mutex<Vec<UiEvent>>);

impl RecordingEvents {
    fn published(&self) -> Vec<UiEvent> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

#[async_trait]
impl EventBus for RecordingEvents {
    async fn publish(&self, event: UiEvent) {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(event);
    }

    async fn subscribe(&self) -> BoxStream<'static, UiEvent> {
        Box::pin(futures::stream::empty())
    }
}

/// A run the scheduler started.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Started {
    pub plan: PlanId,
    pub trigger: Trigger,
    pub at: Timestamp,
}

/// What the scheduler and the queue asked of the backups: the runs they
/// started, the queued operations they ran again, and what they said. A run
/// started is recorded when its turn comes, and holds its place for as long
/// as the scenario says a run of its plan takes.
pub struct RecordingRuns {
    clock: Arc<dyn Clock>,
    started: Arc<Mutex<Vec<Started>>>,
    resumed: Mutex<Vec<Operation>>,
    reports: Mutex<Vec<ScheduleReport>>,
    /// How long a run of each plan takes; one not listed ends at once.
    takes: Mutex<BTreeMap<PlanId, SignedDuration>>,
    /// How the runs of each plan end, in order, and how every run after
    /// them ends; a plan not listed succeeds.
    outcomes: Arc<Mutex<BTreeMap<PlanId, Outcomes>>>,
    /// Where each run says how it ended, as the daemon's runs do.
    ends: RunEndSender,
    /// What the scheduler reads the ends from, until it serves.
    to_read: Mutex<Option<RunEnds>>,
    /// The runs not ended yet, which the steps poll beside the loop.
    going: Mutex<Vec<BoxFuture<'static, ()>>>,
}

/// How a plan's runs end: the listed ones in order, then every other one
/// as `rest`.
struct Outcomes {
    listed: VecDeque<RunOutcome>,
    rest: RunOutcome,
}

impl RecordingRuns {
    pub fn new(clock: Arc<dyn Clock>) -> Self {
        let (ends, to_read) = run_ends();
        Self {
            clock,
            started: Arc::default(),
            resumed: Mutex::default(),
            reports: Mutex::default(),
            takes: Mutex::default(),
            outcomes: Arc::default(),
            ends,
            to_read: Mutex::new(Some(to_read)),
            going: Mutex::default(),
        }
    }

    /// The ends the scheduler reads; there is one reader.
    fn ends(&self) -> RunEnds {
        self.to_read
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
            .expect("one scheduler reads the ends")
    }

    fn end_as(&self, plan: PlanId, listed: Vec<RunOutcome>, rest: RunOutcome) {
        self.outcomes
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(
                plan,
                Outcomes {
                    listed: listed.into(),
                    rest,
                },
            );
    }

    pub fn started(&self) -> Vec<Started> {
        self.started
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    pub fn resumed(&self) -> Vec<Operation> {
        self.resumed
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    pub fn reports(&self) -> Vec<ScheduleReport> {
        self.reports
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    fn takes(&self, plan: PlanId, how_long: SignedDuration) {
        self.takes
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(plan, how_long);
    }

    /// Polls every run not ended yet, again while one ends: a run that ends
    /// gives the next its turn. Says whether a run ended.
    fn poll_going(&self, context: &mut std::task::Context<'_>) -> bool {
        let mut any = false;
        loop {
            let mut going =
                std::mem::take(&mut *self.going.lock().unwrap_or_else(PoisonError::into_inner));
            let before = going.len();
            going.retain_mut(|run| run.poll_unpin(context).is_pending());
            let ended = going.len() < before;
            self.going
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .append(&mut going);
            if !ended {
                return any;
            }
            any = true;
        }
    }
}

#[async_trait]
impl PlanRuns for RecordingRuns {
    async fn start(&self, plan: &PlanId, trigger: Trigger, mut place: Place) {
        let clock = Arc::clone(&self.clock);
        let started = Arc::clone(&self.started);
        let outcomes = Arc::clone(&self.outcomes);
        let ends = self.ends.clone();
        let plan = plan.clone();
        let how_long = self
            .takes
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&plan)
            .copied()
            .unwrap_or(SignedDuration::ZERO);
        let run = async move {
            place.turn().await;
            let number = {
                let mut started = started.lock().unwrap_or_else(PoisonError::into_inner);
                started.push(Started {
                    plan: plan.clone(),
                    trigger,
                    at: clock.now(),
                });
                started.len()
            };
            if how_long > SignedDuration::ZERO {
                clock.sleep(how_long).await;
            }
            let outcome = outcomes
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .get_mut(&plan)
                .map_or(RunOutcome::Succeeded, |outcomes| {
                    outcomes.listed.pop_front().unwrap_or(outcomes.rest)
                });
            // The place goes before the run says how it ended, as a run of
            // the daemon's gives it up before it returns.
            drop(place);
            ends.send(RunEnd {
                operation: OperationId::from_uuid(uuid::Uuid::from_u128(number as u128)),
                plan,
                trigger,
                outcome,
            });
        };
        self.going
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(run.boxed());
    }

    async fn resume(&self, operation: Operation) {
        self.resumed
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(operation);
    }

    fn report(&self, report: ScheduleReport) {
        self.reports
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(report);
    }
}

pub struct SchedulerWorld {
    clock: Arc<FakeClock>,
    zone: TimeZone,
    config: Arc<InMemoryConfigStore>,
    schedules: Arc<InMemoryScheduleStore>,
    power: Arc<FakePower>,
    network: Arc<FakeNetwork>,
    runs: Arc<RecordingRuns>,
    /// The queue the scheduler's runs and a person's take their turns in.
    queue: Arc<RunQueue>,
    /// What the scheduler announced.
    events: Arc<RecordingEvents>,
    /// The agent's concurrency limit.
    concurrency: Concurrency,
    plans: Vec<Plan>,
    /// The scheduler's loop, once the agent serves.
    serving: Option<BoxFuture<'static, ()>>,
    /// When the machine last woke.
    woke_at: Option<Timestamp>,
    /// When the network comes back; `None` when nothing brings it back.
    network_back_at: Option<Timestamp>,
}

impl Default for SchedulerWorld {
    fn default() -> Self {
        let clock = Arc::new(FakeClock::new(time("2026-06-10 00:00+00:00")));
        Self {
            runs: Arc::new(RecordingRuns::new(clock.clone())),
            queue: Arc::new(RunQueue::new()),
            concurrency: Concurrency::default(),
            clock,
            zone: TimeZone::UTC,
            config: Arc::new(InMemoryConfigStore::new()),
            schedules: Arc::new(InMemoryScheduleStore::new()),
            power: Arc::new(FakePower::new()),
            network: Arc::new(FakeNetwork::new()),
            plans: Vec::new(),
            serving: None,
            events: Arc::default(),
            woke_at: None,
            network_back_at: None,
        }
    }
}

impl SchedulerWorld {
    fn plan(&mut self, name: &str) -> &mut Plan {
        let id = plan_id(name);
        self.plans
            .iter_mut()
            .find(|plan| plan.id == id)
            .unwrap_or_else(|| panic!("the scenario has no plan {name}"))
    }

    fn schedule(&mut self, name: &str) -> &mut Schedule {
        self.plan(name)
            .schedule
            .as_mut()
            .unwrap_or_else(|| panic!("the plan {name} has a schedule"))
    }

    /// Saves the plans as the configuration the scheduler reads.
    async fn save(&self) {
        let config = config_of(self.concurrency, self.plans.clone());
        self.config.save(&config).await.expect("the config saves");
    }

    /// Runs the scheduler, moving the clock from one of its sleeps to the
    /// next, until the clock reads `until`.
    fn serve_until(&mut self, until: Timestamp) {
        let serving = self.serving.get_or_insert_with(|| {
            let scheduler = Scheduler {
                clock: self.clock.clone(),
                random: Arc::new(SeededRandom::new(7)),
                schedules: self.schedules.clone(),
                config: self.config.clone(),
                power: self.power.clone(),
                network: self.network.clone(),
                runs: self.runs.clone(),
                queue: self.queue.clone(),
                zone: self.zone.clone(),
                ends: self.runs.ends(),
                events: self.events.clone(),
            };
            async move { scheduler.run().await }.boxed()
        });
        let mut context = std::task::Context::from_waker(std::task::Waker::noop());
        for _ in 0..MOST_TURNS {
            let now = self.clock.now();
            if self.network_back_at.is_some_and(|back| back <= now) {
                self.network.set(Ok(true));
            }
            // Until nothing more happens at this moment: a run that ends
            // says so, and the loop hears it before time moves on.
            let mut ended = true;
            while ended {
                self.runs.poll_going(&mut context);
                assert!(
                    serving.poll_unpin(&mut context).is_pending(),
                    "the scheduler's loop ended"
                );
                ended = self.runs.poll_going(&mut context);
            }
            if now >= until {
                return;
            }
            let mut step = now.duration_until(until);
            if let Some(next) = self.clock.until_next_sleeper() {
                step = step.min(next);
            }
            if let Some(back) = self.network_back_at.filter(|back| *back > now) {
                step = step.min(now.duration_until(back));
            }
            self.clock.advance(step.max(SignedDuration::ZERO));
        }
        panic!("the scheduler's loop turned {MOST_TURNS} times before {until}");
    }

    async fn planned(&self, name: &str) -> Option<Timestamp> {
        let plan = plan_id(name);
        self.schedules
            .planned_runs()
            .await
            .expect("the schedule store answers")
            .into_iter()
            .find(|run| run.plan == plan)
            .map(|run| run.at)
    }

    fn started(&self, name: &str) -> Vec<Started> {
        let plan = plan_id(name);
        self.runs
            .started()
            .into_iter()
            .filter(|run| run.plan == plan)
            .collect()
    }
}

/// A time as the scenario writes it: `2026-06-10 03:00+02:00`.
fn time(text: &str) -> Timestamp {
    text.parse()
        .unwrap_or_else(|error| panic!("{text:?} is a time with its offset: {error}"))
}

fn plan_id(name: &str) -> PlanId {
    PlanId::parse(name).expect("the scenario's plan ID is valid")
}

fn alias() -> RepositoryAlias {
    RepositoryAlias::parse("nas").expect("an alias")
}

fn demo_repository() -> RepositoryConfig {
    RepositoryConfig {
        id: None,
        settings: RepositorySettings {
            repository: "/srv/demo-repo".to_owned(),
            repo_hot: None,
            password: SecretRef::File {
                path: "/srv/demo-password".to_owned(),
                read: FileRead::FirstLine,
            },
            options: BTreeMap::new(),
            options_hot: BTreeMap::new(),
            options_cold: BTreeMap::new(),
        },
        maintenance: Default::default(),
        group_by: Default::default(),
        hooks: Hooks::default(),
    }
}

/// A valid configuration holding `plans`, each on the demo repository, for
/// an agent that runs at most `concurrency` backups at once.
fn config_of(concurrency: Concurrency, plans: Vec<Plan>) -> AgentConfig {
    AgentConfig::new(
        AgentSettings {
            concurrency,
            ..AgentSettings::default()
        },
        Hooks::default(),
        BTreeMap::from([(alias(), demo_repository())]),
        plans,
    )
    .expect("the scenario's configuration is valid")
}

/// A configuration with a plan, unscheduled, for each of `plans`: a plan's
/// name and the alias of the repository it backs up to. Each repository has
/// the ID `id` gives its alias.
pub fn config_with_plans(plans: &[(&str, &str)], id: fn(&str) -> RepositoryId) -> AgentConfig {
    let mut repositories = BTreeMap::new();
    let mut listed: Vec<Plan> = Vec::new();
    for (name, on) in plans {
        let repository = RepositoryAlias::parse(on).expect("the scenario's alias is valid");
        repositories.insert(
            repository.clone(),
            RepositoryConfig {
                id: Some(id(on)),
                ..demo_repository()
            },
        );
        if listed.iter().all(|plan| plan.id != plan_id(name)) {
            listed.push(plan_on(name, repository));
        }
    }
    AgentConfig::new(
        AgentSettings::default(),
        Hooks::default(),
        repositories,
        listed,
    )
    .expect("the scenario's configuration is valid")
}

fn plan_named(name: &str) -> Plan {
    plan_on(name, alias())
}

fn plan_on(name: &str, repository: RepositoryAlias) -> Plan {
    Plan::new(
        plan_id(name),
        repository,
        vec![Source::parse(&format!("/home/demo/{name}")).expect("a source")],
    )
}

fn schedule_zone(name: &str) -> ScheduleZone {
    let zone =
        TimeZone::get(name).unwrap_or_else(|error| panic!("{name:?} is a time zone: {error}"));
    ScheduleZone::new(name, zone).expect("the scenario's time zone has a name")
}

#[given(expr = "the machine's time zone is {string}")]
fn machine_zone(agent: &mut Agent, name: String) {
    agent.scheduler.zone =
        TimeZone::get(&name).unwrap_or_else(|error| panic!("{name:?} is a time zone: {error}"));
}

#[given(expr = "the agent starts at {string}")]
fn starts_at(agent: &mut Agent, text: String) {
    let world = &mut agent.scheduler;
    world.clock = Arc::new(FakeClock::new(time(&text)));
    world.runs = Arc::new(RecordingRuns::new(world.clock.clone()));
}

#[given(expr = "the plan {string} is scheduled {string}")]
async fn scheduled(agent: &mut Agent, name: String, cron: String) {
    let cron = CronExpression::parse(&cron).expect("a cron expression");
    let mut plan = plan_named(&name);
    plan.schedule = Some(Schedule::new(cron));
    agent.scheduler.plans.push(plan);
    agent.scheduler.save().await;
}

#[given(expr = "the plan {string} doesn't make up for runs it missed")]
async fn no_catch_up(agent: &mut Agent, name: String) {
    let schedule = agent.scheduler.schedule(&name);
    *schedule = schedule.clone().with_catch_up(false);
    agent.scheduler.save().await;
}

#[given(expr = "the plan {string} runs only on AC power")]
async fn on_ac_only(agent: &mut Agent, name: String) {
    let schedule = agent.scheduler.schedule(&name);
    *schedule = schedule.clone().with_on_ac_power(true);
    agent.scheduler.save().await;
}

#[given(expr = "the plan {string} reads its schedule in the time zone {string}")]
#[when(expr = "the plan {string} reads its schedule in the time zone {string}")]
async fn plan_zone(agent: &mut Agent, name: String, zone: String) {
    let schedule = agent.scheduler.schedule(&name);
    *schedule = schedule.clone().with_zone(schedule_zone(&zone));
    agent.scheduler.save().await;
}

#[given("the machine runs on battery")]
fn on_battery(agent: &mut Agent) {
    agent.scheduler.power.set(Ok(PowerSource::Battery));
}

#[given(expr = "the plan {string} queues a run that comes due while its last run is going")]
#[when(expr = "the plan {string} queues a run that comes due while its last run is going")]
async fn queues_overlapping(agent: &mut Agent, name: String) {
    agent.scheduler.plan(&name).overlap = Overlap::Queue;
    agent.scheduler.save().await;
}

#[given(expr = "a run of the plan {string} takes {int} minutes")]
fn run_takes(agent: &mut Agent, name: String, minutes: i64) {
    agent
        .scheduler
        .runs
        .takes(plan_id(&name), SignedDuration::from_mins(minutes));
}

#[given(expr = "the agent runs at most {int} backup(s) at once")]
#[when(expr = "the agent runs at most {int} backup(s) at once")]
async fn at_most(agent: &mut Agent, limit: i64) {
    agent.scheduler.concurrency = Concurrency::new(limit).expect("the scenario's limit is valid");
    agent.scheduler.save().await;
}

#[given(expr = "the next run of the plan {string} was planned for {string}")]
async fn planned_for(agent: &mut Agent, name: String, text: String) {
    agent
        .scheduler
        .schedules
        .plan_run(&PlannedRun {
            plan: plan_id(&name),
            at: time(&text),
        })
        .await
        .expect("the schedule store keeps the run");
}

#[when(expr = "the agent serves until {string}")]
fn serves_until(agent: &mut Agent, text: String) {
    agent.scheduler.serve_until(time(&text));
}

#[when("the network goes down")]
fn network_down(agent: &mut Agent) {
    agent.scheduler.network.set(Ok(false));
}

#[when(expr = "the machine sleeps until {string}")]
fn sleeps_until(agent: &mut Agent, text: String) {
    let world = &mut agent.scheduler;
    let woke = time(&text);
    world.clock.suspend(world.clock.now().duration_until(woke));
    world.woke_at = Some(woke);
}

#[when(expr = "the network comes back {int} seconds after the machine wakes")]
fn network_back(agent: &mut Agent, seconds: i64) {
    let world = &mut agent.scheduler;
    let woke = world.woke_at.expect("the machine slept");
    world.network_back_at = Some(
        woke.checked_add(SignedDuration::from_secs(seconds))
            .expect("a time in range"),
    );
}

#[when(expr = "the schedule of the plan {string} is removed")]
async fn schedule_removed(agent: &mut Agent, name: String) {
    agent.scheduler.plan(&name).schedule = None;
    agent.scheduler.save().await;
}

#[then(expr = "the plan {string} ran once, on schedule, at {string}")]
fn ran_on_schedule(agent: &mut Agent, name: String, text: String) {
    assert_eq!(
        agent.scheduler.started(&name),
        vec![Started {
            plan: plan_id(&name),
            trigger: Trigger::Schedule,
            at: time(&text),
        }]
    );
}

#[then(expr = "the plan {string} ran once, to catch up, at {string}")]
fn ran_to_catch_up_at(agent: &mut Agent, name: String, text: String) {
    assert_eq!(
        agent.scheduler.started(&name),
        vec![Started {
            plan: plan_id(&name),
            trigger: Trigger::CatchUp,
            at: time(&text),
        }]
    );
}

#[then(expr = "the plan {string} ran once, to catch up, once the network was up")]
fn ran_to_catch_up_online(agent: &mut Agent, name: String) {
    let started = agent.scheduler.started(&name);
    let back = agent
        .scheduler
        .network_back_at
        .expect("the network came back");
    match started.as_slice() {
        [run] => {
            assert_eq!(run.trigger, Trigger::CatchUp, "{run:?}");
            assert!(
                run.at >= back,
                "the run started at {}, before the network came back at {back}",
                run.at
            );
        }
        runs => panic!("the plan {name} ran {} times: {runs:?}", runs.len()),
    }
}

#[then(expr = "the plan {string} hasn't run")]
fn not_run(agent: &mut Agent, name: String) {
    assert_eq!(agent.scheduler.started(&name), Vec::new());
}

#[then(expr = "the runs of the plan {string} started at {string}")]
fn runs_started_at(agent: &mut Agent, name: String, times: String) {
    let at: Vec<Timestamp> = agent
        .scheduler
        .started(&name)
        .into_iter()
        .map(|run| run.at)
        .collect();
    let expected: Vec<Timestamp> = times.split(", ").map(time).collect();
    assert_eq!(at, expected, "the runs of the plan {name}");
}

#[then("the agent waited for the network once")]
fn waited_once(agent: &mut Agent) {
    let waits = agent
        .scheduler
        .runs
        .reports()
        .into_iter()
        .filter(|report| matches!(report, ScheduleReport::NetworkWaited(_)))
        .count();
    assert_eq!(waits, 1);
}

#[then(
    expr = "the agent said the run of the plan {string} was skipped because the machine is on battery"
)]
fn said_skipped(agent: &mut Agent, name: String) {
    let skipped = ScheduleReport::Skipped {
        plan: plan_id(&name),
        unmet: UnmetCondition::OnBattery,
    };
    assert!(
        agent.scheduler.runs.reports().contains(&skipped),
        "the agent said {:?}",
        agent.scheduler.runs.reports()
    );
}

#[then(
    expr = "the agent said the run of the plan {string} was skipped because its last run is still going"
)]
fn said_overlapped_running(agent: &mut Agent, name: String) {
    said_overlapped(agent, &name, Overlapping::Running);
}

#[then(
    expr = "the agent said the run of the plan {string} was skipped because a run of it is already waiting"
)]
fn said_overlapped_waiting(agent: &mut Agent, name: String) {
    said_overlapped(agent, &name, Overlapping::Waiting);
}

fn said_overlapped(agent: &Agent, name: &str, overlapping: Overlapping) {
    let skipped = ScheduleReport::Overlapped {
        plan: plan_id(name),
        overlapping,
    };
    assert!(
        agent.scheduler.runs.reports().contains(&skipped),
        "the agent said {:?}",
        agent.scheduler.runs.reports()
    );
}

#[then(expr = "the next run of the plan {string} is planned for {string}")]
async fn planned_next(agent: &mut Agent, name: String, text: String) {
    assert_eq!(agent.scheduler.planned(&name).await, Some(time(&text)));
}

#[then(expr = "the plan {string} has no planned run")]
async fn no_planned_run(agent: &mut Agent, name: String) {
    assert_eq!(agent.scheduler.planned(&name).await, None);
}

#[given(
    expr = "the plan {string} retries a failed backup {int} time(s), {int} minute(s) apart at first"
)]
async fn retries(agent: &mut Agent, name: String, attempts: i64, minutes: i64) {
    agent.scheduler.plan(&name).retry = Some(
        RetryPolicy::new(attempts, SignedDuration::from_mins(minutes))
            .expect("the scenario's retry policy is valid"),
    );
    agent.scheduler.save().await;
}

fn outcome(text: &str) -> RunOutcome {
    match text {
        "failed" => RunOutcome::Failed,
        "succeeded" => RunOutcome::Succeeded,
        "cancelled" => RunOutcome::Cancelled,
        other => panic!("{other:?} is no way for a run to end"),
    }
}

#[given(expr = "the runs of the plan {string} end {string}")]
fn runs_end(agent: &mut Agent, name: String, outcomes: String) {
    let listed = outcomes.split(", ").map(outcome).collect();
    agent
        .scheduler
        .runs
        .end_as(plan_id(&name), listed, RunOutcome::Succeeded);
}

#[given(expr = "every run of the plan {string} fails")]
fn every_run_fails(agent: &mut Agent, name: String) {
    agent
        .scheduler
        .runs
        .end_as(plan_id(&name), Vec::new(), RunOutcome::Failed);
}

#[given(
    expr = "retry {int} of the plan {string} was pending for {string}, before the run planned for {string}"
)]
async fn retry_pending(agent: &mut Agent, attempt: u32, name: String, at: String, until: String) {
    agent
        .scheduler
        .schedules
        .keep_retry(&PendingRetry {
            plan: plan_id(&name),
            retry: RetryScheduled {
                attempt,
                at: time(&at),
                until: Some(time(&until)),
            },
        })
        .await
        .expect("the schedule store keeps the retry");
}

#[when(expr = "a person starts a backup of the plan {string}")]
async fn person_starts(agent: &mut Agent, name: String) {
    let plan = plan_id(&name);
    let place = agent.scheduler.queue.push(&plan, agent.scheduler.concurrency);
    let runs = Arc::clone(&agent.scheduler.runs);
    runs.start(&plan, Trigger::Manual, place).await;
}

#[then(expr = "the retries of the plan {string} started at {string}")]
fn retries_started_at(agent: &mut Agent, name: String, times: String) {
    let at: Vec<Timestamp> = agent
        .scheduler
        .started(&name)
        .into_iter()
        .filter(|run| run.trigger == Trigger::Retry)
        .map(|run| run.at)
        .collect();
    let expected: Vec<Timestamp> = times.split(", ").map(time).collect();
    assert_eq!(at, expected, "the retries of the plan {name}");
}

#[then(expr = "the agent announced retry {int} of {int} of the plan {string} for {string}")]
fn announced_retry(agent: &mut Agent, attempt: u32, attempts: u32, name: String, at: String) {
    let published = agent.scheduler.events.published();
    let announced = published.iter().any(|event| {
        matches!(
            event,
            UiEvent::BackupRetrying {
                plan,
                attempt: said,
                attempts: of,
                at: when,
                ..
            } if *plan == name
                && *said == attempt
                && *of == attempts
                && when.parse::<Timestamp>().ok() == Some(time(&at))
        )
    });
    assert!(announced, "the agent announced {published:?}");
}

fn said_retries_ended(agent: &Agent, name: &str, why: NoRetry) {
    let ended = ScheduleReport::RetriesEnded {
        plan: plan_id(name),
        why,
    };
    assert!(
        agent.scheduler.runs.reports().contains(&ended),
        "the agent said {:?}",
        agent.scheduler.runs.reports()
    );
}

#[then(expr = "the agent said the retries of the plan {string} ran out")]
fn said_ran_out(agent: &mut Agent, name: String) {
    said_retries_ended(agent, &name, NoRetry::Exhausted);
}

#[then(expr = "the agent said the retries of the plan {string} stopped at its next scheduled run")]
fn said_next_run(agent: &mut Agent, name: String) {
    said_retries_ended(agent, &name, NoRetry::NextRun);
}

#[then(
    expr = "the agent said the retries of the plan {string} stopped because a run of it is going"
)]
fn said_run_going(agent: &mut Agent, name: String) {
    said_retries_ended(agent, &name, NoRetry::RunGoing);
}

#[then(expr = "the plan {string} has no pending retry")]
async fn no_pending_retry(agent: &mut Agent, name: String) {
    let plan = plan_id(&name);
    let pending = agent
        .scheduler
        .schedules
        .pending_retries()
        .await
        .expect("the schedule store answers");
    assert!(
        pending.iter().all(|retry| retry.plan != plan),
        "{pending:?}"
    );
}
