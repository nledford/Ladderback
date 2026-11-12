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
//!
//! It also retries a failed backup (§5, decision 76): each run says how it
//! ended through the [`RunEnds`], and when a backup of a plan with a retry
//! policy failed, `RetrySchedule` decides when it runs again. A retry not
//! started yet is kept in the `ScheduleStore`, so it outlives a restart.

use std::collections::BTreeMap;
use std::pin::pin;
use std::sync::Arc;

use api::UiEvent;
use async_trait::async_trait;
use domain::execution::OperationKind;
use domain::execution::Trigger;
use domain::execution::conditions::UnmetCondition;
use domain::execution::next_run::NextRun;
use domain::execution::queue::Overlapping;
use domain::execution::retry::{NoRetry, RetrySchedule, RetryScheduled};
use domain::execution::wake::WAKE_CHECK_EVERY;
use domain::planning::{AgentConfig, NoNextRun, Overlap, RetryPolicy, Schedule, ScheduleZone};
use domain::{OperationId, PlanId, SignedDuration, TimeZone, Timestamp};
use futures::future::{Either, select};

use crate::conditions::{NetworkWait, PowerCheck, WakeWatch, check_power, wait_for_network};
use crate::ports::{
    Clock, ConfigError, ConfigStore, EventBus, Network, PendingRetry, PlannedRun, PlatformError,
    PowerSupply, Random, ScheduleStore, StoreError,
};
use crate::queue::{Place, RunQueue};
use crate::recovery::QueuedEnd;
use crate::retry::{RunEnd, RunEnds, RunOutcome};

/// What starts the runs the scheduler and the queue decide on: the
/// composition root's backups of a plan, each on a task of its own. Not a
/// port: it is the use cases the root wires. Which runs may start when is
/// the [`RunQueue`]'s (the overlap policy and the concurrency limit, §5).
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

/// A plan's retry, as the loop holds it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Retrying {
    /// Planned and not started yet: the store keeps it until it starts.
    Pending(RetryScheduled),
    /// Started, in a place of the queue: how its run ends decides what
    /// follows.
    Started(RetryScheduled),
}

/// What the loop holds between its turns.
#[derive(Default)]
struct Plans {
    planned: BTreeMap<PlanId, Planned>,
    /// The planned runs kept from before the start, until the configuration
    /// has been read: each is resumed for its plan, and dropped for a plan
    /// that is no longer scheduled.
    kept: Option<BTreeMap<PlanId, Timestamp>>,
    /// The retry policy of each plan that has one, scheduled or not.
    policies: BTreeMap<PlanId, RetryPolicy>,
    /// Each plan's retry, pending or started.
    retries: BTreeMap<PlanId, Retrying>,
}

impl Scheduler {
    /// Runs scheduled plans until the future is dropped. On start it asks
    /// `NextRun` what becomes of each planned run kept from before, and
    /// takes up each retry kept pending; then, at least every
    /// [`WAKE_CHECK_EVERY`], whenever a run or a retry is due and whenever a
    /// backup ends, it reads the configuration again, checks for a wake,
    /// hears how the backups ended, and starts the runs and retries that
    /// are due.
    pub async fn run(&self) {
        let mut watch = WakeWatch::new(Arc::clone(&self.clock));
        let mut plans = Plans {
            kept: Some(self.kept_runs().await),
            retries: self.kept_retries().await,
            ..Plans::default()
        };
        self.refresh(&mut plans).await;
        let mut heard = None;
        loop {
            // In the order the runs ended: the one that woke the loop first.
            for end in heard
                .take()
                .into_iter()
                .chain(std::iter::from_fn(|| self.ends.try_next()))
            {
                self.ended(&mut plans, end).await;
            }
            let due = self.start_due(&mut plans.planned).await;
            self.give_way(&mut plans.retries, &due).await;
            self.start_retries(&mut plans.retries).await;
            heard = self
                .sleep(self.until_next(&plans.planned, &plans.retries))
                .await;
            self.refresh(&mut plans).await;
            // Before anything due starts: a run missed while the machine
            // slept is `NextRun::resume`'s to decide, not a run on time.
            if watch.check().is_some() {
                self.resume_all(&mut plans.planned).await;
            }
        }
    }

    /// Sleeps for `duration`, or until a backup ends, and returns that end.
    /// Once every run's sender is gone, only the sleep ends it.
    async fn sleep(&self, duration: SignedDuration) -> Option<RunEnd> {
        let ended = async {
            match self.ends.next().await {
                Some(end) => end,
                None => std::future::pending().await,
            }
        };
        match select(pin!(self.clock.sleep(duration)), pin!(ended)).await {
            Either::Left(((), _)) => None,
            Either::Right((end, _)) => Some(end),
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

    /// The retries kept pending from before the start, each as it was
    /// planned.
    async fn kept_retries(&self) -> BTreeMap<PlanId, Retrying> {
        match self.schedules.pending_retries().await {
            Ok(retries) => retries
                .into_iter()
                .map(|pending| (pending.plan, Retrying::Pending(pending.retry)))
                .collect(),
            Err(error) => {
                self.runs.report(ScheduleReport::Store(error));
                BTreeMap::new()
            }
        }
    }

    /// Reads the configuration and brings the plans in line with it: a plan
    /// first found is planned (from its kept run on start, else from now), a
    /// plan whose cadence, time zone or jitter changed is planned again from
    /// now, and a plan no longer scheduled is forgotten. A plan that no
    /// longer retries a failed backup forgets its retry. The queue runs as
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
        plans.policies = config
            .plans()
            .iter()
            .filter_map(|plan| Some((plan.id.clone(), plan.retry.clone()?)))
            .collect();
        let unretried: Vec<PlanId> = plans
            .retries
            .keys()
            .filter(|plan| !plans.policies.contains_key(*plan))
            .cloned()
            .collect();
        for plan in unretried {
            if let Some(Retrying::Pending(_)) = plans.retries.remove(&plan) {
                self.drop_retry(&plan).await;
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
    /// wait, once for all of them, for the network. Returns the plans whose
    /// run came due, started or not.
    async fn start_due(&self, planned: &mut BTreeMap<PlanId, Planned>) -> Vec<PlanId> {
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
        for plan in &due {
            let Some(held) = planned.get_mut(plan) else {
                continue;
            };
            let Some(next) = held.next else { continue };
            held.after_wake = false;
            match check_power(&held.schedule, self.power.as_ref()).await {
                PowerCheck::Met => self.start(plan, held, next.trigger, None).await,
                PowerCheck::Unknown(error) => {
                    self.start(plan, held, next.trigger, Some(error)).await;
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
            held.next = self.keep(plan, following).await;
        }
        due
    }

    /// The retries pending for the plans in `due`, whose scheduled run came
    /// due, are dropped: the series stops at that run (§5).
    async fn give_way(&self, retries: &mut BTreeMap<PlanId, Retrying>, due: &[PlanId]) {
        for plan in due {
            if let Some(Retrying::Pending(_)) = retries.get(plan) {
                retries.remove(plan);
                self.drop_retry(plan).await;
                self.runs.report(ScheduleReport::RetriesEnded {
                    plan: plan.clone(),
                    why: NoRetry::NextRun,
                });
            }
        }
    }

    /// Starts every pending retry that is due, in the order of the plans'
    /// IDs, in a place of the queue, where it waits its turn under the
    /// concurrency limit. A retry whose series' end has come is dropped, and
    /// so is one whose plan has a run going, running or waiting, whatever
    /// its overlap policy: that run takes the retry's place.
    async fn start_retries(&self, retries: &mut BTreeMap<PlanId, Retrying>) {
        let now = self.clock.now();
        let due: Vec<(PlanId, RetryScheduled)> = retries
            .iter()
            .filter_map(|(plan, retrying)| match retrying {
                Retrying::Pending(retry) if retry.at <= now => Some((plan.clone(), *retry)),
                Retrying::Pending(_) | Retrying::Started(_) => None,
            })
            .collect();
        for (plan, retry) in due {
            self.drop_retry(&plan).await;
            let place = if RetrySchedule::superseded(&retry, now) {
                Err(NoRetry::NextRun)
            } else {
                self.queue
                    .due(&plan, Overlap::Skip)
                    .map_err(|_| NoRetry::RunGoing)
            };
            match place {
                Ok(place) => {
                    retries.insert(plan.clone(), Retrying::Started(retry));
                    self.runs.start(&plan, Trigger::Retry, place).await;
                }
                Err(why) => {
                    retries.remove(&plan);
                    self.runs.report(ScheduleReport::RetriesEnded { plan, why });
                }
            }
        }
    }

    /// What follows a backup of `end.plan` that ended (§5). A success or a
    /// cancel ends the plan's retries. A failure of a run the schedule
    /// started is retried when the plan has a retry policy and no run of it
    /// is going: the first retry of a series after a run that wasn't the
    /// series' retry, else the next one. A failure of a run a person or
    /// another program asked for leaves the plan's retries as they were.
    async fn ended(&self, plans: &mut Plans, end: RunEnd) {
        if end.outcome == RunOutcome::Failed && !RetrySchedule::retried(end.trigger) {
            return;
        }
        let series = match plans.retries.remove(&end.plan) {
            Some(Retrying::Started(retry)) if end.trigger == Trigger::Retry => Some(retry),
            Some(Retrying::Pending(_)) => {
                self.drop_retry(&end.plan).await;
                None
            }
            Some(Retrying::Started(_)) | None => None,
        };
        if end.outcome != RunOutcome::Failed {
            return;
        }
        let Some(policy) = plans.policies.get(&end.plan) else {
            return;
        };
        let now = self.clock.now();
        let planned = if self.queue.is_going(&end.plan) {
            Err(NoRetry::RunGoing)
        } else if let Some(failed) = series {
            RetrySchedule::after(policy, &failed, now)
        } else {
            let next_run = plans
                .planned
                .get(&end.plan)
                .and_then(|held| held.next)
                .map(|next| next.at);
            RetrySchedule::first(policy, now, next_run)
        };
        let retry = match planned {
            Ok(retry) => retry,
            Err(why) => {
                self.runs.report(ScheduleReport::RetriesEnded {
                    plan: end.plan,
                    why,
                });
                return;
            }
        };
        let attempts = policy.attempts();
        if let Err(error) = self
            .schedules
            .keep_retry(&PendingRetry {
                plan: end.plan.clone(),
                retry,
            })
            .await
        {
            self.runs.report(ScheduleReport::Store(error));
        }
        plans
            .retries
            .insert(end.plan.clone(), Retrying::Pending(retry));
        self.events
            .publish(UiEvent::BackupRetrying {
                operation: end.operation.as_uuid().hyphenated().to_string(),
                plan: end.plan.as_str().to_owned(),
                attempt: retry.attempt,
                attempts,
                at: retry.at.to_string(),
            })
            .await;
        self.runs.report(ScheduleReport::Retrying {
            plan: end.plan,
            retry,
        });
    }

    /// Forgets `plan`'s pending retry in the store.
    async fn drop_retry(&self, plan: &PlanId) {
        if let Err(error) = self.schedules.drop_retry(plan).await {
            self.runs.report(ScheduleReport::Store(error));
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

    /// How long the loop sleeps: until the first planned run or pending
    /// retry, and at most [`WAKE_CHECK_EVERY`], so that it samples the
    /// clocks that often.
    fn until_next(
        &self,
        planned: &BTreeMap<PlanId, Planned>,
        retries: &BTreeMap<PlanId, Retrying>,
    ) -> SignedDuration {
        let now = self.clock.now();
        let runs = planned
            .values()
            .filter_map(|held| held.next)
            .map(|next| next.at);
        let pending = retries.values().filter_map(|retrying| match retrying {
            Retrying::Pending(retry) => Some(retry.at),
            Retrying::Started(_) => None,
        });
        runs.chain(pending)
            .map(|at| now.duration_until(at))
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
    use domain::execution::retry::RetryScheduled;
    use domain::execution::wake::WAKE_CHECK_EVERY;
    use domain::execution::{Operation, Trigger};
    use domain::planning::{
        AgentConfig, AgentSettings, Concurrency, CronExpression, Hooks, Overlap, Plan,
        RepositoryConfig, RepositorySettings, RetryPolicy, Schedule, ScheduleZone, Source,
    };
    use domain::secrets::{FileRead, SecretRef};
    use domain::{PlanId, RepositoryAlias, SignedDuration, TimeZone, Timestamp};

    use futures::FutureExt;

    use super::{
        PlanRuns, Planned, Plans, Retrying, ScheduleReport, Scheduler, same_runs, zone_of,
    };
    use crate::fakes::{
        FakeClock, FakeNetwork, FakePower, InMemoryConfigStore, InMemoryScheduleStore, SeededRandom,
    };
    use crate::ports::{PendingRetry, PlatformError};
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
        let none = BTreeMap::new();
        assert_eq!(
            scheduler.until_next(&home_planned_for("2026-06-10T03:00:00Z"), &none),
            SignedDuration::from_secs(10)
        );
        assert_eq!(
            scheduler.until_next(&home_planned_for("2026-06-10T04:00:00Z"), &none),
            WAKE_CHECK_EVERY
        );
        assert_eq!(
            scheduler.until_next(&BTreeMap::new(), &none),
            WAKE_CHECK_EVERY
        );
        assert_eq!(
            scheduler.until_next(&home_planned_for("2026-06-10T02:00:00Z"), &none),
            SignedDuration::ZERO
        );
    }

    fn retry_at(text: &str) -> RetryScheduled {
        RetryScheduled {
            attempt: 1,
            at: at(text),
            until: None,
        }
    }

    #[test]
    fn the_loop_sleeps_until_a_pending_retry_that_comes_first() {
        let scheduler = scheduler("2026-06-10T02:59:50Z", Arc::new(Started::default()));
        let planned = home_planned_for("2026-06-10T04:00:00Z");
        let pending = BTreeMap::from([(
            PlanId::parse("docs").unwrap(),
            Retrying::Pending(retry_at("2026-06-10T03:00:00Z")),
        )]);
        assert_eq!(
            scheduler.until_next(&planned, &pending),
            SignedDuration::from_secs(10)
        );
        let started = BTreeMap::from([(
            PlanId::parse("docs").unwrap(),
            Retrying::Started(retry_at("2026-06-10T02:59:55Z")),
        )]);
        assert_eq!(
            scheduler.until_next(&planned, &started),
            WAKE_CHECK_EVERY,
            "a retry that started has nothing left to wait for"
        );
    }

    /// A configuration with the plans `home`, which retries a failed
    /// backup, and `docs`, which doesn't.
    fn home_retries_docs_does_not() -> AgentConfig {
        let nas = RepositoryAlias::parse("nas").unwrap();
        let repository = RepositoryConfig {
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
        };
        let plan = |id: PlanId| {
            let source = Source::parse(&format!("/home/demo/{}", id.as_str())).unwrap();
            Plan::new(id, nas.clone(), vec![source])
        };
        let mut home = plan(home());
        home.retry = Some(RetryPolicy::new(3, SignedDuration::from_mins(10)).unwrap());
        AgentConfig::new(
            AgentSettings::default(),
            Hooks::default(),
            BTreeMap::from([(nas.clone(), repository)]),
            vec![home, plan(PlanId::parse("docs").unwrap())],
        )
        .unwrap()
    }

    #[tokio::test]
    async fn a_plan_that_no_longer_retries_forgets_its_pending_retry() {
        let scheduler = scheduler("2026-06-10T03:00:00Z", Arc::new(Started::default()));
        let docs = PlanId::parse("docs").unwrap();
        let pending = |plan: PlanId| PendingRetry {
            plan,
            retry: retry_at("2026-06-10T03:10:00Z"),
        };
        for plan in [home(), docs.clone()] {
            scheduler
                .schedules
                .keep_retry(&pending(plan))
                .await
                .unwrap();
        }
        scheduler
            .config
            .save(&home_retries_docs_does_not())
            .await
            .unwrap();
        let mut plans = Plans {
            retries: scheduler.kept_retries().await,
            ..Plans::default()
        };
        scheduler.refresh(&mut plans).await;
        assert_eq!(
            plans.retries.keys().cloned().collect::<Vec<_>>(),
            vec![home()]
        );
        assert_eq!(
            scheduler.schedules.pending_retries().await.unwrap(),
            vec![pending(home())]
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
