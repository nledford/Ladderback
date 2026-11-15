//! Steps for `features/updates/release_check.feature`: the release check
//! as the agent runs it while it serves, against a scripted release feed,
//! the in-memory schedule store and a clock the steps move from one of its
//! sleeps to the next.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, PoisonError};

use app::fakes::{
    FakeClock, InMemoryConfigStore, InMemoryScheduleStore, ScriptedReleaseFeed, SeededRandom,
};
use app::ports::{Clock, ConfigStore, ReleaseCheck, ReleaseFeedError, ScheduleStore};
use app::updates::{CheckForUpdates, ReleaseCheckReport, UpdateNotice};
use cucumber::{given, then, when};
use domain::planning::{AgentConfig, AgentSettings, Hooks, UpdateCheck};
use domain::updates::Version;
use domain::{SignedDuration, Timestamp};
use futures::FutureExt;
use futures::future::BoxFuture;

use super::Agent;

/// The most turns of the loop one step lets it take, far more than a few
/// days of daily checks need.
const MOST_TURNS: u32 = 10_000;

pub struct UpdatesWorld {
    running: Version,
    started: Timestamp,
    clock: Arc<FakeClock>,
    feed: Arc<ScriptedReleaseFeed>,
    schedules: Arc<InMemoryScheduleStore>,
    config: Arc<InMemoryConfigStore>,
    notice: Arc<UpdateNotice>,
    reports: Arc<Mutex<Vec<ReleaseCheckReport>>>,
    serving: Option<BoxFuture<'static, ()>>,
}

impl Default for UpdatesWorld {
    fn default() -> Self {
        let started = time("2026-01-01 00:00+00:00");
        let clock = Arc::new(FakeClock::new(started));
        Self {
            running: version("0.1.0"),
            started,
            feed: Arc::new(ScriptedReleaseFeed::new(clock.clone())),
            clock,
            schedules: Arc::default(),
            config: Arc::default(),
            notice: Arc::default(),
            reports: Arc::default(),
            serving: None,
        }
    }
}

impl std::fmt::Debug for UpdatesWorld {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UpdatesWorld")
            .field("running", &self.running)
            .field("started", &self.started)
            .finish_non_exhaustive()
    }
}

impl UpdatesWorld {
    /// Runs the release check, moving the clock from one of its sleeps to
    /// the next, until the clock reads `until`.
    fn serve_until(&mut self, until: Timestamp) {
        let serving = self.serving.get_or_insert_with(|| {
            let reports = self.reports.clone();
            let check = CheckForUpdates {
                clock: self.clock.clone(),
                // Each draw falls near the middle of the jitter.
                random: Arc::new(SeededRandom::new(u64::MAX / 2)),
                feed: self.feed.clone(),
                schedules: self.schedules.clone(),
                config: self.config.clone(),
                running: self.running.clone(),
                notice: self.notice.clone(),
                reports: Arc::new(move |report| {
                    reports
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .push(report);
                }),
            };
            async move { check.run().await }.boxed()
        });
        let mut context = std::task::Context::from_waker(std::task::Waker::noop());
        for _ in 0..MOST_TURNS {
            assert!(
                serving.poll_unpin(&mut context).is_pending(),
                "the release check's loop ended"
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
        panic!("the release check's loop turned {MOST_TURNS} times before {until}");
    }

    fn reports(&self) -> Vec<ReleaseCheckReport> {
        self.reports
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    fn said_available(&self) -> Vec<Version> {
        self.reports()
            .into_iter()
            .filter_map(|report| match report {
                ReleaseCheckReport::UpdateAvailable(version) => Some(version),
                _ => None,
            })
            .collect()
    }
}

/// A time as the scenario writes it: `2026-06-10 03:00+02:00`.
fn time(text: &str) -> Timestamp {
    text.parse()
        .unwrap_or_else(|error| panic!("{text:?} is a time with its offset: {error}"))
}

fn version(text: &str) -> Version {
    Version::parse(text).unwrap_or_else(|_| panic!("{text:?} is a version"))
}

#[given(expr = "the agent runs version {string}")]
fn runs_version(agent: &mut Agent, text: String) {
    agent.updates.running = version(&text);
}

#[given(expr = "the agent's clock reads {string}")]
fn clock_reads(agent: &mut Agent, text: String) {
    let world = &mut agent.updates;
    world.started = time(&text);
    world.clock = Arc::new(FakeClock::new(world.started));
    world.feed = Arc::new(ScriptedReleaseFeed::new(world.clock.clone()));
}

#[given(expr = "the newest release is {string}")]
fn newest_release(agent: &mut Agent, tag: String) {
    agent.updates.feed.release(&tag, false);
}

#[given(expr = "the newest release is {string}, marked a prerelease")]
fn newest_prerelease(agent: &mut Agent, tag: String) {
    agent.updates.feed.release(&tag, true);
}

#[when(expr = "the newest release becomes {string}")]
fn newest_release_becomes(agent: &mut Agent, tag: String) {
    agent.updates.feed.release(&tag, false);
}

#[given(expr = "the newest release can't be read: {}")]
fn newest_release_fails(agent: &mut Agent, failure: String) {
    let error = match failure.as_str() {
        "unreachable" => ReleaseFeedError::Unreachable,
        "timed out" => ReleaseFeedError::TimedOut,
        "rejected" => ReleaseFeedError::Rejected { status: 403 },
        "unreadable" => ReleaseFeedError::Unreadable,
        other => panic!("{other:?} is no way a release check fails"),
    };
    agent.updates.feed.fail(error);
}

#[given(expr = "the agent last checked at {string}, when version {string} was available")]
async fn last_checked(agent: &mut Agent, at: String, text: String) {
    let check = ReleaseCheck {
        at: time(&at),
        update: Some(version(&text)),
    };
    agent
        .updates
        .schedules
        .keep_release_check(&check)
        .await
        .expect("the schedule store keeps the check");
}

#[given("the agent's configuration turns the release check off")]
async fn check_off(agent: &mut Agent) {
    let settings = AgentSettings {
        update_check: UpdateCheck::Off,
        ..AgentSettings::default()
    };
    let config = AgentConfig::new(settings, Hooks::default(), BTreeMap::new(), Vec::new())
        .expect("an empty configuration is valid");
    agent
        .updates
        .config
        .save(&config)
        .await
        .expect("the configuration saves");
}

#[when(expr = "the agent serves for {int} day(s)")]
fn serves_for(agent: &mut Agent, days: i64) {
    let world = &mut agent.updates;
    let until = world.clock.now() + SignedDuration::from_hours(24 * days);
    world.serve_until(until);
}

#[when(expr = "the agent serves until its clock reads {string}")]
fn serves_until(agent: &mut Agent, text: String) {
    agent.updates.serve_until(time(&text));
}

#[then(expr = "the header shows that version {string} is available")]
fn shows_update(agent: &mut Agent, text: String) {
    assert_eq!(agent.updates.notice.available(), Some(version(&text)));
}

#[then("the header shows no available update")]
fn shows_no_update(agent: &mut Agent) {
    assert_eq!(agent.updates.notice.available(), None);
}

#[then(expr = "the log said once that version {string} is available")]
fn said_once(agent: &mut Agent, text: String) {
    let said = agent.updates.said_available();
    let times = said.iter().filter(|said| **said == version(&text)).count();
    assert_eq!(times, 1, "said {said:?}");
}

#[then("the log said nothing of an update")]
fn said_nothing(agent: &mut Agent) {
    let said = agent.updates.said_available();
    assert!(said.is_empty(), "said {said:?}");
}

#[then("the log noted the failed checks only for debugging")]
fn noted_failures(agent: &mut Agent) {
    let reports = agent.updates.reports();
    assert!(!reports.is_empty(), "the failed checks were noted");
    assert!(
        reports
            .iter()
            .all(|report| matches!(report, ReleaseCheckReport::Failed(_))),
        "only the failed checks were noted: {reports:?}"
    );
}

#[then(expr = "the agent asked for the newest release {int} time(s)")]
fn asked_times(agent: &mut Agent, times: usize) {
    let asked = agent.updates.feed.asked();
    assert_eq!(asked.len(), times, "asked at {asked:?}");
}

#[then("the agent never asked for the newest release")]
fn never_asked(agent: &mut Agent) {
    let asked = agent.updates.feed.asked();
    assert!(asked.is_empty(), "asked at {asked:?}");
}

#[then("each time at least a day after the one before")]
fn a_day_apart(agent: &mut Agent) {
    let asked = agent.updates.feed.asked();
    for pair in asked.windows(2) {
        let [earlier, later] = pair else { continue };
        let apart = earlier.duration_until(*later);
        assert!(
            apart >= SignedDuration::from_hours(24),
            "asked at {asked:?}"
        );
    }
}

#[then(expr = "the agent first asked within {int} minutes of starting")]
fn first_asked_within(agent: &mut Agent, minutes: i64) {
    let world = &agent.updates;
    let asked = world.feed.asked();
    let first = asked.first().expect("the agent asked");
    let after = world.started.duration_until(*first);
    assert!(
        after <= SignedDuration::from_mins(minutes),
        "first asked {after} after starting"
    );
}
