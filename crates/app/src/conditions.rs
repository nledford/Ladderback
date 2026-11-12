//! The checks a scheduled run goes through before it starts (technical
//! design §5): the plan's AC power condition, the wait for the network that
//! a catch-up run after a wake makes, and the watch that notices a wake. The
//! scheduler that runs them is #274's; each check is bounded, so none can
//! hold a run back for good.

use std::sync::Arc;

use domain::SignedDuration;
use domain::execution::conditions::UnmetCondition;
use domain::execution::wake::Wake;
use domain::planning::Schedule;

use crate::ports::{Clock, Network, PlatformError, PowerSupply};

/// The longest a catch-up run after a wake waits for the network.
pub const NETWORK_WAIT: SignedDuration = SignedDuration::from_mins(2);

/// How often the network wait asks whether the machine is online.
pub const NETWORK_CHECK_EVERY: SignedDuration = SignedDuration::from_secs(5);

/// What the power check found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PowerCheck {
    /// The run may start: the plan asks nothing, or the machine is on AC.
    Met,
    /// The run doesn't start.
    Unmet(UnmetCondition),
    /// The run may start, though the platform couldn't say what powers the
    /// machine: why, for the agent's log.
    Unknown(PlatformError),
}

impl PowerCheck {
    /// Whether the run starts.
    pub fn lets_run(&self) -> bool {
        todo!()
    }
}

/// Checks the AC power condition of `schedule`. A plan that doesn't ask for
/// AC power never asks the platform.
pub async fn check_power(schedule: &Schedule, power: &dyn PowerSupply) -> PowerCheck {
    let _ = (schedule, power);
    todo!()
}

/// How the wait for the network ended. Whatever the end, the run goes ahead:
/// a run that fails for want of a network says so in its own result.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NetworkWait {
    /// The machine is online, after waiting this long.
    Online { waited: SignedDuration },
    /// The machine was still offline after [`NETWORK_WAIT`].
    GaveUp,
    /// The platform couldn't say whether the machine is online, so the wait
    /// ended at once: why, for the agent's log.
    Unknown(PlatformError),
}

/// Waits until the machine is online, for at most [`NETWORK_WAIT`] of
/// monotonic time, asking every [`NETWORK_CHECK_EVERY`]. A check that hangs
/// is given up at the bound too.
pub async fn wait_for_network(clock: &dyn Clock, network: &dyn Network) -> NetworkWait {
    let _ = (clock, network);
    todo!()
}

/// Notices the machine waking from sleep, sampling the clocks every
/// [`WAKE_CHECK_EVERY`](domain::execution::wake::WAKE_CHECK_EVERY).
pub struct WakeWatch {
    clock: Arc<dyn Clock>,
}

impl WakeWatch {
    /// Starts watching from the clocks' current reading.
    pub fn new(clock: Arc<dyn Clock>) -> Self {
        let _ = clock;
        todo!()
    }

    /// Returns at the first sample that shows a wake.
    pub async fn next_wake(&mut self) -> Wake {
        let _ = &self.clock;
        todo!()
    }
}

#[cfg(test)]
#[cfg(feature = "fakes")]
mod tests {
    use std::task::{Context, Poll, Waker};

    use async_trait::async_trait;
    use domain::Timestamp;
    use futures::FutureExt;

    use super::*;
    use crate::fakes::FakeClock;

    /// How the wait for `network` ends, and at what monotonic time: time
    /// moves on a second at a time, for at most ten minutes of moves, so a
    /// wait that never ends fails here even on a clock that doesn't move.
    fn ends(clock: &FakeClock, network: &dyn Network) -> (NetworkWait, SignedDuration) {
        let mut wait = wait_for_network(clock, network).boxed();
        let mut context = Context::from_waker(Waker::noop());
        for _ in 0..600 {
            if let Poll::Ready(ended) = wait.poll_unpin(&mut context) {
                return (ended, clock.monotonic());
            }
            clock.advance(SignedDuration::from_secs(1));
        }
        panic!("the wait for the network was still waiting after ten minutes");
    }

    /// A network check that never answers.
    struct Hangs;

    #[async_trait]
    impl Network for Hangs {
        async fn is_online(&self) -> Result<bool, PlatformError> {
            std::future::pending().await
        }
    }

    #[test]
    fn a_network_check_that_hangs_is_cut_off_when_the_wait_ends() {
        let clock = FakeClock::new(Timestamp::UNIX_EPOCH);
        assert_eq!(ends(&clock, &Hangs), (NetworkWait::GaveUp, NETWORK_WAIT));
    }

    /// A network check that takes 6 s to say the machine is offline. It
    /// fails past 100 checks, more than a 2 min wait makes, so a wait whose
    /// clock never sleeps fails instead of spinning.
    struct Slow(Arc<FakeClock>, std::sync::atomic::AtomicUsize);

    #[async_trait]
    impl Network for Slow {
        async fn is_online(&self) -> Result<bool, PlatformError> {
            let asked = self.1.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            assert!(asked < 100, "checked {asked} times");
            self.0.sleep(SignedDuration::from_secs(6)).await;
            Ok(false)
        }
    }

    /// Checks that end at 6 s, 17 s, … 116 s leave 4 s of the wait, so the
    /// last pause between checks is cut short to end the wait on time.
    #[test]
    fn the_last_pause_between_checks_ends_with_the_wait() {
        let clock = Arc::new(FakeClock::new(Timestamp::UNIX_EPOCH));
        assert_eq!(
            ends(&clock, &Slow(Arc::clone(&clock), Default::default())),
            (NetworkWait::GaveUp, NETWORK_WAIT)
        );
    }
}
