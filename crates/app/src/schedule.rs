//! The scheduler (technical design §5): the loop in `serve` that runs each
//! scheduled plan at its next run, made up for once when the agent comes
//! back, after the plan's conditions. It is driven by the `Clock` port, and
//! keeps each plan's planned run through the `ScheduleStore` port, so a run
//! that passed while the agent was off or asleep is found when it is back.
//!
//! When a plan runs is the domain's `NextRun` (decision 60); whether it may
//! run is `crate::conditions`. What this loop adds is the order (decision
//! 73): it samples the clocks for a wake before it starts any run that is
//! due, so a run missed while the machine slept is decided by `NextRun`'s
//! rules for a missed run whichever of its timers fires first.

use std::collections::BTreeMap;
use std::sync::Arc;

use async_trait::async_trait;
use domain::execution::OperationKind;
use domain::execution::Trigger;
use domain::execution::conditions::UnmetCondition;
use domain::execution::next_run::NextRun;
use domain::execution::queue::Overlapping;
use domain::execution::retry::{NoRetry, RetryScheduled};
use domain::execution::wake::WAKE_CHECK_EVERY;
use domain::planning::{AgentConfig, NoNextRun, Overlap, Schedule, ScheduleZone};
use domain::{OperationId, PlanId, SignedDuration, TimeZone, Timestamp};

use crate::conditions::{NetworkWait, PowerCheck, WakeWatch, check_power, wait_for_network};
use crate::ports::{
    Clock, ConfigError, ConfigStore, EventBus, Network, PlannedRun, PlatformError, PowerSupply,
    Random, ScheduleStore, StoreError,
};
use crate::queue::{Place, RunQueue};
use crate::recovery::QueuedEnd;
use crate::retry::RunEnds;

/// What starts the runs the scheduler and the queue decide on: the
/// composition root's backups of a plan, each on a task of its own. Not a
/// port: it is the use cases the root wires. Which runs may start when is
/// the [`RunQueue`]'s (the overlap policy and the concurrency limit, §5);
/// retries (#275) go here too.
#[async_trait]
pub trait PlanRuns: Send + Sync {
    /// Starts a backup of `plan`, started by `trigger`, as a new operation,
    /// and returns once the operation log has it queued. It runs once
    /// `place`'s turn comes, and holds the place until it ends. A run that
    /// can't start (a secret that can't be resolved) is the
    /// implementation's to say in the daemon's log, and drops its place.
    async fn start(&self, plan: &PlanId, trigger: Trigger, place: Place);

    /// Runs `operation`, a backup the log holds as queued from before the
    /// daemon started, under its own ID, and returns once it is on its way.
    async fn resume(&self, operation: domain::execution::Operation);

    /// Says what the scheduler did that no operation records.
    fn report(&self, report: ScheduleReport);
}

/// What the scheduler did that no operation records, for the daemon's log.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ScheduleReport {
    /// A plan's run didn't start: a condition it sets wasn't met.
    Skipped { plan: PlanId, unmet: UnmetCondition },
    /// A plan's run didn't start: its last run is still going, running or
    /// waiting for its turn, and its overlap policy skips it (§5).
    Overlapped {
        plan: PlanId,
        overlapping: Overlapping,
    },
    /// A plan's run started, though the platform couldn't say what powers
    /// the machine.
    PowerUnknown { plan: PlanId, error: PlatformError },
    /// The catch-up runs after a wake waited for the network, and how that
    /// ended.
    NetworkWaited(NetworkWait),
    /// A plan's schedule has no run after now: it isn't run again.
    NoNextRun { plan: PlanId },
    /// The planned runs couldn't be read or kept, or a queued operation
    /// couldn't be ended; the scheduler goes on with what it holds.
    Store(StoreError),
    /// The configuration couldn't be read; the scheduler goes on with the
    /// plans it last read.
    Config(ConfigError),
    /// An operation queued before the start can't run again, and ended as
    /// failed because the daemon stopped (decision 73).
    QueuedEnded {
        operation: OperationId,
        kind: OperationKind,
        why: QueuedEnd,
    },
    /// A plan's backup failed, and the plan retries it as `retry` says.
    Retrying { plan: PlanId, retry: RetryScheduled },
    /// A plan's retries ended without a success: the failed backup isn't
    /// retried, or a retry planned didn't run, and why.
    RetriesEnded { plan: PlanId, why: NoRetry },
}

/// The loop that runs scheduled plans (§5). It owns nothing but its ports;
/// the composition root spawns [`run`](Self::run) once the daemon serves and
/// drops it when the daemon stops.
pub struct Scheduler {
    pub clock: Arc<dyn Clock>,
    pub random: Arc<dyn Random>,
    pub schedules: Arc<dyn ScheduleStore>,
    pub config: Arc<dyn ConfigStore>,
    pub power: Arc<dyn PowerSupply>,
    pub network: Arc<dyn Network>,
    pub runs: Arc<dyn PlanRuns>,
    /// The agent's one queue of backups, which the routes share.
    pub queue: Arc<RunQueue>,
    /// The machine's time zone, which a cron schedule that names none is
    /// read in.
    pub zone: TimeZone,
    /// How each backup ended, as its run says it: the scheduler retries a
    /// failed one (§5).
    pub ends: RunEnds,
    /// Where the scheduler announces a retry it plans (`backup.retrying`).
    pub events: Arc<dyn EventBus>,
}

/// A scheduled plan, as the loop holds it.
#[derive(Clone, Debug)]
struct Planned {
    schedule: Schedule,
    /// What the plan does with a run that comes due while its last one is
    /// still going.
    overlap: Overlap,
    /// `None` once the schedule has no run after its last one.
    next: Option<NextRun>,
    /// A catch-up the agent decided on as it woke: it waits for the network.
    after_wake: bool,
}

/// What the loop holds between its turns.
#[derive(Default)]
struct Plans {
    planned: BTreeMap<PlanId, Planned>,
    /// The planned runs kept from before the start, until the configuration
    /// has been read: each is resumed for its plan, and dropped for a plan
    /// that is no longer scheduled.
    kept: Option<BTreeMap<PlanId, Timestamp>>,
}

impl Scheduler {
    /// Runs scheduled plans until the future is dropped. On start it asks
    /// `NextRun` what becomes of each planned run kept from before; then, at
    /// least every [`WAKE_CHECK_EVERY`] and whenever a run is due, it reads
    /// the configuration again, checks for a wake, and starts the runs that
    /// are due.
    pub async fn run(&self) {
        let mut watch = WakeWatch::new(Arc::clone(&self.clock));
        let mut plans = Plans {
            planned: BTreeMap::new(),
            kept: Some(self.kept_runs().await),
        };
        self.refresh(&mut plans).await;
        loop {
            self.start_due(&mut plans.planned).await;
            self.clock.sleep(self.until_next(&plans.planned)).await;
            self.refresh(&mut plans).await;
            // Before anything due starts: a run missed while the machine
            // slept is `NextRun::resume`'s to decide, not a run on time.
            if watch.check().is_some() {
                self.resume_all(&mut plans.planned).await;
            }
        }
    }

    async fn kept_runs(&self) -> BTreeMap<PlanId, Timestamp> {
        match self.schedules.planned_runs().await {
            Ok(runs) => runs.into_iter().map(|run| (run.plan, run.at)).collect(),
            Err(error) => {
                self.runs.report(ScheduleReport::Store(error));
                BTreeMap::new()
            }
        }
    }

    /// Reads the configuration and brings the plans in line with it: a plan
    /// first found is planned (from its kept run on start, else from now), a
    /// plan whose cadence, time zone or jitter changed is planned again from
    /// now, and a plan no longer scheduled is forgotten. The queue runs as
    /// many backups at once as it says from then on, the runs already
    /// waiting included. A configuration that can't be read changes nothing.
    async fn refresh(&self, plans: &mut Plans) {
        let config = match self.config.load().await {
            Ok(config) => config,
            Err(error) => {
                self.runs.report(ScheduleReport::Config(error));
                return;
            }
        };
        let now = self.clock.now();
        self.queue.set_limit(config.agent().concurrency);
        let scheduled = scheduled(&config);
        let mut kept = plans.kept.take().unwrap_or_default();
        for (plan, (schedule, overlap)) in &scheduled {
            let next = match plans.planned.get_mut(plan) {
                Some(held) if same_runs(&held.schedule, schedule) => {
                    // Its conditions and overlap policy may have changed;
                    // its runs haven't.
                    held.schedule = schedule.clone();
                    held.overlap = *overlap;
                    continue;
                }
                Some(_) => {
                    NextRun::after(schedule, self.zone_of(schedule), now, self.random.draw())
                }
                None => match kept.remove(plan) {
                    Some(at) => NextRun::resume(
                        schedule,
                        self.zone_of(schedule),
                        at,
                        now,
                        self.random.draw(),
                    ),
                    None => {
                        NextRun::after(schedule, self.zone_of(schedule), now, self.random.draw())
                    }
                },
            };
            let next = self.keep(plan, next).await;
            plans.planned.insert(
                plan.clone(),
                Planned {
                    schedule: schedule.clone(),
                    overlap: *overlap,
                    next,
                    after_wake: false,
                },
            );
        }
        let gone: Vec<PlanId> = plans
            .planned
            .keys()
            .filter(|plan| !scheduled.contains_key(*plan))
            .cloned()
            .collect();
        for plan in gone.iter().chain(kept.keys()) {
            plans.planned.remove(plan);
            if let Err(error) = self.schedules.unplan(plan).await {
                self.runs.report(ScheduleReport::Store(error));
            }
        }
    }

    /// After a wake: what becomes of each planned run is `NextRun::resume`'s
    /// answer, and a catch-up it decides on waits for the network.
    async fn resume_all(&self, planned: &mut BTreeMap<PlanId, Planned>) {
        let now = self.clock.now();
        for (plan, held) in planned.iter_mut() {
            let Some(next) = held.next else { continue };
            let resumed = NextRun::resume(
                &held.schedule,
                self.zone_of(&held.schedule),
                next.at,
                now,
                self.random.draw(),
            );
            held.after_wake = resumed.is_ok_and(|run| run.trigger == Trigger::CatchUp);
            held.next = self.keep(plan, resumed).await;
        }
    }

    /// Starts every run that is due, in the order of the plans' IDs, each
    /// after the plan's conditions and as its overlap policy says, in a place
    /// of the queue, and plans each plan's following run from the time its
    /// run started (or was skipped). The catch-up runs after a wake first
    /// wait, once for all of them, for the network.
    async fn start_due(&self, planned: &mut BTreeMap<PlanId, Planned>) {
        let now = self.clock.now();
        let due: Vec<PlanId> = planned
            .iter()
            .filter(|(_, held)| held.next.is_some_and(|next| next.at <= now))
            .map(|(plan, _)| plan.clone())
            .collect();
        if due
            .iter()
            .any(|plan| planned.get(plan).is_some_and(|held| held.after_wake))
        {
            let waited = wait_for_network(self.clock.as_ref(), self.network.as_ref()).await;
            self.runs.report(ScheduleReport::NetworkWaited(waited));
        }
        for plan in due {
            let Some(held) = planned.get_mut(&plan) else {
                continue;
            };
            let Some(next) = held.next else { continue };
            held.after_wake = false;
            match check_power(&held.schedule, self.power.as_ref()).await {
                PowerCheck::Met => self.start(&plan, held, next.trigger, None).await,
                PowerCheck::Unknown(error) => {
                    self.start(&plan, held, next.trigger, Some(error)).await;
                }
                PowerCheck::Unmet(unmet) => self.runs.report(ScheduleReport::Skipped {
                    plan: plan.clone(),
                    unmet,
                }),
            }
            let started = self.clock.now();
            let following = NextRun::after(
                &held.schedule,
                self.zone_of(&held.schedule),
                started,
                self.random.draw(),
            );
            held.next = self.keep(&plan, following).await;
        }
    }

    /// Starts `plan`'s due run in a place of the queue, or says why its
    /// overlap policy skips it: its last run is still going (§5). A run that
    /// starts though the power source is `unknown` says so first; a skipped
    /// one says only why it was skipped.
    async fn start(
        &self,
        plan: &PlanId,
        held: &Planned,
        trigger: Trigger,
        unknown: Option<PlatformError>,
    ) {
        match self.queue.due(plan, held.overlap) {
            Ok(place) => {
                if let Some(error) = unknown {
                    self.runs.report(ScheduleReport::PowerUnknown {
                        plan: plan.clone(),
                        error,
                    });
                }
                self.runs.start(plan, trigger, place).await;
            }
            Err(overlapping) => self.runs.report(ScheduleReport::Overlapped {
                plan: plan.clone(),
                overlapping,
            }),
        }
    }

    /// Keeps `next` as `plan`'s planned run and returns it; a schedule with
    /// no run left is reported and forgotten.
    async fn keep(&self, plan: &PlanId, next: Result<NextRun, NoNextRun>) -> Option<NextRun> {
        let kept = match next {
            Ok(next) => {
                self.schedules
                    .plan_run(&PlannedRun {
                        plan: plan.clone(),
                        at: next.at,
                    })
                    .await
            }
            Err(NoNextRun) => {
                self.runs
                    .report(ScheduleReport::NoNextRun { plan: plan.clone() });
                self.schedules.unplan(plan).await
            }
        };
        if let Err(error) = kept {
            self.runs.report(ScheduleReport::Store(error));
        }
        next.ok()
    }

    /// How long the loop sleeps: until the first planned run, and at most
    /// [`WAKE_CHECK_EVERY`], so that it samples the clocks that often.
    fn until_next(&self, planned: &BTreeMap<PlanId, Planned>) -> SignedDuration {
        let now = self.clock.now();
        planned
            .values()
            .filter_map(|held| held.next)
            .map(|next| now.duration_until(next.at))
            .fold(WAKE_CHECK_EVERY, SignedDuration::min)
            .max(SignedDuration::ZERO)
    }

    /// The zone `schedule` is read in: the one it names, else the machine's.
    fn zone_of<'a>(&'a self, schedule: &'a Schedule) -> &'a TimeZone {
        zone_of(schedule, &self.zone)
    }
}

/// The plans of `config` that have a schedule, each with its overlap
/// policy.
fn scheduled(config: &AgentConfig) -> BTreeMap<PlanId, (Schedule, Overlap)> {
    config
        .plans()
        .iter()
        .filter_map(|plan| Some((plan.id.clone(), (plan.schedule.clone()?, plan.overlap))))
        .collect()
}

/// The zone a cron schedule is read in (§5): the one `schedule` names, else
/// `machine`, the machine's local zone.
fn zone_of<'a>(schedule: &'a Schedule, machine: &'a TimeZone) -> &'a TimeZone {
    schedule.zone().map_or(machine, ScheduleZone::zone)
}

/// Whether two schedules put runs at the same times: the same cadence, time
/// zone and jitter. Catch-up and the AC power condition don't move a planned
/// run.
fn same_runs(held: &Schedule, read: &Schedule) -> bool {
    held.cadence() == read.cadence() && held.zone() == read.zone() && held.jitter() == read.jitter()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::{Arc, Mutex, PoisonError};

    use async_trait::async_trait;
    use domain::execution::next_run::NextRun;
    use domain::execution::queue::Overlapping;
    use domain::execution::wake::WAKE_CHECK_EVERY;
    use domain::execution::{Operation, Trigger};
    use domain::planning::{
        AgentConfig, AgentSettings, Concurrency, CronExpression, Hooks, Overlap, Schedule,
        ScheduleZone,
    };
    use domain::{PlanId, SignedDuration, TimeZone, Timestamp};

    use futures::FutureExt;

    use super::{PlanRuns, Planned, Plans, ScheduleReport, Scheduler, same_runs, zone_of};
    use crate::fakes::{
        FakeClock, FakeNetwork, FakePower, InMemoryConfigStore, InMemoryScheduleStore, SeededRandom,
    };
    use crate::ports::PlatformError;
    use crate::queue::{Place, RunQueue};

    /// The runs the scheduler starts, in order, each holding its place as a
    /// run that never ends, and what it said.
    #[derive(Default)]
    struct Started {
        runs: Mutex<Vec<(PlanId, Trigger)>>,
        places: Mutex<Vec<Place>>,
        reports: Mutex<Vec<ScheduleReport>>,
    }

    impl Started {
        fn runs(&self) -> Vec<(PlanId, Trigger)> {
            self.runs
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .clone()
        }

        fn reports(&self) -> Vec<ScheduleReport> {
            self.reports
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .clone()
        }
    }

    #[async_trait]
    impl PlanRuns for Started {
        async fn start(&self, plan: &PlanId, trigger: Trigger, place: Place) {
            self.runs
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push((plan.clone(), trigger));
            self.places
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(place);
        }

        async fn resume(&self, _: Operation) {}

        fn report(&self, report: ScheduleReport) {
            self.reports
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(report);
        }
    }

    fn at(text: &str) -> Timestamp {
        text.parse().unwrap()
    }

    fn home() -> PlanId {
        PlanId::parse("home").unwrap()
    }

    fn scheduler(now: &str, runs: Arc<Started>) -> Scheduler {
        Scheduler {
            clock: Arc::new(FakeClock::new(at(now))),
            random: Arc::new(SeededRandom::new(7)),
            schedules: Arc::new(InMemoryScheduleStore::new()),
            config: Arc::new(InMemoryConfigStore::new()),
            power: Arc::new(FakePower::new()),
            network: Arc::new(FakeNetwork::new()),
            runs,
            queue: Arc::new(RunQueue::new()),
            zone: TimeZone::UTC,
            ends: crate::retry::run_ends().1,
            events: Arc::new(crate::fakes::InMemoryEventBus::new()),
        }
    }

    /// The plan `home`, scheduled daily at 03:00, with its next run at `next`.
    fn home_planned_for(next: &str) -> BTreeMap<PlanId, Planned> {
        home_planned_with(next, Overlap::Skip)
    }

    /// The same, with `overlap` as its overlap policy.
    fn home_planned_with(next: &str, overlap: Overlap) -> BTreeMap<PlanId, Planned> {
        BTreeMap::from([(
            home(),
            Planned {
                schedule: daily(),
                overlap,
                next: Some(NextRun {
                    at: at(next),
                    trigger: Trigger::Schedule,
                }),
                after_wake: false,
            },
        )])
    }

    fn one() -> Concurrency {
        Concurrency::default()
    }

    // These three would otherwise be asserted only by the scheduler
    // scenarios, where a loop that never sleeps spins inside one poll: a
    // mutant there hangs the acceptance run instead of failing it.

    #[tokio::test]
    async fn a_run_planned_for_now_starts_now_and_plans_the_one_after() {
        let runs = Arc::new(Started::default());
        let scheduler = scheduler("2026-06-10T03:00:00Z", runs.clone());
        let mut planned = home_planned_for("2026-06-10T03:00:00Z");
        scheduler.start_due(&mut planned).await;
        assert_eq!(runs.runs(), vec![(home(), Trigger::Schedule)]);
        assert_eq!(
            planned[&home()].next.map(|next| next.at),
            Some(at("2026-06-11T03:00:00Z"))
        );
    }

    #[tokio::test]
    async fn a_run_a_second_away_waits() {
        let runs = Arc::new(Started::default());
        let scheduler = scheduler("2026-06-10T02:59:59Z", runs.clone());
        let mut planned = home_planned_for("2026-06-10T03:00:00Z");
        scheduler.start_due(&mut planned).await;
        assert_eq!(runs.runs(), vec![]);
        assert_eq!(
            planned[&home()].next.map(|next| next.at),
            Some(at("2026-06-10T03:00:00Z"))
        );
    }

    /// `home` runs at 03:00 and is still going (its place is held) when a
    /// run of it comes due again; what became of the run, and of the plan's
    /// next run.
    async fn due_again_while_running(overlap: Overlap) -> (Arc<Started>, Option<Timestamp>) {
        let runs = Arc::new(Started::default());
        let scheduler = scheduler("2026-06-10T03:00:00Z", runs.clone());
        let mut planned = home_planned_with("2026-06-10T03:00:00Z", overlap);
        scheduler.start_due(&mut planned).await;
        if let Some(held) = planned.get_mut(&home()) {
            held.next = Some(NextRun {
                at: at("2026-06-10T03:00:00Z"),
                trigger: Trigger::Schedule,
            });
        }
        scheduler.start_due(&mut planned).await;
        let next = planned[&home()].next.map(|next| next.at);
        (runs, next)
    }

    #[tokio::test]
    async fn a_run_due_while_the_plan_s_last_one_runs_is_skipped_and_said_so() {
        let (runs, next) = due_again_while_running(Overlap::Skip).await;
        assert_eq!(runs.runs(), vec![(home(), Trigger::Schedule)]);
        assert_eq!(
            runs.reports(),
            vec![ScheduleReport::Overlapped {
                plan: home(),
                overlapping: Overlapping::Running
            }]
        );
        // The run after it is planned as for a run that started.
        assert_eq!(next, Some(at("2026-06-11T03:00:00Z")));
    }

    #[tokio::test]
    async fn a_plan_that_queues_starts_its_run_due_while_its_last_one_runs() {
        let (runs, next) = due_again_while_running(Overlap::Queue).await;
        assert_eq!(
            runs.runs(),
            vec![(home(), Trigger::Schedule), (home(), Trigger::Schedule)]
        );
        assert_eq!(runs.reports(), vec![]);
        assert_eq!(next, Some(at("2026-06-11T03:00:00Z")));
    }

    /// A run that would start though the power source is unknown, but that
    /// its overlap policy skips, is said as skipped only, never as starting.
    #[tokio::test]
    async fn a_run_skipped_for_overlap_on_unknown_power_says_only_the_skip() {
        let runs = Arc::new(Started::default());
        let mut scheduler = scheduler("2026-06-10T03:00:00Z", runs.clone());
        let power = Arc::new(FakePower::new());
        power.set(Err(PlatformError::TimedOut {
            from: "demo power".to_owned(),
        }));
        scheduler.power = power;
        let mut planned = home_planned_for("2026-06-10T03:00:00Z");
        let place = scheduler
            .queue
            .due(&home(), Overlap::Skip)
            .expect("an idle plan takes a place");
        if let Some(held) = planned.get_mut(&home()) {
            held.schedule = daily().with_on_ac_power(true);
        }
        scheduler.start_due(&mut planned).await;
        assert_eq!(runs.runs(), vec![]);
        assert_eq!(
            runs.reports(),
            vec![ScheduleReport::Overlapped {
                plan: home(),
                overlapping: Overlapping::Running
            }]
        );
        drop(place);
    }

    /// The limit the configuration says applies to the runs already waiting
    /// as soon as the scheduler reads it: a raised one starts them, a
    /// lowered one holds them back.
    #[tokio::test]
    async fn the_limit_read_from_the_configuration_applies_to_the_waiting_runs() {
        let scheduler = scheduler("2026-06-10T03:00:00Z", Arc::new(Started::default()));
        let docs = PlanId::parse("docs").unwrap();
        let photos = PlanId::parse("photos").unwrap();
        let running = scheduler.queue.push(&docs, one());
        let mut waiting = scheduler.queue.push(&home(), one());
        let save = |limit: i64| {
            let config = AgentConfig::new(
                AgentSettings {
                    concurrency: Concurrency::new(limit).unwrap(),
                    ..AgentSettings::default()
                },
                Hooks::default(),
                BTreeMap::new(),
                Vec::new(),
            )
            .unwrap();
            let store = scheduler.config.clone();
            async move { store.save(&config).await.unwrap() }
        };
        save(2).await;
        scheduler.refresh(&mut Plans::default()).await;
        assert!(waiting.turn().now_or_never().is_some(), "a raised limit");

        let mut third = scheduler.queue.push(&photos, Concurrency::new(2).unwrap());
        save(1).await;
        scheduler.refresh(&mut Plans::default()).await;
        drop(running);
        assert!(third.turn().now_or_never().is_none(), "a lowered limit");
        drop(waiting);
        assert!(third.turn().now_or_never().is_some());
    }

    #[test]
    fn the_loop_sleeps_until_the_first_planned_run_and_at_most_until_its_wake_check() {
        let scheduler = scheduler("2026-06-10T02:59:50Z", Arc::new(Started::default()));
        assert_eq!(
            scheduler.until_next(&home_planned_for("2026-06-10T03:00:00Z")),
            SignedDuration::from_secs(10)
        );
        assert_eq!(
            scheduler.until_next(&home_planned_for("2026-06-10T04:00:00Z")),
            WAKE_CHECK_EVERY
        );
        assert_eq!(scheduler.until_next(&BTreeMap::new()), WAKE_CHECK_EVERY);
        assert_eq!(
            scheduler.until_next(&home_planned_for("2026-06-10T02:00:00Z")),
            SignedDuration::ZERO
        );
    }

    fn daily() -> Schedule {
        Schedule::new(CronExpression::parse("0 3 * * *").unwrap())
    }

    fn named(name: &str) -> ScheduleZone {
        ScheduleZone::new(name, TimeZone::get(name).unwrap()).unwrap()
    }

    #[test]
    fn a_schedule_is_read_in_the_zone_it_names() {
        let berlin = daily().with_zone(named("Europe/Berlin"));
        let machine = TimeZone::get("Asia/Tokyo").unwrap();
        assert_eq!(
            zone_of(&berlin, &machine).iana_name(),
            Some("Europe/Berlin")
        );
    }

    #[test]
    fn a_schedule_that_names_no_zone_is_read_in_the_machine_s() {
        let machine = TimeZone::get("Asia/Tokyo").unwrap();
        assert_eq!(zone_of(&daily(), &machine).iana_name(), Some("Asia/Tokyo"));
    }

    #[test]
    fn a_change_of_time_zone_moves_the_runs() {
        let berlin = daily().with_zone(named("Europe/Berlin"));
        assert!(!same_runs(&daily(), &berlin));
        assert!(!same_runs(&berlin, &daily().with_zone(named("Asia/Tokyo"))));
        assert!(same_runs(
            &berlin,
            &daily().with_zone(named("Europe/Berlin"))
        ));
    }

    #[test]
    fn a_change_of_catch_up_or_power_keeps_the_runs() {
        let changed = daily().with_catch_up(false).with_on_ac_power(true);
        assert!(same_runs(&daily(), &changed));
    }

    #[test]
    fn a_change_of_jitter_moves_the_runs() {
        let spread = daily().with_jitter(SignedDuration::from_mins(10)).unwrap();
        assert!(!same_runs(&daily(), &spread));
    }
}
