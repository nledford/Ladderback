//! The checks a scheduled run goes through before it starts (technical
//! design §5): the plan's AC power condition, the rules of its repository's
//! locations (decision 80), the wait for the network that
//! a catch-up run after a wake makes, and the watch that notices a wake. The
//! scheduler that runs them is #274's; the network wait is bounded here,
//! whatever the platform does, so it can't hold a run back for good.

use std::sync::Arc;

use domain::SignedDuration;
use domain::execution::conditions::{UnmetCondition, location_condition, power_condition};
use domain::execution::wake::{ClockSample, WAKE_CHECK_EVERY, Wake, WakeDetector};
use domain::planning::{Link, LocationRule, Schedule};
use futures::future::{Either, select};

use crate::ports::{Clock, Network, NetworkLink, PlatformError, PowerSupply};

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
        !matches!(self, Self::Unmet(_))
    }
}

/// Checks the AC power condition of `schedule`. A plan that doesn't ask for
/// AC power never asks the platform.
pub async fn check_power(schedule: &Schedule, power: &dyn PowerSupply) -> PowerCheck {
    if !schedule.on_ac_power() {
        return PowerCheck::Met;
    }
    match power.source().await {
        Ok(source) => match power_condition(schedule, Some(source)) {
            Ok(()) => PowerCheck::Met,
            Err(unmet) => PowerCheck::Unmet(unmet),
        },
        Err(error) => PowerCheck::Unknown(error),
    }
}

/// The machine's network link, read at most once: the scheduler makes one
/// for the plans due together, so their rules all see one network.
pub struct LinkReading {
    link: Arc<dyn NetworkLink>,
    read: Option<Option<Link>>,
}

impl LinkReading {
    /// A reading of `link` not taken yet.
    pub fn new(link: Arc<dyn NetworkLink>) -> Self {
        Self { link, read: None }
    }

    /// The link, read the first time it is asked for; `None` when the
    /// platform couldn't report it.
    pub async fn link(&mut self) -> Option<Link> {
        todo!("{:?}", self.read)
    }
}

/// Checks the rules of the locations of a plan's repository, `rules` in
/// order (decision 80): the run is skipped only when every location has a
/// rule and each is known not to hold. The link is read only when every
/// location has a rule, so a repository with a location that has none never
/// asks the platform.
pub async fn check_location_rules(
    rules: &[Option<LocationRule>],
    reading: &mut LinkReading,
) -> Result<(), UnmetCondition> {
    todo!(
        "{rules:?} {:?}",
        location_condition(rules, reading.read.flatten())
    )
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
    let start = clock.monotonic();
    let remaining = || NETWORK_WAIT.saturating_sub(clock.monotonic().saturating_sub(start));
    loop {
        let left = remaining();
        if left <= SignedDuration::ZERO {
            return NetworkWait::GaveUp;
        }
        match select(network.is_online(), clock.sleep(left)).await {
            Either::Left((Ok(true), _)) => {
                return NetworkWait::Online {
                    waited: clock.monotonic().saturating_sub(start),
                };
            }
            Either::Left((Ok(false), _)) => {}
            Either::Left((Err(error), _)) => return NetworkWait::Unknown(error),
            Either::Right(((), _)) => return NetworkWait::GaveUp,
        }
        clock.sleep(NETWORK_CHECK_EVERY.min(remaining())).await;
    }
}

/// Notices the machine waking from sleep, sampling the clocks every
/// [`WAKE_CHECK_EVERY`] of monotonic time.
pub struct WakeWatch {
    clock: Arc<dyn Clock>,
    detector: WakeDetector,
}

impl WakeWatch {
    /// Starts watching from the clocks' current reading.
    pub fn new(clock: Arc<dyn Clock>) -> Self {
        let detector = WakeDetector::new(sample(clock.as_ref()));
        Self { clock, detector }
    }

    /// Returns at the first sample that shows a wake.
    pub async fn next_wake(&mut self) -> Wake {
        loop {
            self.clock.sleep(WAKE_CHECK_EVERY).await;
            if let Some(wake) = self.check() {
                return wake;
            }
        }
    }

    /// Samples the clocks now, and returns the wake the sample shows since
    /// the one before: for a loop that sleeps on its own timers, as the
    /// scheduler does, and samples at least every [`WAKE_CHECK_EVERY`].
    pub fn check(&mut self) -> Option<Wake> {
        self.detector.sample(sample(self.clock.as_ref()))
    }
}

fn sample(clock: &dyn Clock) -> ClockSample {
    ClockSample {
        wall: clock.now(),
        monotonic: clock.monotonic(),
    }
}

#[cfg(test)]
#[cfg(feature = "fakes")]
mod tests {
    use std::task::{Context, Poll, Waker};

    use async_trait::async_trait;
    use domain::Timestamp;
    use futures::FutureExt;

    use domain::planning::NetworkKind;

    use super::*;
    use crate::fakes::{FakeClock, FakeNetworkLink};

    fn wired() -> Option<LocationRule> {
        Some(LocationRule::new(vec![NetworkKind::Wired]).unwrap())
    }

    #[test]
    fn a_repository_with_a_location_that_has_no_rule_never_reads_the_link() {
        let link = Arc::new(FakeNetworkLink::new());
        let mut reading = LinkReading::new(link.clone());
        let checked = check_location_rules(&[wired(), None], &mut reading).now_or_never();
        assert_eq!(checked, Some(Ok(())));
        assert_eq!(link.asked(), 0);
    }

    #[test]
    fn one_reading_serves_every_check_that_takes_it() {
        let link = Arc::new(FakeNetworkLink::new());
        link.set(Ok(Link::WiFi));
        let mut reading = LinkReading::new(link.clone());
        for _ in 0..3 {
            let checked = check_location_rules(&[wired()], &mut reading).now_or_never();
            assert_eq!(checked, Some(Err(UnmetCondition::NoLocationRuleHolds)));
        }
        assert_eq!(link.asked(), 1);
    }

    #[test]
    fn a_link_the_platform_cannot_report_never_skips_a_run() {
        let link = Arc::new(FakeNetworkLink::new());
        link.set(Err(PlatformError::Unreadable {
            from: "fake".to_owned(),
            detail: "gone".to_owned(),
        }));
        let mut reading = LinkReading::new(link);
        let checked = check_location_rules(&[wired()], &mut reading).now_or_never();
        assert_eq!(checked, Some(Ok(())));
    }

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
