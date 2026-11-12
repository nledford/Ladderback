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
use domain::execution::wake::WAKE_CHECK_EVERY;
use domain::planning::{AgentConfig, NoNextRun, Schedule, ScheduleZone};
use domain::{OperationId, PlanId, SignedDuration, TimeZone, Timestamp};

use crate::conditions::{NetworkWait, PowerCheck, WakeWatch, check_power, wait_for_network};
use crate::ports::{
    Clock, ConfigError, ConfigStore, Network, PlannedRun, PlatformError, PowerSupply, Random,
    ScheduleStore, StoreError,
};
use crate::recovery::QueuedEnd;

/// What starts the runs the scheduler and the queue decide on: the
/// composition root's backups of a plan, each on a task of its own. Not a
/// port: it is the use cases the root wires, and the seam where the overlap
/// policy and the limit on concurrent runs (#276) and retries (#275) go.
#[async_trait]
pub trait PlanRuns: Send + Sync {
    /// Starts a backup of `plan`, started by `trigger`, as a new operation,
    /// and returns once the operation log has it queued. A run that can't
    /// start (a secret that can't be resolved) is the implementation's to
    /// say in the daemon's log.
    async fn start(&self, plan: &PlanId, trigger: Trigger);

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
    /// The machine's time zone, which a cron schedule that names none is
    /// read in.
    pub zone: TimeZone,
}

/// A scheduled plan, as the loop holds it.
#[derive(Clone, Debug)]
struct Planned {
    schedule: Schedule,
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
    /// now, and a plan no longer scheduled is forgotten. A configuration that
    /// can't be read changes nothing.
    async fn refresh(&self, plans: &mut Plans) {
        let config = match self.config.load().await {
            Ok(config) => config,
            Err(error) => {
                self.runs.report(ScheduleReport::Config(error));
                return;
            }
        };
        let now = self.clock.now();
        let scheduled = scheduled(&config);
        let mut kept = plans.kept.take().unwrap_or_default();
        for (plan, schedule) in &scheduled {
            let next = match plans.planned.get_mut(plan) {
                Some(held) if same_runs(&held.schedule, schedule) => {
                    // Its conditions may have changed; its runs haven't.
                    held.schedule = schedule.clone();
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
    /// after the plan's conditions, and plans each plan's following run from
    /// the time its run started (or was skipped). The catch-up runs after a
    /// wake first wait, once for all of them, for the network.
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
                PowerCheck::Met => self.runs.start(&plan, next.trigger).await,
                PowerCheck::Unknown(error) => {
                    self.runs.report(ScheduleReport::PowerUnknown {
                        plan: plan.clone(),
                        error,
                    });
                    self.runs.start(&plan, next.trigger).await;
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

/// The plans of `config` that have a schedule.
fn scheduled(config: &AgentConfig) -> BTreeMap<PlanId, Schedule> {
    config
        .plans()
        .iter()
        .filter_map(|plan| Some((plan.id.clone(), plan.schedule.clone()?)))
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
    use domain::planning::{CronExpression, Schedule, ScheduleZone};
    use domain::{SignedDuration, TimeZone};

    use super::{same_runs, zone_of};

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
