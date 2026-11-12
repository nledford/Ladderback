//! Which backups run and which wait (technical design §5): the agent's
//! concurrency limit across its plans, a plan's one backup at a time, and a
//! plan's overlap policy for a run its schedule starts. Nothing here runs
//! anything or waits: `app::queue` hands each run its turn as this decides.

use std::collections::BTreeMap;

use crate::PlanId;
use crate::planning::{Concurrency, Overlap};

/// A run's place in the queue. Tickets are issued in the order the runs
/// asked for a place, so the lowest waiting one is the oldest.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Ticket(u64);

/// Why a run a schedule starts was skipped: the plan's last run is still
/// going.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Overlapping {
    /// The last run is running, and the plan skips (`overlap = "skip"`).
    Running,
    /// A run of the plan is waiting for its turn already. Under `skip` that
    /// is its last run, still going; under `queue` it is the run that starts
    /// once the last one ends, and a second one waiting would add nothing.
    Waiting,
}

/// The backups that hold a slot and the ones waiting for one: the domain
/// side of the persistent queue (§5, the concurrency limit). At most
/// `limit` run at once, across plans; a plan has at most one running, and a
/// run its schedule starts never joins a run of its plan already waiting.
#[derive(Clone, Debug)]
pub struct BackupQueue {
    limit: Concurrency,
    /// The next ticket to issue.
    issued: u64,
    running: BTreeMap<Ticket, PlanId>,
    /// By ticket, so oldest first.
    waiting: BTreeMap<Ticket, PlanId>,
}

impl BackupQueue {
    /// An empty queue that runs at most `limit` backups at once.
    pub fn new(limit: Concurrency) -> Self {
        Self {
            limit,
            issued: 0,
            running: BTreeMap::new(),
            waiting: BTreeMap::new(),
        }
    }

    /// Runs at most `limit` backups at once from now on. Lowering it stops
    /// nothing that runs: no run starts until fewer than `limit` do.
    pub fn set_limit(&mut self, limit: Concurrency) {
        self.limit = limit;
    }

    /// A run of `plan` its schedule starts came due: it waits for its turn,
    /// unless `overlap` says it is skipped. Under either policy a plan that
    /// has a run waiting already skips it ([`Overlapping::Waiting`]); under
    /// `skip`, so does a plan whose last run is running.
    pub fn due(&mut self, plan: &PlanId, overlap: Overlap) -> Result<Ticket, Overlapping> {
        if self.waiting.values().any(|waiting| waiting == plan) {
            return Err(Overlapping::Waiting);
        }
        if overlap == Overlap::Skip && self.running.values().any(|running| running == plan) {
            return Err(Overlapping::Running);
        }
        Ok(self.push(plan))
    }

    /// A run of `plan` that waits for its turn whatever else of the plan is
    /// running or waiting: a run started by hand, or one the queue held from
    /// before the daemon started.
    pub fn push(&mut self, plan: &PlanId) -> Ticket {
        let ticket = Ticket(self.issued);
        // A daemon would have to queue a run every nanosecond for 584 years
        // to wrap it.
        self.issued = self.issued.wrapping_add(1);
        self.waiting.insert(ticket, plan.clone());
        ticket
    }

    /// Starts the waiting runs that may start now, and returns their
    /// tickets: while fewer than the limit run, the oldest waiting run whose
    /// plan has none running.
    pub fn start(&mut self) -> Vec<Ticket> {
        let mut started = Vec::new();
        while self.running.len() < usize::from(self.limit.get()) {
            let running = &self.running;
            let Some(ticket) = self
                .waiting
                .iter()
                .find(|(_, plan)| !running.values().any(|other| other == *plan))
                .map(|(ticket, _)| *ticket)
            else {
                break;
            };
            if let Some(plan) = self.waiting.remove(&ticket) {
                self.running.insert(ticket, plan);
                started.push(ticket);
            }
        }
        started
    }

    /// The run with `ticket` ended, or left the queue before its turn. A
    /// ticket the queue doesn't hold changes nothing.
    pub fn leave(&mut self, ticket: Ticket) {
        self.running.remove(&ticket);
        self.waiting.remove(&ticket);
    }

    /// Whether a run of `plan` is going: running, or waiting for its turn.
    pub fn is_going(&self, plan: &PlanId) -> bool {
        self.running
            .values()
            .chain(self.waiting.values())
            .any(|going| going == plan)
    }
}

#[cfg(test)]
mod tests {
    use super::{BackupQueue, Overlapping, Ticket};
    use crate::PlanId;
    use crate::planning::{Concurrency, Overlap};

    fn plan(name: &str) -> PlanId {
        PlanId::parse(name).unwrap()
    }

    fn limit(value: i64) -> Concurrency {
        Concurrency::new(value).unwrap()
    }

    #[test]
    fn a_run_starts_at_once_while_a_slot_is_free() {
        let mut queue = BackupQueue::new(limit(1));
        let ticket = queue.push(&plan("home"));
        assert_eq!(queue.start(), vec![ticket]);
        assert_eq!(queue.start(), Vec::<Ticket>::new());
    }

    #[test]
    fn the_limit_counts_the_running_backups_of_every_plan() {
        let mut queue = BackupQueue::new(limit(2));
        let docs = queue.push(&plan("docs"));
        let home = queue.push(&plan("home"));
        let photos = queue.push(&plan("photos"));
        assert_eq!(queue.start(), vec![docs, home]);
        queue.leave(home);
        assert_eq!(queue.start(), vec![photos]);
    }

    #[test]
    fn the_oldest_waiting_run_starts_first() {
        let mut queue = BackupQueue::new(limit(1));
        let docs = queue.push(&plan("docs"));
        let photos = queue.push(&plan("photos"));
        let home = queue.push(&plan("home"));
        assert_eq!(queue.start(), vec![docs]);
        queue.leave(docs);
        assert_eq!(queue.start(), vec![photos]);
        queue.leave(photos);
        assert_eq!(queue.start(), vec![home]);
    }

    #[test]
    fn a_plan_runs_one_backup_at_a_time_whatever_the_limit() {
        let mut queue = BackupQueue::new(limit(4));
        let first = queue.push(&plan("home"));
        let second = queue.push(&plan("home"));
        let docs = queue.push(&plan("docs"));
        // The second run of `home` waits for the first; `docs`, behind it,
        // doesn't wait for either.
        assert_eq!(queue.start(), vec![first, docs]);
        queue.leave(first);
        assert_eq!(queue.start(), vec![second]);
    }

    #[test]
    fn a_waiting_run_that_leaves_never_starts() {
        let mut queue = BackupQueue::new(limit(1));
        let docs = queue.push(&plan("docs"));
        let home = queue.push(&plan("home"));
        let photos = queue.push(&plan("photos"));
        assert_eq!(queue.start(), vec![docs]);
        queue.leave(home);
        queue.leave(docs);
        assert_eq!(queue.start(), vec![photos]);
    }

    #[test]
    fn a_ticket_the_queue_does_not_hold_changes_nothing() {
        let mut queue = BackupQueue::new(limit(1));
        let docs = queue.push(&plan("docs"));
        assert_eq!(queue.start(), vec![docs]);
        queue.leave(docs);
        let home = queue.push(&plan("home"));
        queue.leave(docs);
        assert_eq!(queue.start(), vec![home]);
        let photos = queue.push(&plan("photos"));
        queue.leave(docs);
        assert_eq!(queue.start(), Vec::<Ticket>::new());
        queue.leave(home);
        assert_eq!(queue.start(), vec![photos]);
    }

    #[test]
    fn every_place_has_its_own_ticket() {
        let mut queue = BackupQueue::new(limit(1));
        let first = queue.push(&plan("home"));
        let second = queue.push(&plan("docs"));
        let third = queue.due(&plan("photos"), Overlap::Skip).unwrap();
        assert!(
            first < second && second < third,
            "{first:?} {second:?} {third:?}"
        );
    }

    #[test]
    fn a_raised_limit_starts_the_runs_it_makes_room_for() {
        let mut queue = BackupQueue::new(limit(1));
        let docs = queue.push(&plan("docs"));
        let home = queue.push(&plan("home"));
        let photos = queue.push(&plan("photos"));
        assert_eq!(queue.start(), vec![docs]);
        queue.set_limit(limit(3));
        assert_eq!(queue.start(), vec![home, photos]);
    }

    #[test]
    fn a_lowered_limit_stops_nothing_and_starts_nothing_until_there_is_room() {
        let mut queue = BackupQueue::new(limit(2));
        let docs = queue.push(&plan("docs"));
        let home = queue.push(&plan("home"));
        assert_eq!(queue.start(), vec![docs, home]);
        let photos = queue.push(&plan("photos"));
        queue.set_limit(limit(1));
        queue.leave(docs);
        assert_eq!(queue.start(), Vec::<Ticket>::new());
        queue.leave(home);
        assert_eq!(queue.start(), vec![photos]);
    }

    #[test]
    fn a_due_run_of_an_idle_plan_waits_for_its_turn_under_either_policy() {
        for overlap in [Overlap::Skip, Overlap::Queue] {
            let mut queue = BackupQueue::new(limit(1));
            let docs = queue.push(&plan("docs"));
            assert_eq!(queue.start(), vec![docs]);
            let home = queue.due(&plan("home"), overlap);
            assert!(home.is_ok(), "{overlap:?}");
            assert_eq!(queue.start(), Vec::<Ticket>::new(), "{overlap:?}");
            queue.leave(docs);
            assert_eq!(queue.start(), home.into_iter().collect::<Vec<_>>());
        }
    }

    #[test]
    fn a_plan_that_skips_skips_a_run_due_while_its_last_one_runs() {
        let mut queue = BackupQueue::new(limit(2));
        let home = queue.push(&plan("home"));
        assert_eq!(queue.start(), vec![home]);
        assert_eq!(
            queue.due(&plan("home"), Overlap::Skip),
            Err(Overlapping::Running)
        );
        // Another plan isn't held back by it.
        assert!(queue.due(&plan("docs"), Overlap::Skip).is_ok());
    }

    #[test]
    fn a_plan_that_queues_waits_for_its_last_run_to_end() {
        let mut queue = BackupQueue::new(limit(2));
        let first = queue.push(&plan("home"));
        assert_eq!(queue.start(), vec![first]);
        let second = queue.due(&plan("home"), Overlap::Queue).unwrap();
        assert_eq!(queue.start(), Vec::<Ticket>::new());
        queue.leave(first);
        assert_eq!(queue.start(), vec![second]);
    }

    #[test]
    fn a_plan_never_has_two_runs_waiting_under_either_policy() {
        for overlap in [Overlap::Skip, Overlap::Queue] {
            let mut queue = BackupQueue::new(limit(1));
            let docs = queue.push(&plan("docs"));
            assert_eq!(queue.start(), vec![docs]);
            assert!(queue.due(&plan("home"), overlap).is_ok(), "{overlap:?}");
            assert_eq!(
                queue.due(&plan("home"), overlap),
                Err(Overlapping::Waiting),
                "{overlap:?}"
            );
        }
    }

    #[test]
    fn a_run_waiting_behind_a_running_one_of_its_plan_skips_the_next_under_queue() {
        let mut queue = BackupQueue::new(limit(1));
        let first = queue.push(&plan("home"));
        assert_eq!(queue.start(), vec![first]);
        assert!(queue.due(&plan("home"), Overlap::Queue).is_ok());
        assert_eq!(
            queue.due(&plan("home"), Overlap::Queue),
            Err(Overlapping::Waiting)
        );
        assert_eq!(
            queue.due(&plan("home"), Overlap::Skip),
            Err(Overlapping::Waiting)
        );
    }

    #[test]
    fn a_plan_whose_run_ended_runs_again_when_due() {
        let mut queue = BackupQueue::new(limit(1));
        let first = queue.due(&plan("home"), Overlap::Skip).unwrap();
        assert_eq!(queue.start(), vec![first]);
        queue.leave(first);
        let second = queue.due(&plan("home"), Overlap::Skip).unwrap();
        assert_eq!(queue.start(), vec![second]);
    }

    #[test]
    fn a_run_by_hand_waits_behind_its_plan_whatever_its_policy() {
        let mut queue = BackupQueue::new(limit(2));
        let first = queue.due(&plan("home"), Overlap::Skip).unwrap();
        assert_eq!(queue.start(), vec![first]);
        let by_hand = queue.push(&plan("home"));
        let again = queue.push(&plan("home"));
        assert_eq!(queue.start(), Vec::<Ticket>::new());
        queue.leave(first);
        assert_eq!(queue.start(), vec![by_hand]);
        queue.leave(by_hand);
        assert_eq!(queue.start(), vec![again]);
    }

    #[test]
    fn a_plan_is_going_while_a_run_of_it_waits_or_runs() {
        let mut queue = BackupQueue::new(limit(1));
        let docs = queue.push(&plan("docs"));
        let home = queue.push(&plan("home"));
        assert!(!queue.is_going(&plan("photos")));
        assert!(queue.is_going(&plan("docs")), "waiting");
        assert_eq!(queue.start(), vec![docs]);
        assert!(queue.is_going(&plan("docs")), "running");
        assert!(queue.is_going(&plan("home")), "waiting behind the limit");
        queue.leave(docs);
        assert!(!queue.is_going(&plan("docs")));
        queue.leave(home);
        assert!(!queue.is_going(&plan("home")));
    }
}
