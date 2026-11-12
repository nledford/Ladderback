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
use domain::execution::queue::{BackupQueue, Overlapping, Ticket};
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

    /// A place for any other run of `plan`: one started by hand, or one the
    /// queue held from before the start (`BackupQueue::push`). `limit` is
    /// the agent's concurrency, as the configuration says now, and applies
    /// from then on.
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

    /// Whether a run of `plan` is going: it holds a place, running or
    /// waiting for its turn.
    pub fn is_going(&self, plan: &PlanId) -> bool {
        let state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        state.queue.is_going(plan)
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

#[cfg(test)]
mod tests {
    use domain::PlanId;
    use domain::execution::queue::Overlapping;
    use domain::planning::{Concurrency, Overlap};
    use futures::FutureExt;

    use super::{Place, RunQueue};

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
}
