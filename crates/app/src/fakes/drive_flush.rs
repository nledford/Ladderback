//! A [`DriveFlush`] that counts its flushes and fails as a test tells it.

use std::sync::{Mutex, MutexGuard, PoisonError};

use async_trait::async_trait;

use crate::ports::{DriveFlush, StoreError};

/// The drive of a database that exists only in memory.
#[derive(Debug, Default)]
pub struct InMemoryDriveFlush {
    state: Mutex<State>,
}

#[derive(Debug, Default)]
struct State {
    /// How many flushes succeeded.
    flushed: usize,
    /// What each flush fails with, while it is set.
    failing: Option<StoreError>,
}

impl InMemoryDriveFlush {
    /// Every flush from now on fails with `error`.
    pub fn fail(&self, error: StoreError) {
        self.lock().failing = Some(error);
    }

    /// Flushes succeed again.
    pub fn recover(&self) {
        self.lock().failing = None;
    }

    /// How many flushes succeeded.
    pub fn flushed(&self) -> usize {
        self.lock().flushed
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

#[async_trait]
impl DriveFlush for InMemoryDriveFlush {
    async fn flush(&self) -> Result<(), StoreError> {
        let mut state = self.lock();
        if let Some(error) = &state.failing {
            return Err(error.clone());
        }
        state.flushed = state.flushed.saturating_add(1);
        Ok(())
    }
}
