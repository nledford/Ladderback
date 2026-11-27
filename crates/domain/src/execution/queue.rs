//! Which runs go and which wait (technical design §5): the agent's
//! concurrency limit across its plans, a plan's one backup at a time, and a
//! plan's overlap policy for a run its schedule starts; and the same for
//! maintenance runs, under their own limit and keyed by their repository
//! (decision 75, point 8). Nothing here runs anything or waits: `app::queue`
//! hands each run its turn as this decides.

use std::collections::{BTreeMap, BTreeSet};

use crate::planning::{Concurrency, Overlap};
use crate::{PlanId, RepositoryAlias};

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

/// Why a run started by hand was refused: a run of its plan is waiting for
/// its turn already, and backs up the sources as they are when it starts,
/// so a second one waiting would add nothing (decision 75, point 4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RunWaiting;

/// The runs that hold a slot and the ones waiting for one, each under its
/// key: the domain side of the persistent queue (§5, the concurrency
/// limit). At most `limit` run at once, across keys; a key has at most one
/// running, and neither a run its schedule starts nor one started by hand
/// joins a run of its key already waiting.
#[derive(Clone, Debug)]
pub struct Queue<K> {
    limit: Concurrency,
    /// The next ticket to issue.
    issued: u64,
    running: BTreeMap<Ticket, K>,
    /// By ticket, so oldest first.
    waiting: BTreeMap<Ticket, K>,
    /// Held for a restart: no waiting run starts again (decision 94).
    held: bool,
    /// The keys whose runs from before the start are still to be handed
    /// back to the queue (decision 73).
    resuming: BTreeSet<K>,
}

/// The backups, keyed by their plan.
pub type BackupQueue = Queue<PlanId>;

/// The maintenance runs (forget, prune, check, adoption), keyed by their
/// repository, under `maintenance-concurrency` (decision 75, point 8).
pub type MaintenanceQueue = Queue<RepositoryAlias>;

impl<K: Ord + Clone> Queue<K> {
    /// An empty queue that runs at most `limit` runs at once.
    pub fn new(limit: Concurrency) -> Self {
        Self {
            limit,
            issued: 0,
            running: BTreeMap::new(),
            waiting: BTreeMap::new(),
            held: false,
            resuming: BTreeSet::new(),
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
    pub fn due(&mut self, plan: &K, overlap: Overlap) -> Result<Ticket, Overlapping> {
        if self.has_waiting(plan) {
            return Err(Overlapping::Waiting);
        }
        if overlap == Overlap::Skip && self.running.values().any(|running| running == plan) {
            return Err(Overlapping::Running);
        }
        Ok(self.push(plan))
    }

    /// A run of `plan` started by hand: it waits for its turn behind a run
    /// of the plan that is running, and is refused while one is waiting
    /// ([`RunWaiting`]), whichever way that one came: a schedule, a person,
    /// or the queue held from before the start, handed back or still to be
    /// ([`Self::resuming`]).
    pub fn by_hand(&mut self, plan: &K) -> Result<Ticket, RunWaiting> {
        if self.has_waiting(plan) || self.resuming.contains(plan) {
            return Err(RunWaiting);
        }
        Ok(self.push(plan))
    }

    /// Whether a run of `plan` waits for its turn.
    fn has_waiting(&self, plan: &K) -> bool {
        self.waiting.values().any(|waiting| waiting == plan)
    }

    /// A run of `plan` the queue held from before the daemon started: it
    /// waits for its turn whatever else of the plan is running or waiting,
    /// since the log has it queued already. The plan is handed back.
    pub fn push(&mut self, plan: &K) -> Ticket {
        let ticket = Ticket(self.issued);
        // A daemon would have to queue a run every nanosecond for 584 years
        // to wrap it.
        self.issued = self.issued.wrapping_add(1);
        self.waiting.insert(ticket, plan.clone());
        self.resuming.remove(plan);
        ticket
    }

    /// Starts the waiting runs that may start now, and returns their
    /// tickets: while fewer than the limit run, the oldest waiting run whose
    /// plan has none running. A held queue starts none.
    pub fn start(&mut self) -> Vec<Ticket> {
        let mut started = Vec::new();
        while !self.held && self.running.len() < usize::from(self.limit.get()) {
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
    pub fn is_going(&self, plan: &K) -> bool {
        self.going(plan).is_some()
    }

    /// The run of `key` that is going: the one running, or else the oldest
    /// waiting for its turn; `None` when none is.
    pub fn going(&self, key: &K) -> Option<Ticket> {
        let of_key = |(ticket, going): (&Ticket, &K)| (going == key).then_some(*ticket);
        self.running
            .iter()
            .find_map(of_key)
            .or_else(|| self.waiting.iter().find_map(of_key))
    }

    /// Whether any backup holds a place: running, or waiting for its turn.
    pub fn is_busy(&self) -> bool {
        !self.running.is_empty() || !self.waiting.is_empty()
    }

    /// Holds the queue for a restart that waits for the running runs to
    /// end: from now on no waiting run starts. A backup that asks for a
    /// place waits, for the daemon that starts next to run (decision 94); a
    /// maintenance run is refused one, and one still waiting ends at the next
    /// start (decision 75, point 8). The restart ends the hold, or
    /// [`Self::release`] when it is canceled.
    pub fn hold(&mut self) {
        self.held = true;
    }

    /// Whether a restart holds the queue ([`Self::hold`]).
    pub fn is_held(&self) -> bool {
        self.held
    }

    /// Lets go of the hold of a restart that was canceled: the waiting runs
    /// start again at the next [`Self::start`], under the limit.
    pub fn release(&mut self) {
        self.held = false;
    }

    /// The plans of the runs the queue held from before the daemon started,
    /// which the daemon is still handing back to it (decision 73): until a
    /// plan's run takes its place again ([`Self::push`]), or the daemon has
    /// handed back every one ([`Self::resumed`]), a run of the plan started
    /// by hand is refused as if that run waited already ([`RunWaiting`]).
    pub fn resuming(&mut self, plans: impl IntoIterator<Item = K>) {
        self.resuming.extend(plans);
    }

    /// The daemon has handed back, or ended, every run the queue held from
    /// before the start: no plan is refused for one from now on.
    pub fn resumed(&mut self) {
        self.resuming.clear();
    }

    /// How many runs hold a slot: running, past their turn.
    pub fn running(&self) -> usize {
        self.running.len()
    }
}

#[cfg(test)]
mod tests {
    use super::{BackupQueue, Overlapping, RunWaiting, Ticket};
    use crate::PlanId;
    use crate::planning::{Concurrency, Overlap};

    fn plan(name: &str) -> PlanId {
        PlanId::parse(name).unwrap()
    }

    fn limit(value: i64) -> Concurrency {
        Concurrency::new(value).unwrap()
    }

    #[test]
    fn the_run_going_is_the_running_one_or_else_the_oldest_waiting() {
        let mut queue = BackupQueue::new(limit(1));
        assert_eq!(queue.going(&plan("home")), None);
        let docs = queue.push(&plan("docs"));
        let first = queue.push(&plan("home"));
        queue.push(&plan("home"));
        assert_eq!(
            queue.going(&plan("home")),
            Some(first),
            "the oldest waiting"
        );
        assert_eq!(queue.start(), vec![docs]);
        let later = queue.push(&plan("docs"));
        assert_eq!(queue.going(&plan("docs")), Some(docs), "the running one");
        queue.leave(docs);
        assert_eq!(queue.going(&plan("docs")), Some(later));
        assert_eq!(queue.going(&plan("photos")), None);
    }

    #[test]
    fn a_queue_is_busy_while_a_run_is_waiting_or_running() {
        let mut queue = BackupQueue::new(limit(1));
        assert!(!queue.is_busy());
        let home = queue.push(&plan("home"));
        assert!(queue.is_busy(), "waiting");
        assert_eq!(queue.start(), vec![home]);
        assert!(queue.is_busy(), "running");
        queue.leave(home);
        assert!(!queue.is_busy());
    }

    #[test]
    fn a_queue_is_held_from_a_restarts_hold_until_its_release() {
        let mut queue = BackupQueue::new(limit(1));
        assert!(!queue.is_held());
        queue.hold();
        assert!(queue.is_held());
        queue.release();
        assert!(!queue.is_held());
    }

    #[test]
    fn a_held_queue_starts_no_waiting_run_while_its_running_ones_end() {
        let mut queue = BackupQueue::new(limit(1));
        let home = queue.push(&plan("home"));
        let docs = queue.push(&plan("docs"));
        assert_eq!(queue.start(), vec![home]);
        queue.hold();
        let photos = queue.push(&plan("photos"));
        assert_eq!(queue.running(), 1);
        queue.leave(home);
        assert_eq!(queue.start(), Vec::<Ticket>::new());
        assert_eq!(queue.running(), 0);
        assert!(queue.is_going(&plan("docs")) && queue.is_going(&plan("photos")));
        // Left before their turn, as a cancel takes them out.
        queue.leave(docs);
        queue.leave(photos);
        assert!(!queue.is_going(&plan("docs")));
    }

    #[test]
    fn a_released_queue_starts_its_waiting_runs_oldest_first_under_the_limit() {
        let mut queue = BackupQueue::new(limit(1));
        queue.hold();
        let home = queue.push(&plan("home"));
        queue.push(&plan("docs"));
        assert_eq!(queue.start(), Vec::<Ticket>::new());
        queue.release();
        assert_eq!(queue.start(), vec![home]);
    }

    #[test]
    fn the_running_count_is_the_runs_past_their_turn() {
        let mut queue = BackupQueue::new(limit(2));
        assert_eq!(queue.running(), 0);
        queue.push(&plan("home"));
        queue.push(&plan("docs"));
        queue.push(&plan("photos"));
        assert_eq!(queue.running(), 0, "waiting runs aren't running");
        queue.start();
        assert_eq!(queue.running(), 2);
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
    fn a_run_by_hand_waits_behind_a_running_run_of_its_plan_whatever_its_policy() {
        for overlap in [Overlap::Skip, Overlap::Queue] {
            let mut queue = BackupQueue::new(limit(2));
            let first = queue.due(&plan("home"), overlap).unwrap();
            assert_eq!(queue.start(), vec![first]);
            let by_hand = queue
                .by_hand(&plan("home"))
                .expect("a running run doesn't wait");
            assert_eq!(queue.start(), Vec::<Ticket>::new(), "{overlap:?}");
            queue.leave(first);
            assert_eq!(queue.start(), vec![by_hand], "{overlap:?}");
        }
    }

    #[test]
    fn a_run_by_hand_is_refused_while_a_run_of_its_plan_waits_however_it_came() {
        let waiting: [fn(&mut BackupQueue) -> Ticket; 4] = [
            |queue| queue.due(&plan("home"), Overlap::Skip).unwrap(),
            |queue| queue.due(&plan("home"), Overlap::Queue).unwrap(),
            |queue| queue.by_hand(&plan("home")).unwrap(),
            |queue| queue.push(&plan("home")),
        ];
        for (way, wait) in waiting.into_iter().enumerate() {
            let mut queue = BackupQueue::new(limit(1));
            let docs = queue.push(&plan("docs"));
            assert_eq!(queue.start(), vec![docs]);
            let home = wait(&mut queue);
            assert_eq!(queue.by_hand(&plan("home")), Err(RunWaiting), "way {way}");
            assert!(queue.by_hand(&plan("photos")).is_ok(), "way {way}");
            queue.leave(docs);
            assert_eq!(
                queue.start(),
                vec![home],
                "way {way}: the refused run took no place"
            );
        }
    }

    #[test]
    fn a_run_by_hand_is_refused_while_its_plans_run_from_before_the_start_is_handed_back() {
        let mut queue = BackupQueue::new(limit(1));
        queue.resuming([plan("home"), plan("docs")]);
        assert_eq!(queue.by_hand(&plan("home")), Err(RunWaiting));
        assert_eq!(queue.by_hand(&plan("docs")), Err(RunWaiting));
        let photos = queue
            .by_hand(&plan("photos"))
            .expect("a plan with no run from before the start");
        assert_eq!(
            queue.start(),
            vec![photos],
            "the refused runs took no place"
        );
    }

    #[test]
    fn a_plans_run_from_before_the_start_back_in_the_queue_is_the_one_it_waits_on() {
        let mut queue = BackupQueue::new(limit(1));
        queue.resuming([plan("home"), plan("docs")]);
        let held = queue.push(&plan("home"));
        assert_eq!(queue.by_hand(&plan("home")), Err(RunWaiting), "it waits");
        assert_eq!(queue.start(), vec![held]);
        let by_hand = queue
            .by_hand(&plan("home"))
            .expect("its run from before the start is running, not waiting");
        assert_eq!(
            queue.by_hand(&plan("docs")),
            Err(RunWaiting),
            "still handed back"
        );
        queue.leave(held);
        assert_eq!(queue.start(), vec![by_hand]);
    }

    #[test]
    fn once_every_run_from_before_the_start_is_handed_back_no_plan_is_refused_for_one() {
        let mut queue = BackupQueue::new(limit(1));
        queue.resuming([plan("home")]);
        queue.resumed();
        let home = queue
            .by_hand(&plan("home"))
            .expect("its run from before the start ended rather than run");
        assert_eq!(queue.start(), vec![home]);
    }

    #[test]
    fn a_run_by_hand_that_waits_skips_its_plans_due_run_under_either_policy() {
        for overlap in [Overlap::Skip, Overlap::Queue] {
            let mut queue = BackupQueue::new(limit(1));
            let docs = queue.push(&plan("docs"));
            assert_eq!(queue.start(), vec![docs]);
            queue.by_hand(&plan("home")).unwrap();
            assert_eq!(
                queue.due(&plan("home"), overlap),
                Err(Overlapping::Waiting),
                "{overlap:?}"
            );
        }
    }

    #[test]
    fn a_run_by_hand_waits_behind_its_plan_whatever_its_policy() {
        let mut queue = BackupQueue::new(limit(2));
        let first = queue.due(&plan("home"), Overlap::Skip).unwrap();
        assert_eq!(queue.start(), vec![first]);
        let held = queue.push(&plan("home"));
        let again = queue.push(&plan("home"));
        assert_eq!(queue.start(), Vec::<Ticket>::new());
        queue.leave(first);
        assert_eq!(queue.start(), vec![held]);
        queue.leave(held);
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
