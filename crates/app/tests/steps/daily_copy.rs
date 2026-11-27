//! Steps for `features/execution/daily_copy.feature`: the daily copy of the
//! database as the daemon keeps it while it serves, against the in-memory
//! daily copy and a clock the steps move from one of its sleeps to the
//! next.

use std::sync::{Arc, Mutex, PoisonError};

use app::daily_copy::KeepDailyCopy;
use app::fakes::{FakeClock, InMemoryDailyCopy};
use app::ports::{Clock, DailyCopy, DailyCopyError};
use cucumber::{given, then, when};
use domain::{SignedDuration, Timestamp};
use futures::FutureExt;
use futures::future::BoxFuture;

use super::Agent;

/// The most turns of the loop one step lets it take, far more than a few
/// days of daily copies need.
const MOST_TURNS: u32 = 10_000;

pub struct DailyCopyWorld {
    clock: Arc<FakeClock>,
    copies: Arc<InMemoryDailyCopy>,
    said: Arc<Mutex<Vec<DailyCopyError>>>,
    serving: Option<BoxFuture<'static, ()>>,
}

impl Default for DailyCopyWorld {
    fn default() -> Self {
        Self::at(time("2026-01-01 00:00+00:00"))
    }
}

impl std::fmt::Debug for DailyCopyWorld {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DailyCopyWorld").finish_non_exhaustive()
    }
}

impl DailyCopyWorld {
    /// A daemon whose clock reads `now`, not serving yet.
    fn at(now: Timestamp) -> Self {
        let clock = Arc::new(FakeClock::new(now));
        Self {
            copies: Arc::new(InMemoryDailyCopy::new(clock.clone())),
            clock,
            said: Arc::default(),
            serving: None,
        }
    }

    /// Keeps the daily copy, moving the clock from one of the loop's sleeps
    /// to the next, until the clock reads `until`.
    fn serve_until(&mut self, until: Timestamp) {
        let serving = self.serving.get_or_insert_with(|| {
            let said = self.said.clone();
            let keep = KeepDailyCopy {
                clock: self.clock.clone(),
                copies: self.copies.clone(),
                reports: Arc::new(move |error| {
                    said.lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .push(error);
                }),
            };
            async move { keep.run().await }.boxed()
        });
        let mut context = std::task::Context::from_waker(std::task::Waker::noop());
        for _ in 0..MOST_TURNS {
            assert!(
                serving.poll_unpin(&mut context).is_pending(),
                "the daily copy's loop ended"
            );
            let now = self.clock.now();
            if now >= until {
                return;
            }
            let mut step = now.duration_until(until);
            if let Some(next) = self.clock.until_next_sleeper() {
                step = step.min(next);
            }
            self.clock.advance(step.max(SignedDuration::ZERO));
        }
        panic!("the daily copy's loop turned {MOST_TURNS} times before {until}");
    }

    fn said(&self) -> Vec<DailyCopyError> {
        self.said
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

/// A time as the scenario writes it: `2026-06-10 03:00+02:00`.
fn time(text: &str) -> Timestamp {
    text.parse()
        .unwrap_or_else(|error| panic!("{text:?} is a time with its offset: {error}"))
}

#[given(expr = "the daemon's clock reads {string}")]
fn clock_reads(agent: &mut Agent, text: String) {
    agent.daily_copy = DailyCopyWorld::at(time(&text));
}

#[given("the database has no daily copy")]
async fn no_copy(agent: &mut Agent) {
    let copies: &dyn DailyCopy = agent.daily_copy.copies.as_ref();
    assert_eq!(copies.taken_at().await, Ok(None));
}

#[given(expr = "the daily copy was taken at {string}")]
fn copy_taken_at(agent: &mut Agent, text: String) {
    agent.daily_copy.copies.taken_before(time(&text));
}

#[given("the database is unavailable")]
fn unavailable(agent: &mut Agent) {
    agent.daily_copy.copies.fail(DailyCopyError::Unavailable {
        detail: "pool timed out while waiting for an open connection".to_owned(),
    });
}

#[when("the database becomes available again")]
fn available_again(agent: &mut Agent) {
    agent.daily_copy.copies.recover();
}

#[when(expr = "the daemon serves for {int} days and {int} hours")]
fn serves_for(agent: &mut Agent, days: i64, hours: i64) {
    let world = &mut agent.daily_copy;
    let until = world.clock.now() + SignedDuration::from_hours(24 * days + hours);
    world.serve_until(until);
}

#[when(expr = "the daemon serves until its clock reads {string}")]
fn serves_until(agent: &mut Agent, text: String) {
    agent.daily_copy.serve_until(time(&text));
}

#[then(expr = "the daily copy was taken at {string}")]
fn taken_once_at(agent: &mut Agent, at: String) {
    assert_eq!(agent.daily_copy.copies.taken(), vec![time(&at)]);
}

#[then(expr = "the daily copy was taken at {string}, {string} and {string}")]
fn taken_three_times(agent: &mut Agent, first: String, second: String, third: String) {
    assert_eq!(
        agent.daily_copy.copies.taken(),
        vec![time(&first), time(&second), time(&third)]
    );
}

#[then("no daily copy was taken")]
fn none_taken(agent: &mut Agent) {
    let taken = agent.daily_copy.copies.taken();
    assert!(taken.is_empty(), "taken at {taken:?}");
}

#[then("the daemon's log said once that the daily copy was skipped")]
fn said_skipped_once(agent: &mut Agent) {
    let said = agent.daily_copy.said();
    assert!(
        matches!(said.as_slice(), [DailyCopyError::Unavailable { .. }]),
        "said {said:?}"
    );
}
