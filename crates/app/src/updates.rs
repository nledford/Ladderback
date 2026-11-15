//! The release check (technical design §5; decision NN): the loop in
//! `serve` that asks for Ladderback's newest release at most once a day and
//! keeps the [`UpdateNotice`] the header shows. When it asks is the
//! domain's `next_check`, and whether a release is an update is
//! `available_update`; what this loop adds is the order: it shows the update
//! kept from the last check at once, reads the configuration before each
//! check, so a check turned off neither asks nor shows anything, and keeps
//! each check through the `ScheduleStore` port, so a restart neither asks
//! again within the day nor hides the update it found.

use std::sync::{Arc, Mutex, PoisonError};

use domain::updates::Version;

use crate::ports::{
    Clock, ConfigError, ConfigStore, Random, ReleaseFeed, ReleaseFeedError, ScheduleStore,
    StoreError,
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
        todo!("{:?}", self.running)
    }
}
