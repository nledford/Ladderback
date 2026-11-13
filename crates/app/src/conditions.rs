//! The checks a scheduled run goes through before it starts (technical
//! design §5): the plan's AC power condition, the rules of its repository's
//! locations (decision 80), the wait for the network that
//! a catch-up run after a wake makes, and the watch that notices a wake. The
//! scheduler that runs them is #274's; the network wait is bounded here,
//! whatever the platform does, so it can't hold a run back for good.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use async_trait::async_trait;
use domain::SignedDuration;
use domain::execution::conditions::{UnmetCondition, location_condition, power_condition};
use domain::execution::wake::{ClockSample, WAKE_CHECK_EVERY, Wake, WakeDetector};
use domain::planning::{CurrentNetworkName, Link, LocationRule, RuleVerdict, Schedule};
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

/// The machine's network link and, when a rule asks for it, its Wi-Fi
/// network's name, each read at most once: the scheduler makes one for the
/// plans due together, and a job's choice of location one for its rules, so
/// the rules see one network.
pub struct LinkReading {
    link: Arc<dyn NetworkLink>,
    read: Option<Option<Link>>,
    name: Option<Option<CurrentNetworkName>>,
}

impl LinkReading {
    /// A reading of `link` not taken yet.
    pub fn new(link: Arc<dyn NetworkLink>) -> Self {
        Self {
            link,
            read: None,
            name: None,
        }
    }

    /// The link, read the first time it is asked for; `None` when the
    /// platform couldn't report it.
    pub async fn link(&mut self) -> Option<Link> {
        if let Some(read) = self.read {
            return read;
        }
        let read = self.link.link().await.ok();
        self.read = Some(read);
        read
    }

    /// The Wi-Fi network's name, read the first time it is asked for, and
    /// only when the link is Wi-Fi: `None` off Wi-Fi, on no network, or when
    /// the platform couldn't read it.
    async fn name(&mut self) -> Option<CurrentNetworkName> {
        if let Some(name) = &self.name {
            return name.clone();
        }
        let name = if self.link().await == Some(Link::WiFi) {
            self.link.network_name().await.ok().flatten()
        } else {
            None
        };
        self.name = Some(name.clone());
        name
    }

    /// What `rule` says of the network: the link, and the name only when
    /// the rule lists names.
    pub async fn verdict(&mut self, rule: &LocationRule) -> RuleVerdict {
        let link = self.link().await;
        let name = if rule.asks_name() {
            self.name().await
        } else {
            None
        };
        rule.holds(link, name.as_ref())
    }
}

/// Checks the rules of the locations of a plan's repository, `rules` in
/// order (decision 80): the run is skipped only when every location has a
/// rule and each is known not to hold. The link is read only when every
/// location has a rule, so a repository with a location that has none never
/// asks the platform, and the name only when a rule lists names.
pub async fn check_location_rules(
    rules: &[Option<LocationRule>],
    reading: &mut LinkReading,
) -> Result<(), UnmetCondition> {
    if rules.is_empty() || rules.iter().any(Option::is_none) {
        return Ok(());
    }
    let link = reading.link().await;
    let name = if rules.iter().flatten().any(LocationRule::asks_name) {
        reading.name().await
    } else {
        None
    };
    location_condition(rules, link, name.as_ref())
}

/// A network link whose reads of the Wi-Fi network's name are watched
/// (decision 80, point 10): whether the last read failed, for the
/// Repositories page's note on a rule with names, and the first failure
/// since the daemon started, told once to its log. Not being on a network
/// is no failure: the name is then simply absent.
pub struct NameWatch {
    inner: Arc<dyn NetworkLink>,
    unreadable: AtomicBool,
    told: AtomicBool,
    checked: AtomicBool,
    tell: Box<dyn Fn() + Send + Sync>,
}

impl NameWatch {
    /// Watches `inner`, calling `tell` the first time a name can't be read.
    pub fn new(inner: Arc<dyn NetworkLink>, tell: impl Fn() + Send + Sync + 'static) -> Self {
        Self {
            inner,
            unreadable: AtomicBool::new(false),
            told: AtomicBool::new(false),
            checked: AtomicBool::new(false),
            tell: Box::new(tell),
        }
    }

    /// Whether this machine reads the name, as far as is known: until a
    /// read fails, it is taken to.
    pub fn reads_names(&self) -> bool {
        !self.unreadable.load(Ordering::Relaxed)
    }

    /// Reads the name once per start, the first time a rule with names is
    /// seen, so the page can note a rule this machine can't evaluate before
    /// any run has tried it. Later calls do nothing.
    pub async fn check_once(&self) {
        if !self.checked.swap(true, Ordering::Relaxed) {
            // What the read found is kept by the watch itself.
            drop(self.network_name().await);
        }
    }
}

#[async_trait]
impl NetworkLink for NameWatch {
    async fn link(&self) -> Result<Link, PlatformError> {
        self.inner.link().await
    }

    async fn network_name(&self) -> Result<Option<CurrentNetworkName>, PlatformError> {
        let answer = self.inner.network_name().await;
        let failed = answer.is_err();
        self.unreadable.store(failed, Ordering::Relaxed);
        if failed && !self.told.swap(true, Ordering::Relaxed) {
            (self.tell)();
        }
        answer
    }
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
        Some(LocationRule::new(vec![NetworkKind::Wired.into()]).unwrap())
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

    fn only_office() -> Option<LocationRule> {
        let office = domain::planning::NetworkName::parse("Office").unwrap();
        let names = domain::planning::NameFilter::Only(vec![office]);
        Some(
            LocationRule::new(vec![
                domain::planning::Alternative::new(NetworkKind::WiFi, Some(names)).unwrap(),
            ])
            .unwrap(),
        )
    }

    fn on(name: &str) -> Arc<FakeNetworkLink> {
        let link = Arc::new(FakeNetworkLink::new());
        link.set(Ok(Link::WiFi));
        link.set_name(Ok(CurrentNetworkName::new(name.as_bytes())));
        link
    }

    #[test]
    fn the_name_is_read_once_and_only_for_a_rule_that_lists_names() {
        let link = on("Guest");
        let mut reading = LinkReading::new(link.clone());
        let checked = check_location_rules(&[wired(), wired()], &mut reading).now_or_never();
        assert_eq!(checked, Some(Err(UnmetCondition::NoLocationRuleHolds)));
        assert_eq!(link.names_asked(), 0);
        for _ in 0..2 {
            let checked = check_location_rules(&[only_office()], &mut reading).now_or_never();
            assert_eq!(checked, Some(Err(UnmetCondition::NoLocationRuleHolds)));
        }
        assert_eq!(link.names_asked(), 1);
    }

    #[test]
    fn the_name_is_not_read_off_wi_fi() {
        let link = on("Office");
        link.set(Ok(Link::Wired));
        let mut reading = LinkReading::new(link.clone());
        let checked = check_location_rules(&[only_office()], &mut reading).now_or_never();
        assert_eq!(checked, Some(Err(UnmetCondition::NoLocationRuleHolds)));
        assert_eq!(link.names_asked(), 0);
    }

    #[test]
    fn a_name_the_platform_cannot_read_never_skips_a_run() {
        let link = on("Office");
        link.set_name(Err(PlatformError::Unreadable {
            from: "fake".to_owned(),
            detail: "hidden".to_owned(),
        }));
        let mut reading = LinkReading::new(link);
        let checked = check_location_rules(&[only_office()], &mut reading).now_or_never();
        assert_eq!(checked, Some(Ok(())));
    }

    #[test]
    fn a_failed_name_read_is_told_once_and_noted_until_a_read_succeeds() {
        let link = on("Office");
        let told = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counted = Arc::clone(&told);
        let watch = NameWatch::new(link.clone(), move || {
            counted.fetch_add(1, Ordering::Relaxed);
        });
        assert!(watch.reads_names());
        let hidden = || PlatformError::Unreadable {
            from: "fake".to_owned(),
            detail: "hidden".to_owned(),
        };
        link.set_name(Err(hidden()));
        for _ in 0..2 {
            assert_eq!(watch.network_name().now_or_never(), Some(Err(hidden())));
            assert!(!watch.reads_names());
        }
        assert_eq!(told.load(Ordering::Relaxed), 1);
        link.set_name(Ok(None));
        assert_eq!(watch.network_name().now_or_never(), Some(Ok(None)));
        assert!(watch.reads_names());
    }

    #[test]
    fn the_first_check_reads_the_name_once_per_start() {
        let link = on("Office");
        let watch = NameWatch::new(link.clone(), || {});
        for _ in 0..3 {
            assert_eq!(watch.check_once().now_or_never(), Some(()));
        }
        assert_eq!(link.names_asked(), 1);
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
