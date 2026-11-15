//! A [`ReleaseFeed`] that answers as a test tells it, and records when it
//! was asked.

use std::sync::{Arc, Mutex, PoisonError};

use async_trait::async_trait;
use domain::Timestamp;
use domain::updates::{Release, Version};

use crate::ports::{Clock, ReleaseFeed, ReleaseFeedError};

/// Answers each request with the answer last set, reading a release's tag
/// as the real adapter does; [`ReleaseFeedError::Unreachable`] until one is
/// set. It records the clock's time of each request.
pub struct ScriptedReleaseFeed {
    clock: Arc<dyn Clock>,
    answer: Mutex<Result<Release, ReleaseFeedError>>,
    asked: Mutex<Vec<Timestamp>>,
}

impl ScriptedReleaseFeed {
    pub fn new(clock: Arc<dyn Clock>) -> Self {
        Self {
            clock,
            answer: Mutex::new(Err(ReleaseFeedError::Unreachable)),
            asked: Mutex::new(Vec::new()),
        }
    }

    /// The newest release is the one tagged `tag`, marked a prerelease when
    /// `prerelease` is; a tag that isn't `v` and a version is
    /// [`ReleaseFeedError::Unreadable`].
    pub fn release(&self, tag: &str, prerelease: bool) {
        let answer = Version::of_tag(tag)
            .map(|version| Release {
                version,
                prerelease,
            })
            .map_err(|_| ReleaseFeedError::Unreadable);
        self.answer_with(answer);
    }

    /// Every request from now on fails with `error`.
    pub fn fail(&self, error: ReleaseFeedError) {
        self.answer_with(Err(error));
    }

    fn answer_with(&self, answer: Result<Release, ReleaseFeedError>) {
        *self.answer.lock().unwrap_or_else(PoisonError::into_inner) = answer;
    }

    /// When it was asked, in order.
    pub fn asked(&self) -> Vec<Timestamp> {
        self.asked
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

#[async_trait]
impl ReleaseFeed for ScriptedReleaseFeed {
    async fn newest(&self) -> Result<Release, ReleaseFeedError> {
        self.asked
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(self.clock.now());
        self.answer
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}
