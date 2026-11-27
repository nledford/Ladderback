//! The turns of the persistent queue (technical design §5): each backup the
//! agent starts takes a [`Place`] here, and runs once the place's turn comes.
//! What may start when is the domain's [`BackupQueue`]: the concurrency
//! limit across plans, a plan's one backup at a time, and a plan's overlap
//! policy for a run its schedule starts. This module only hands each run its
//! turn, and frees a run's slot when its place is dropped, however the run
//! ended.

use std::collections::BTreeMap;
use std::convert::Infallible;
use std::sync::{Arc, Mutex, PoisonError};

use domain::PlanId;
use domain::execution::queue::{BackupQueue, Overlapping, RunWaiting, Ticket};
use domain::planning::{Concurrency, Overlap};
use futures::channel::oneshot;

/// The agent's one queue of backups. Every backup takes its place here, so
/// the limit holds across the scheduler, the routes and the runs the queue
/// held from before the start.
pub struct RunQueue {
    state: Arc<Mutex<State>>,
}

struct State {
    queue: BackupQueue,
    /// The waiting runs' turns, sent as each starts.
    turns: BTreeMap<Ticket, oneshot::Sender<()>>,
}

/// A run's place in the queue: it waits for its [`turn`](Self::turn), then
/// holds one of the agent's slots until it is dropped. Dropping it, before
/// or after its turn, leaves the queue and starts whatever may start then.
#[must_use = "a place dropped at once leaves the queue"]
pub struct Place {
    ticket: Ticket,
    state: Arc<Mutex<State>>,
    turn: oneshot::Receiver<()>,
}

impl RunQueue {
    /// An empty queue that runs one backup at a time until it is told the
    /// configuration's limit ([`Self::set_limit`], [`Self::push`]).
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(State {
                queue: BackupQueue::new(Concurrency::default()),
                turns: BTreeMap::new(),
            })),
        }
    }

    /// Runs at most `limit` backups at once from now on, as the
    /// configuration says: a raised limit starts the waiting runs it makes
    /// room for at once, and a lowered one starts nothing until fewer run.
    pub fn set_limit(&self, limit: Concurrency) {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        state.queue.set_limit(limit);
        state.start();
    }

    /// A place for a run of `plan` its schedule starts, which came due now,
    /// or why it is skipped (`BackupQueue::due`), under the limit the
    /// scheduler last set.
    pub fn due(&self, plan: &PlanId, overlap: Overlap) -> Result<Place, Overlapping> {
        self.admit(|queue| queue.due(plan, overlap))
    }

    /// A place for a run of `plan` started by hand, by a person, an API
    /// token, an AI tool or the hub, or why it is refused: a run of the plan
    /// is waiting already (`BackupQueue::by_hand`). `limit` is the agent's
    /// concurrency, as the configuration says now, and applies from then
    /// on, refused or not.
    pub fn by_hand(&self, plan: &PlanId, limit: Concurrency) -> Result<Place, RunWaiting> {
        self.admit(|queue| {
            queue.set_limit(limit);
            queue.by_hand(plan)
        })
    }

    /// A place for a run of `plan` the queue held from before the start
    /// (`BackupQueue::push`). `limit` is the agent's concurrency, as the
    /// configuration says now, and applies from then on.
    pub fn push(&self, plan: &PlanId, limit: Concurrency) -> Place {
        let taken = self.admit(|queue| {
            queue.set_limit(limit);
            Ok::<_, Infallible>(queue.push(plan))
        });
        match taken {
            Ok(place) => place,
            Err(never) => match never {},
        }
    }

    /// A place for a run of `plan` that waited with none, under `limit`,
    /// or `RunWaiting` while a run of `plan` is going, running or waiting
    /// for its turn: it gives way to it (#1221). One step under the
    /// queue's lock, so two runs of a plan whose waits end at once can't
    /// both take one. `limit` applies from then on, refused or not.
    pub fn unless_going(&self, plan: &PlanId, limit: Concurrency) -> Result<Place, RunWaiting> {
        self.admit(|queue| {
            queue.set_limit(limit);
            if queue.is_going(plan) {
                Err(RunWaiting)
            } else {
                Ok(queue.push(plan))
            }
        })
    }

    /// Whether a run of `plan` is going: it holds a place, running or
    /// waiting for its turn.
    pub fn is_going(&self, plan: &PlanId) -> bool {
        let state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        state.queue.is_going(plan)
    }

    /// Whether any backup holds a place, running or waiting for its turn: a
    /// plan dry run gives way to it (decision 89, point 13).
    pub fn is_busy(&self) -> bool {
        let state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        state.queue.is_busy()
    }

    /// Holds the queue for a restart that waits for the running backups to
    /// end: no waiting run gets its turn from now on (`BackupQueue::hold`).
    pub fn hold(&self) {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        state.queue.hold();
    }

    /// Lets go of the hold of a restart that was canceled, and starts the
    /// waiting runs that may start now (`BackupQueue::release`).
    pub fn release(&self) {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        state.queue.release();
        state.start();
    }

    /// The plans of the runs the queue held from before the start, which the
    /// daemon is still handing back to it: until a plan's run takes its
    /// place again, or every one is handed back ([`Self::resumed`]), a run
    /// of the plan started by hand is refused (`BackupQueue::resuming`).
    pub fn resuming(&self, plans: impl IntoIterator<Item = PlanId>) {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        state.queue.resuming(plans);
    }

    /// Every run the queue held from before the start is handed back to it,
    /// or ended (`BackupQueue::resumed`).
    pub fn resumed(&self) {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        state.queue.resumed();
    }

    /// How many backups are running: past their turn, their place kept.
    pub fn running(&self) -> usize {
        let state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        state.queue.running()
    }

    /// Asks `take` for a ticket, and starts whatever may start now: the new
    /// run, or, under a limit `take` raised, runs that waited, whether or
    /// not `take` gave a ticket.
    fn admit<E>(
        &self,
        take: impl FnOnce(&mut BackupQueue) -> Result<Ticket, E>,
    ) -> Result<Place, E> {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        let place = take(&mut state.queue).map(|ticket| {
            let (sender, turn) = oneshot::channel();
            state.turns.insert(ticket, sender);
            Place {
                ticket,
                state: Arc::clone(&self.state),
                turn,
            }
        });
        state.start();
        place
    }
}

impl State {
    /// Hands their turn to the runs that may start now.
    fn start(&mut self) {
        for ticket in self.queue.start() {
            if let Some(turn) = self.turns.remove(&ticket) {
                // A place drops its receiver only as it leaves the queue.
                let _ = turn.send(());
            }
        }
    }
}

impl Default for RunQueue {
    fn default() -> Self {
        Self::new()
    }
}

impl Place {
    /// Waits until the run may start, which may be at once.
    pub async fn turn(&mut self) {
        // The sender goes only with the turn, or as the place leaves, which
        // can't happen while it waits here.
        let _ = (&mut self.turn).await;
    }
}

impl Drop for Place {
    fn drop(&mut self) {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        state.queue.leave(self.ticket);
        state.turns.remove(&self.ticket);
        state.start();
    }
}

/// The agent's one queue of maintenance runs (decision 75, point 8): forget,
/// prune, check and adoption, keyed by their repository, under the limit
/// `maintenance-concurrency` sets.
pub struct MaintenanceQueue;

/// Why a run was refused a place: a run of its key is going, running or
/// waiting for its turn. Names that run's operation, the running one first,
/// when the queue knows it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Going(pub Option<domain::OperationId>);

impl MaintenanceQueue {
    pub fn new() -> Self {
        Self
    }

    pub fn unless_going_as(
        &self,
        _repository: &domain::RepositoryAlias,
        _operation: domain::OperationId,
        _limit: Concurrency,
    ) -> Result<Place, Going> {
        todo!()
    }

    pub fn hold(&self) {
        todo!()
    }

    pub fn release(&self) {
        todo!()
    }

    pub fn running(&self) -> usize {
        todo!()
    }
}

#[cfg(test)]
mod tests {
    use domain::PlanId;
    use domain::execution::queue::{Overlapping, RunWaiting};
    use domain::planning::{Concurrency, Overlap};
    use futures::FutureExt;

    use super::{Going, MaintenanceQueue, Place, RunQueue};

    fn repository(name: &str) -> domain::RepositoryAlias {
        domain::RepositoryAlias::parse(name).unwrap()
    }

    fn operation(n: u128) -> domain::OperationId {
        domain::OperationId::from_uuid(domain::Uuid::from_u128(n))
    }

    #[test]
    fn a_maintenance_run_of_a_repository_going_is_refused_naming_the_one_going() {
        let queue = MaintenanceQueue::new();
        let mut nas = queue
            .unless_going_as(&repository("nas"), operation(1), limit(1))
            .unwrap();
        let mut laptop = queue
            .unless_going_as(&repository("laptop"), operation(2), limit(1))
            .unwrap();
        assert!(has_turn(&mut nas));
        assert!(!has_turn(&mut laptop), "the limit of one wasn't kept");
        assert_eq!(
            queue
                .unless_going_as(&repository("nas"), operation(3), limit(1))
                .err(),
            Some(Going(Some(operation(1)))),
            "a running run is named"
        );
        assert_eq!(
            queue
                .unless_going_as(&repository("laptop"), operation(4), limit(1))
                .err(),
            Some(Going(Some(operation(2)))),
            "a waiting run is named"
        );
        assert_eq!(queue.running(), 1);
        drop(nas);
        assert!(has_turn(&mut laptop));
        drop(laptop);
        assert!(
            queue
                .unless_going_as(&repository("nas"), operation(5), limit(1))
                .is_ok()
        );
    }

    fn plan(name: &str) -> PlanId {
        PlanId::parse(name).unwrap()
    }

    fn limit(value: i64) -> Concurrency {
        Concurrency::new(value).unwrap()
    }

    /// Whether `place`'s turn has come, without waiting for it.
    fn has_turn(place: &mut Place) -> bool {
        place.turn().now_or_never().is_some()
    }

    #[test]
    fn a_run_takes_its_turn_at_once_while_a_slot_is_free() {
        let queue = RunQueue::new();
        let mut home = queue.push(&plan("home"), limit(1));
        assert!(has_turn(&mut home));
    }

    #[test]
    fn the_queue_is_busy_while_a_backup_holds_a_place_running_or_waiting() {
        let queue = RunQueue::new();
        assert!(!queue.is_busy());
        queue.hold();
        let waiting = queue.push(&plan("docs"), limit(1));
        assert_eq!(queue.running(), 0);
        assert!(queue.is_busy(), "a waiting run holds a place");
        drop(waiting);
        queue.release();
        let running = queue.push(&plan("home"), limit(1));
        assert_eq!(queue.running(), 1);
        assert!(queue.is_busy(), "a running one too");
        drop(running);
        assert!(!queue.is_busy());
    }

    #[test]
    fn a_run_beyond_the_limit_waits_until_a_running_one_ends() {
        let queue = RunQueue::new();
        let mut docs = queue.push(&plan("docs"), limit(1));
        let mut home = queue.push(&plan("home"), limit(1));
        assert!(has_turn(&mut docs));
        assert!(!has_turn(&mut home));
        drop(docs);
        assert!(has_turn(&mut home));
    }

    #[test]
    fn a_place_that_leaves_before_its_turn_lets_the_next_one_start() {
        let queue = RunQueue::new();
        let mut docs = queue.push(&plan("docs"), limit(1));
        let home = queue.push(&plan("home"), limit(1));
        let mut photos = queue.push(&plan("photos"), limit(1));
        assert!(has_turn(&mut docs));
        drop(home);
        assert!(!has_turn(&mut photos));
        drop(docs);
        assert!(has_turn(&mut photos));
    }

    #[test]
    fn a_held_queue_gives_no_turn_once_its_running_backups_end() {
        let queue = RunQueue::new();
        let docs = queue.push(&plan("docs"), limit(1));
        let mut home = queue.push(&plan("home"), limit(1));
        assert_eq!(queue.running(), 1);
        queue.hold();
        // Neither a raised limit nor the end of the running one starts it.
        let mut photos = queue.push(&plan("photos"), limit(3));
        drop(docs);
        assert_eq!(queue.running(), 0);
        assert!(!has_turn(&mut home));
        assert!(!has_turn(&mut photos));
    }

    #[test]
    fn a_released_queue_gives_the_waiting_runs_their_turn_at_once() {
        let queue = RunQueue::new();
        queue.hold();
        let mut home = queue.push(&plan("home"), limit(1));
        let mut docs = queue.push(&plan("docs"), limit(1));
        assert!(!has_turn(&mut home));
        queue.release();
        assert!(has_turn(&mut home), "the oldest waiting run didn't start");
        assert!(!has_turn(&mut docs), "the limit wasn't kept");
    }

    #[test]
    fn the_limit_a_place_is_asked_for_with_applies_from_then_on() {
        let queue = RunQueue::new();
        let mut docs = queue.push(&plan("docs"), limit(1));
        let mut home = queue.push(&plan("home"), limit(1));
        assert!(has_turn(&mut docs));
        assert!(!has_turn(&mut home));
        // A raised limit starts the run waiting for it at once.
        let mut photos = queue.push(&plan("photos"), limit(3));
        assert!(has_turn(&mut home));
        assert!(has_turn(&mut photos));
    }

    #[test]
    fn a_limit_set_applies_to_the_runs_already_waiting() {
        let queue = RunQueue::new();
        let docs = queue.push(&plan("docs"), limit(1));
        let mut home = queue.push(&plan("home"), limit(1));
        queue.set_limit(limit(2));
        assert!(has_turn(&mut home), "a raised limit starts the waiting run");
        let mut photos = queue.push(&plan("photos"), limit(2));
        queue.set_limit(limit(1));
        drop(docs);
        assert!(!has_turn(&mut photos), "a lowered limit holds it back");
        drop(home);
        assert!(has_turn(&mut photos));
    }

    #[test]
    fn a_due_run_skipped_for_overlap_takes_no_place() {
        let queue = RunQueue::new();
        let mut first = queue.due(&plan("home"), Overlap::Skip).unwrap();
        assert!(has_turn(&mut first));
        assert!(matches!(
            queue.due(&plan("home"), Overlap::Skip),
            Err(Overlapping::Running)
        ));
        let mut docs = queue.due(&plan("docs"), Overlap::Skip).unwrap();
        assert!(!has_turn(&mut docs));
        drop(first);
        assert!(has_turn(&mut docs));
    }

    #[test]
    fn a_due_run_that_queues_starts_once_its_plans_last_run_ends() {
        let queue = RunQueue::new();
        queue.set_limit(limit(2));
        let mut first = queue.due(&plan("home"), Overlap::Queue).unwrap();
        let mut second = queue.due(&plan("home"), Overlap::Queue).unwrap();
        assert!(has_turn(&mut first));
        assert!(!has_turn(&mut second));
        assert!(matches!(
            queue.due(&plan("home"), Overlap::Queue),
            Err(Overlapping::Waiting)
        ));
        drop(first);
        assert!(has_turn(&mut second));
    }

    #[test]
    fn a_run_by_hand_waits_behind_its_plans_running_run() {
        let queue = RunQueue::new();
        let mut first = queue.by_hand(&plan("home"), limit(2)).unwrap();
        assert!(has_turn(&mut first));
        let mut second = queue
            .by_hand(&plan("home"), limit(2))
            .expect("a running run isn't waiting");
        assert!(!has_turn(&mut second));
        drop(first);
        assert!(has_turn(&mut second));
    }

    #[test]
    fn a_run_by_hand_is_refused_while_its_plan_has_one_waiting_and_takes_no_place() {
        let queue = RunQueue::new();
        let docs = queue.push(&plan("docs"), limit(1));
        let mut home = queue.by_hand(&plan("home"), limit(1)).unwrap();
        assert!(!has_turn(&mut home));
        assert!(matches!(
            queue.by_hand(&plan("home"), limit(1)),
            Err(RunWaiting)
        ));
        drop(docs);
        assert!(has_turn(&mut home));
        drop(home);
        assert!(!queue.is_busy(), "the refused run held no place");
    }

    #[test]
    fn a_plan_is_going_while_it_holds_a_place() {
        let queue = RunQueue::new();
        let docs = queue.push(&plan("docs"), limit(1));
        let home = queue.push(&plan("home"), limit(1));
        assert!(queue.is_going(&plan("docs")));
        assert!(queue.is_going(&plan("home")));
        assert!(!queue.is_going(&plan("photos")));
        drop(docs);
        drop(home);
        assert!(!queue.is_going(&plan("docs")));
        assert!(!queue.is_going(&plan("home")));
    }

    #[test]
    fn a_run_that_waited_takes_a_place_only_while_its_plan_has_none() {
        let queue = RunQueue::new();
        let running = queue.push(&plan("home"), limit(1));
        assert!(matches!(
            queue.unless_going(&plan("home"), limit(2)),
            Err(RunWaiting)
        ));
        let mut docs = queue
            .unless_going(&plan("docs"), limit(2))
            .expect("docs has no run going");
        assert!(has_turn(&mut docs), "under the limit it asked with");
        drop(running);
        assert!(queue.unless_going(&plan("home"), limit(2)).is_ok());
    }

    #[test]
    fn a_run_by_hand_of_a_plan_still_resuming_is_refused_until_every_one_is_resumed() {
        let queue = RunQueue::new();
        queue.resuming([plan("home")]);
        assert!(matches!(
            queue.by_hand(&plan("home"), limit(2)),
            Err(RunWaiting)
        ));
        assert!(queue.by_hand(&plan("docs"), limit(2)).is_ok());
        queue.resumed();
        assert!(queue.by_hand(&plan("home"), limit(2)).is_ok());
    }
}
