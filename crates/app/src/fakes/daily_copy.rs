//! A [`DailyCopy`] that keeps only when each copy was taken, by the clock
//! it is given, and fails as a test tells it.

use std::sync::{Arc, Mutex, PoisonError};

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
        let _ = (clock, State::default());
        todo!()
    }

    /// A copy is there, taken at `at`, as by an earlier start of the daemon.
    pub fn taken_before(&self, at: Timestamp) {
        let _ = at;
        todo!()
    }

    /// Every copy from now on fails with `error`, and leaves the one there
    /// is as it was.
    pub fn fail(&self, error: DailyCopyError) {
        let _ = error;
        todo!()
    }

    /// Copies are taken again.
    pub fn recover(&self) {
        todo!()
    }

    /// When each copy it took was taken, in order; a copy
    /// [`taken_before`](Self::taken_before) isn't one.
    pub fn taken(&self) -> Vec<Timestamp> {
        todo!()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

#[async_trait]
impl DailyCopy for InMemoryDailyCopy {
    async fn taken_at(&self) -> Result<Option<Timestamp>, DailyCopyError> {
        let _ = (&self.clock, self.lock());
        todo!()
    }

    async fn take(&self) -> Result<(), DailyCopyError> {
        todo!()
    }
}
