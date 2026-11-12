//! Steps for `features/execution/run_conditions.feature`, for the
//! fake-backed acceptance runner. Time is a manual fake clock that the steps
//! move a second at a time, polling the check under test after each move, so
//! a check that waits too long, or for ever, shows as a failed bound.

use std::sync::Arc;
use std::task::Poll;

use app::conditions::{NetworkWait, PowerCheck, WakeWatch, check_power, wait_for_network};
use app::fakes::{FakeClock, FakeNetwork, FakePower};
use app::ports::{Clock, PlatformError};
use cucumber::{given, then, when};
use domain::execution::conditions::{PowerSource, UnmetCondition};
use domain::execution::wake::Wake;
use domain::planning::{CronExpression, Schedule};
use domain::{SignedDuration, Timestamp};
use futures::FutureExt;

use super::Agent;

/// The longest the steps move time on for: a check still waiting by then
/// waits too long.
const GIVE_UP_AFTER: SignedDuration = SignedDuration::from_hours(1);

const ONE_SECOND: SignedDuration = SignedDuration::from_secs(1);

pub struct ConditionsWorld {
    clock: Arc<FakeClock>,
    power: Arc<FakePower>,
    network: Arc<FakeNetwork>,
    schedule: Option<Schedule>,
    power_check: Option<PowerCheck>,
    watch: Option<WakeWatch>,
    /// The wake the watch noticed, and how long after waking it did.
    noticed: Option<(Wake, SignedDuration)>,
    /// How long after the wake the network comes up; `None` never.
    network_up_after: Option<SignedDuration>,
    /// How the wait ended, and how long after the wake.
    waited: Option<(NetworkWait, SignedDuration)>,
}

impl Default for ConditionsWorld {
    fn default() -> Self {
        Self {
            clock: Arc::new(FakeClock::new(
                Timestamp::from_second(1_800_000_000).expect("a timestamp"),
            )),
            power: Arc::new(FakePower::new()),
            network: Arc::new(FakeNetwork::new()),
            schedule: None,
            power_check: None,
            watch: None,
            noticed: None,
            network_up_after: None,
            waited: None,
        }
    }
}

fn duration(text: &str) -> SignedDuration {
    text.parse()
        .unwrap_or_else(|e| panic!("{text:?} is no duration: {e}"))
}

fn unreadable() -> PlatformError {
    PlatformError::Unreadable {
        from: "the demo machine".to_owned(),
        detail: "nothing to read".to_owned(),
    }
}

fn schedule(on_ac_power: bool) -> Schedule {
    Schedule::new(CronExpression::parse("0 * * * *").expect("a cron expression"))
        .with_on_ac_power(on_ac_power)
}

#[given(expr = "the plan {string} asks to run only on AC power")]
fn asks_for_ac(agent: &mut Agent, _plan: String) {
    agent.conditions.schedule = Some(schedule(true));
}

#[given(expr = "the plan {string} doesn't ask for AC power")]
fn asks_nothing(agent: &mut Agent, _plan: String) {
    agent.conditions.schedule = Some(schedule(false));
}

#[given("the machine is on battery")]
fn on_battery(agent: &mut Agent) {
    agent.conditions.power.set(Ok(PowerSource::Battery));
}

#[given("the machine is on AC power")]
fn on_ac(agent: &mut Agent) {
    agent.conditions.power.set(Ok(PowerSource::Ac));
}

#[given("the machine can't tell what powers it")]
fn power_unknown(agent: &mut Agent) {
    agent.conditions.power.set(Err(unreadable()));
}

#[when(expr = "the plan {string} is due")]
async fn due(agent: &mut Agent, _plan: String) {
    let world = &mut agent.conditions;
    let schedule = world.schedule.as_ref().expect("a plan");
    world.power_check = Some(check_power(schedule, world.power.as_ref()).await);
}

fn power_check(agent: &Agent) -> &PowerCheck {
    agent
        .conditions
        .power_check
        .as_ref()
        .expect("the plan was due")
}

#[then(expr = "the run of the plan {string} is skipped because the machine is on battery")]
fn skipped_on_battery(agent: &mut Agent, _plan: String) {
    let check = power_check(agent);
    assert_eq!(check, &PowerCheck::Unmet(UnmetCondition::OnBattery));
    assert!(!check.lets_run());
}

#[then(expr = "the plan {string} runs")]
fn runs(agent: &mut Agent, _plan: String) {
    let check = power_check(agent);
    assert_eq!(check, &PowerCheck::Met);
    assert!(check.lets_run());
}

#[then(expr = "the plan {string} runs, though the machine couldn't tell what powers it")]
fn runs_unknown(agent: &mut Agent, _plan: String) {
    let check = power_check(agent);
    assert_eq!(check, &PowerCheck::Unknown(unreadable()));
    assert!(check.lets_run());
}

#[given("the agent watches for wakes")]
fn watches(agent: &mut Agent) {
    let clock: Arc<dyn Clock> = agent.conditions.clock.clone();
    agent.conditions.watch = Some(WakeWatch::new(clock));
}

/// Moves time on a second at a time, for at most `awake`, until the watch
/// notices a wake; `asleep` first, if any, with the machine asleep.
fn watch_over(world: &mut ConditionsWorld, asleep: Option<SignedDuration>, awake: SignedDuration) {
    let clock = world.clock.clone();
    let watch = world.watch.as_mut().expect("the agent watches for wakes");
    let mut next = std::pin::pin!(watch.next_wake());
    assert!(
        next.as_mut().now_or_never().is_none(),
        "no wake before the machine slept"
    );
    if let Some(asleep) = asleep {
        clock.suspend(asleep);
    }
    let mut awake_for = SignedDuration::ZERO;
    while awake_for < awake {
        clock.advance(ONE_SECOND);
        awake_for += ONE_SECOND;
        if let Some(wake) = next.as_mut().now_or_never() {
            world.noticed = Some((wake, awake_for));
            return;
        }
    }
}

#[when(regex = r"^the machine sleeps for (.+)$")]
fn sleeps(agent: &mut Agent, asleep: String) {
    watch_over(
        &mut agent.conditions,
        Some(duration(&asleep)),
        GIVE_UP_AFTER,
    );
}

#[when(expr = "{int} minutes pass with the machine awake")]
fn awake(agent: &mut Agent, minutes: i64) {
    watch_over(
        &mut agent.conditions,
        None,
        SignedDuration::from_mins(minutes),
    );
}

#[then(expr = "within {int} seconds of waking the agent notices that it slept for {int} hours")]
fn noticed_within(agent: &mut Agent, seconds: i64, hours: i64) {
    let (wake, after) = agent.conditions.noticed.expect("the agent noticed a wake");
    assert!(
        after <= SignedDuration::from_secs(seconds),
        "noticed {after} after waking"
    );
    assert_eq!(wake.slept, SignedDuration::from_hours(hours));
    assert_eq!(wake.at, agent.conditions.clock.now());
}

#[then(regex = r"^the agent notices that it slept for (.+)$")]
fn noticed(agent: &mut Agent, slept: String) {
    let (wake, _) = agent.conditions.noticed.expect("the agent noticed a wake");
    assert_eq!(wake.slept, duration(&slept));
}

#[then("the agent notices no wake")]
fn no_wake(agent: &mut Agent) {
    assert_eq!(agent.conditions.noticed, None);
}

#[given("the network is down")]
fn network_down(agent: &mut Agent) {
    agent.conditions.network.set(Ok(false));
}

#[given("the network is up")]
fn network_up(agent: &mut Agent) {
    agent.conditions.network.set(Ok(true));
}

#[given("the machine can't tell whether it is online")]
fn network_unknown(agent: &mut Agent) {
    agent.conditions.network.set(Err(unreadable()));
}

#[given(expr = "the network comes up {int} seconds after the machine wakes")]
fn comes_up(agent: &mut Agent, seconds: i64) {
    agent.conditions.network_up_after = Some(SignedDuration::from_secs(seconds));
}

#[given("the network stays down")]
fn stays_down(agent: &mut Agent) {
    agent.conditions.network_up_after = None;
}

/// The world's network, failing past 1000 checks, more than a 2 min wait
/// makes, so a wait whose clock never sleeps fails instead of spinning.
struct Capped(Arc<FakeNetwork>);

#[async_trait::async_trait]
impl app::ports::Network for Capped {
    async fn is_online(&self) -> Result<bool, PlatformError> {
        let asked = self.0.asked();
        assert!(asked < 1000, "checked {asked} times");
        app::ports::Network::is_online(self.0.as_ref()).await
    }
}

#[when("the machine wakes and a catch-up run waits for the network")]
fn waits(agent: &mut Agent) {
    let world = &mut agent.conditions;
    let clock = world.clock.clone();
    let network = world.network.clone();
    let capped = Capped(Arc::clone(&network));
    let woke = clock.monotonic();
    let mut wait = wait_for_network(clock.as_ref(), &capped).boxed();
    // Counted here, not read off the clock, so a wait on a clock that
    // doesn't move fails the bound too.
    let mut moved = SignedDuration::ZERO;
    loop {
        if world.network_up_after.is_some_and(|after| moved >= after) {
            network.set(Ok(true));
        }
        if let Poll::Ready(ended) = wait.poll_unpin(&mut noop_context()) {
            world.waited = Some((ended, clock.monotonic() - woke));
            return;
        }
        assert!(
            moved < GIVE_UP_AFTER,
            "the wait for the network was still waiting after {moved}"
        );
        clock.advance(ONE_SECOND);
        moved += ONE_SECOND;
    }
}

fn noop_context() -> std::task::Context<'static> {
    std::task::Context::from_waker(std::task::Waker::noop())
}

fn waited(agent: &Agent) -> &(NetworkWait, SignedDuration) {
    agent.conditions.waited.as_ref().expect("the run waited")
}

#[then(expr = "the run goes ahead within {int} seconds of the network coming up")]
fn ahead_once_up(agent: &mut Agent, seconds: i64) {
    let up_after = agent
        .conditions
        .network_up_after
        .expect("the network comes up");
    let (ended, after) = waited(agent);
    assert!(
        matches!(ended, NetworkWait::Online { waited } if waited == after),
        "{ended:?}"
    );
    assert!(*after >= up_after, "went ahead {after} after the wake");
    assert!(
        *after <= up_after + SignedDuration::from_secs(seconds),
        "went ahead {after} after the wake"
    );
}

#[then(expr = "the run goes ahead {int} minutes after the wake, without the network")]
fn ahead_without(agent: &mut Agent, minutes: i64) {
    let (ended, after) = waited(agent);
    assert_eq!(ended, &NetworkWait::GaveUp);
    assert_eq!(*after, SignedDuration::from_mins(minutes));
}

#[then("the run goes ahead at once")]
fn ahead_at_once(agent: &mut Agent) {
    let (ended, after) = waited(agent);
    assert_eq!(
        ended,
        &NetworkWait::Online {
            waited: SignedDuration::ZERO
        }
    );
    assert_eq!(*after, SignedDuration::ZERO);
}

#[then("the run goes ahead at once, though the machine couldn't tell whether it is online")]
fn ahead_unknown(agent: &mut Agent) {
    let (ended, after) = waited(agent);
    assert_eq!(ended, &NetworkWait::Unknown(unreadable()));
    assert_eq!(*after, SignedDuration::ZERO);
    assert_eq!(agent.conditions.network.asked(), 1, "asked once, at once");
}
