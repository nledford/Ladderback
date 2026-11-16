//! The release check (technical design §5; decision 107): the loop in
//! `serve` that asks for Ladderback's newest release at most once a day and
//! keeps the [`UpdateNotice`] the header shows. When it asks is the
//! domain's `next_check`, and whether a release is an update is
//! `available_update`; what this loop adds is the order: it shows the update
//! kept from the last check at once, reads the configuration before each
//! check, so a check turned off neither asks nor shows anything, and keeps
//! each check through the `ScheduleStore` port, so a restart neither asks
//! again within the day nor hides the update it found.

use std::sync::{Arc, Mutex, PoisonError};

use domain::Timestamp;
use domain::planning::UpdateCheck;
use domain::updates::{Release, Version, available_update, next_check};

use crate::ports::{
    Clock, ConfigError, ConfigStore, Random, ReleaseCheck, ReleaseFeed, ReleaseFeedError,
    ScheduleStore, StoreError,
};

/// The available update the header shows, shared between the release
/// check, which sets it, and the routes, which read it.
#[derive(Debug, Default)]
pub struct UpdateNotice {
    shown: Mutex<Option<Version>>,
}

impl UpdateNotice {
    pub fn new() -> Self {
        Self::default()
    }

    /// The available update, or `None` when there is none to show.
    pub fn available(&self) -> Option<Version> {
        self.shown
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Shows `update` from now on, or nothing when it is `None`.
    pub fn show(&self, update: Option<Version>) {
        *self.shown.lock().unwrap_or_else(PoisonError::into_inner) = update;
    }
}

/// What the release check did that the daemon's log tells.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReleaseCheckReport {
    /// A check found `0` the available update, and the one before hadn't:
    /// said once for each new version.
    UpdateAvailable(Version),
    /// A check couldn't read the newest release: noted only for debugging,
    /// as nothing is wrong with the agent.
    Failed(ReleaseFeedError),
    /// The last check couldn't be read or kept.
    Store(StoreError),
    /// The configuration couldn't be read, so the check didn't ask.
    Config(ConfigError),
}

/// The loop that checks for a newer release (§5). It owns nothing but its
/// ports and the notice; the composition root spawns [`run`](Self::run)
/// once the daemon serves and drops it when the daemon stops.
pub struct CheckForUpdates {
    pub clock: Arc<dyn Clock>,
    pub random: Arc<dyn Random>,
    pub feed: Arc<dyn ReleaseFeed>,
    pub schedules: Arc<dyn ScheduleStore>,
    pub config: Arc<dyn ConfigStore>,
    /// The version the agent runs.
    pub running: Version,
    /// What the header shows.
    pub notice: Arc<UpdateNotice>,
    /// Where it says what the log tells.
    pub reports: Arc<dyn Fn(ReleaseCheckReport) + Send + Sync>,
}

impl CheckForUpdates {
    /// Checks for a newer release until the future is dropped. On start it
    /// shows the update the last check kept, when the check is on and that
    /// update is newer than the version that runs; then, at each
    /// `next_check`, it reads the configuration, and, with the check on,
    /// asks for the newest release, shows what it found (a failure keeps the
    /// update found before) and keeps the check. With the check off it
    /// shows nothing and asks nothing, and reads the configuration again a
    /// day later.
    pub async fn run(&self) {
        let kept = self.kept().await;
        let mut last = kept.as_ref().map(|check| check.at);
        // The kept update shows only while it is still newer than the
        // version that runs: after an upgrade to it, it is none.
        let mut update = kept.and_then(|check| {
            let version = check.update?;
            available_update(
                &self.running,
                &Release {
                    version,
                    prerelease: false,
                },
            )
        });
        if self.is_on().await {
            self.notice.show(update.clone());
        }
        loop {
            let at = next_check(last, self.clock.now(), self.random.draw());
            self.clock.sleep_until(at).await;
            let now = self.clock.now();
            last = Some(now);
            if self.is_on().await {
                update = self.check(now, update).await;
            } else {
                self.notice.show(None);
            }
        }
    }

    /// The release check last kept, or `None`, told, when it can't be read.
    async fn kept(&self) -> Option<ReleaseCheck> {
        self.schedules
            .release_check()
            .await
            .unwrap_or_else(|error| {
                (self.reports)(ReleaseCheckReport::Store(error));
                None
            })
    }

    /// Whether the configuration has the check on; off, told, when it can't
    /// be read.
    async fn is_on(&self) -> bool {
        match self.config.load().await {
            Ok(config) => config.agent().update_check == UpdateCheck::On,
            Err(error) => {
                (self.reports)(ReleaseCheckReport::Config(error));
                false
            }
        }
    }

    /// Asks for the newest release at `now`, shows the update it is (or
    /// `previous`, when the answer can't be read) and keeps the check.
    async fn check(&self, now: Timestamp, previous: Option<Version>) -> Option<Version> {
        let found = match self.feed.newest().await {
            Ok(release) => available_update(&self.running, &release),
            Err(error) => {
                (self.reports)(ReleaseCheckReport::Failed(error));
                previous.clone()
            }
        };
        if let Some(version) = found
            .as_ref()
            .filter(|found| previous.as_ref() != Some(*found))
        {
            (self.reports)(ReleaseCheckReport::UpdateAvailable(version.clone()));
        }
        self.notice.show(found.clone());
        let check = ReleaseCheck {
            at: now,
            update: found.clone(),
        };
        if let Err(error) = self.schedules.keep_release_check(&check).await {
            (self.reports)(ReleaseCheckReport::Store(error));
        }
        found
    }
}
