use std::sync::Mutex;

use async_trait::async_trait;
use domain::{SignedDuration, Timestamp};
use futures::channel::oneshot;

use crate::ports::Clock;

/// A clock that only moves when told to.
///
/// In manual mode, [`advance`](Self::advance) moves time and wakes every
/// sleeper whose deadline has come. In auto-advancing mode, a sleeper jumps
/// time straight to its deadline, so a scenario that sleeps for hours
/// finishes at once with the right timestamps.
pub struct FakeClock {
    auto_advance: bool,
    state: Mutex<State>,
}

struct State {
    now: Timestamp,
    sleepers: Vec<(Timestamp, oneshot::Sender<()>)>,
}

impl State {
    fn move_to(&mut self, to: Timestamp) {
        self.now = self.now.max(to);
        let now = self.now;
        // A dropped receiver means the sleeper was cancelled; nothing to wake.
        for (_, wake) in self
            .sleepers
            .extract_if(.., |(deadline, _)| *deadline <= now)
        {
            let _ = wake.send(());
        }
    }
}

impl FakeClock {
    pub fn new(start: Timestamp) -> Self {
        Self::with_mode(start, false)
    }

    pub fn auto_advancing(start: Timestamp) -> Self {
        Self::with_mode(start, true)
    }

    fn with_mode(start: Timestamp, auto_advance: bool) -> Self {
        Self {
            auto_advance,
            state: Mutex::new(State {
                now: start,
                sleepers: Vec::new(),
            }),
        }
    }

    pub fn advance(&self, by: SignedDuration) {
        let mut state = self.lock();
        let to = state.now + by;
        state.move_to(to);
    }

    /// How many sleepers are waiting for time to move. A test driving a
    /// manual clock waits for one before each [`advance`](Self::advance):
    /// moving time before the sleeper registers would skip its deadline.
    pub fn sleepers(&self) -> usize {
        self.lock().sleepers.len()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        // A panic while holding this lock leaves plain data; keep using it.
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }
}

#[async_trait]
impl Clock for FakeClock {
    fn now(&self) -> Timestamp {
        self.lock().now
    }

    async fn sleep_until(&self, deadline: Timestamp) {
        let woken = {
            let mut state = self.lock();
            if deadline <= state.now {
                return;
            }
            if self.auto_advance {
                state.move_to(deadline);
                return;
            }
            let (wake, woken) = oneshot::channel();
            state.sleepers.push((deadline, wake));
            woken
        };
        // The sender lives in `state` until the deadline passes, so this only
        // errors if the clock itself is dropped; then nobody can advance it.
        let _ = woken.await;
    }
}
