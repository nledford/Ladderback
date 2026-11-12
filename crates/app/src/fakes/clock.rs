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
///
/// Monotonic time starts at zero and moves with wall-clock time, except
/// while the machine sleeps ([`suspend`](Self::suspend)).
pub struct FakeClock {
    auto_advance: bool,
    state: Mutex<State>,
}

/// When a sleeper wakes: at a wall-clock time, or at a monotonic one.
#[derive(Clone, Copy)]
enum Deadline {
    Wall(Timestamp),
    Monotonic(SignedDuration),
}

struct State {
    now: Timestamp,
    monotonic: SignedDuration,
    sleepers: Vec<(Deadline, oneshot::Sender<()>)>,
}

impl State {
    fn has_passed(&self, deadline: Deadline) -> bool {
        match deadline {
            Deadline::Wall(at) => at <= self.now,
            Deadline::Monotonic(at) => at <= self.monotonic,
        }
    }

    /// Moves both clocks on by `by`, if it is positive.
    fn advance(&mut self, by: SignedDuration) {
        let by = by.max(SignedDuration::ZERO);
        self.now = self.now.saturating_add(by).unwrap_or(self.now);
        self.monotonic = self.monotonic.saturating_add(by);
        self.wake_due();
    }

    fn wake_due(&mut self) {
        // A dropped receiver means the sleeper was cancelled; nothing to wake.
        let due: Vec<_> = self
            .sleepers
            .extract_if(.., |(deadline, _)| match deadline {
                Deadline::Wall(at) => *at <= self.now,
                Deadline::Monotonic(at) => *at <= self.monotonic,
            })
            .collect();
        for (_, wake) in due {
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
                monotonic: SignedDuration::ZERO,
                sleepers: Vec::new(),
            }),
        }
    }

    pub fn advance(&self, by: SignedDuration) {
        self.lock().advance(by);
    }

    /// The machine sleeps for `by`: wall-clock time moves on, monotonic
    /// time stands still. A sleeper on wall-clock time whose deadline passes
    /// wakes; one on monotonic time waits on for the rest of its time.
    pub fn suspend(&self, by: SignedDuration) {
        let mut state = self.lock();
        let by = by.max(SignedDuration::ZERO);
        state.now = state.now.saturating_add(by).unwrap_or(state.now);
        state.wake_due();
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

    /// Waits until `deadline` passes; in auto-advancing mode, moves both
    /// clocks on to it at once.
    async fn wait_for(&self, deadline: Deadline) {
        let woken = {
            let mut state = self.lock();
            if state.has_passed(deadline) {
                return;
            }
            if self.auto_advance {
                let by = match deadline {
                    Deadline::Wall(at) => at.duration_since(state.now),
                    Deadline::Monotonic(at) => at.saturating_sub(state.monotonic),
                };
                state.advance(by);
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

#[async_trait]
impl Clock for FakeClock {
    fn now(&self) -> Timestamp {
        self.lock().now
    }

    fn monotonic(&self) -> SignedDuration {
        self.lock().monotonic
    }

    async fn sleep_until(&self, deadline: Timestamp) {
        self.wait_for(Deadline::Wall(deadline)).await;
    }

    async fn sleep(&self, duration: SignedDuration) {
        let deadline = self.lock().monotonic.saturating_add(duration);
        self.wait_for(Deadline::Monotonic(deadline)).await;
    }
}
