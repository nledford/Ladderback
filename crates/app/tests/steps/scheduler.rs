//! Steps for `features/execution/scheduler.feature`: the scheduler's loop,
//! run as `serve` runs it, on a manual fake clock the steps move from one of
//! its sleeps to the next, with the in-memory configuration and schedule
//! stores, a scripted power supply and network, and a recording of the runs
//! it starts in place of the backups.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, PoisonError};

use app::fakes::{
    FakeClock, FakeNetwork, FakePower, InMemoryConfigStore, InMemoryScheduleStore, SeededRandom,
};
use app::ports::{Clock, ConfigStore, PlannedRun, ScheduleStore};
use app::schedule::{PlanRuns, ScheduleReport, Scheduler};
use async_trait::async_trait;
use cucumber::{given, then, when};
use domain::execution::conditions::{PowerSource, UnmetCondition};
use domain::execution::{Operation, Trigger};
use domain::planning::{
    AgentConfig, AgentSettings, CronExpression, Hooks, Plan, RepositoryConfig, RepositorySettings,
    Schedule, ScheduleZone, Source,
};
use domain::secrets::{FileRead, SecretRef};
use domain::{PlanId, RepositoryAlias, RepositoryId, SignedDuration, TimeZone, Timestamp};
use futures::FutureExt;
use futures::future::BoxFuture;

use super::Agent;

/// More turns of the loop than any scenario's stretch of time takes: a loop
/// that spins without sleeping fails instead of hanging.
const MOST_TURNS: u32 = 100_000;

/// A run the scheduler started.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Started {
    pub plan: PlanId,
    pub trigger: Trigger,
    pub at: Timestamp,
}

/// What the scheduler and the queue asked of the backups: the runs they
/// started, the queued operations they ran again, and what they said.
pub struct RecordingRuns {
    clock: Arc<dyn Clock>,
    started: Mutex<Vec<Started>>,
    resumed: Mutex<Vec<Operation>>,
    reports: Mutex<Vec<ScheduleReport>>,
}

impl RecordingRuns {
    pub fn new(clock: Arc<dyn Clock>) -> Self {
        Self {
            clock,
            started: Mutex::default(),
            resumed: Mutex::default(),
            reports: Mutex::default(),
        }
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
}

#[async_trait]
impl PlanRuns for RecordingRuns {
    async fn start(&self, plan: &PlanId, trigger: Trigger) {
        self.started
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(Started {
                plan: plan.clone(),
                trigger,
                at: self.clock.now(),
            });
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
            clock,
            zone: TimeZone::UTC,
            config: Arc::new(InMemoryConfigStore::new()),
            schedules: Arc::new(InMemoryScheduleStore::new()),
            power: Arc::new(FakePower::new()),
            network: Arc::new(FakeNetwork::new()),
            plans: Vec::new(),
            serving: None,
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
        let config = config_of(self.plans.clone());
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
                zone: self.zone.clone(),
            };
            async move { scheduler.run().await }.boxed()
        });
        let mut context = std::task::Context::from_waker(std::task::Waker::noop());
        for _ in 0..MOST_TURNS {
            let now = self.clock.now();
            if self.network_back_at.is_some_and(|back| back <= now) {
                self.network.set(Ok(true));
            }
            assert!(
                serving.poll_unpin(&mut context).is_pending(),
                "the scheduler's loop ended"
            );
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

/// A valid configuration holding `plans`, each on the demo repository.
fn config_of(plans: Vec<Plan>) -> AgentConfig {
    AgentConfig::new(
        AgentSettings::default(),
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

#[then(expr = "the next run of the plan {string} is planned for {string}")]
async fn planned_next(agent: &mut Agent, name: String, text: String) {
    assert_eq!(agent.scheduler.planned(&name).await, Some(time(&text)));
}

#[then(expr = "the plan {string} has no planned run")]
async fn no_planned_run(agent: &mut Agent, name: String) {
    assert_eq!(agent.scheduler.planned(&name).await, None);
}
