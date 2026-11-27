//! A [`DailyCopy`] that keeps only when each copy was taken, by the clock
//! it is given, and fails as a test tells it.

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use async_trait::async_trait;
use domain::Timestamp;

use crate::ports::{Clock, DailyCopy, DailyCopyError};

/// The daily copy of a database that exists only in memory: a copy is the
/// clock's time when it was taken.
pub struct InMemoryDailyCopy {
    clock: Arc<dyn Clock>,
    state: Mutex<State>,
}

#[derive(Default)]
struct State {
    /// When the copy there is was taken.
    there: Option<Timestamp>,
    /// When each copy this one took was taken, in order.
    taken: Vec<Timestamp>,
    /// What each copy fails with, while it is set.
    failing: Option<DailyCopyError>,
}

impl InMemoryDailyCopy {
    pub fn new(clock: Arc<dyn Clock>) -> Self {
        Self {
            clock,
            state: Mutex::default(),
        }
    }

    /// A copy is there, taken at `at`, as by an earlier start of the daemon.
    pub fn taken_before(&self, at: Timestamp) {
        self.lock().there = Some(at);
    }

    /// Every copy from now on fails with `error`, and leaves the one there
    /// is as it was.
    pub fn fail(&self, error: DailyCopyError) {
        self.lock().failing = Some(error);
    }

    /// Copies are taken again.
    pub fn recover(&self) {
        self.lock().failing = None;
    }

    /// When each copy it took was taken, in order; a copy
    /// [`taken_before`](Self::taken_before) isn't one.
    pub fn taken(&self) -> Vec<Timestamp> {
        self.lock().taken.clone()
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

#[async_trait]
impl DailyCopy for InMemoryDailyCopy {
    async fn taken_at(&self) -> Result<Option<Timestamp>, DailyCopyError> {
        Ok(self.lock().there)
    }

    async fn take(&self) -> Result<(), DailyCopyError> {
        let now = self.clock.now();
        let mut state = self.lock();
        if let Some(error) = &state.failing {
            return Err(error.clone());
        }
        state.there = Some(now);
        state.taken.push(now);
        Ok(())
    }
}
