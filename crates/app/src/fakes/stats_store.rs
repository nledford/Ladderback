use std::sync::{Mutex, MutexGuard, PoisonError};

use async_trait::async_trait;
use domain::maintenance::StatsReading;
use domain::{RepositoryAlias, Timestamp};

use crate::ports::{StatsStore, StoreError};

/// The readings of repositories' stats, kept in memory in the order they were
/// kept, for use-case tests and acceptance scenarios. It keeps times to the
/// millisecond, as the real store does.
#[derive(Default)]
pub struct InMemoryStatsStore {
    kept: Mutex<Vec<StatsReading>>,
}

impl InMemoryStatsStore {
    pub fn new() -> Self {
        Self::default()
    }

    fn kept(&self) -> MutexGuard<'_, Vec<StatsReading>> {
        self.kept.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The readings of `alias`, oldest first, in the order kept among
    /// readings taken at the same instant (a stable sort keeps that order).
    fn of(&self, alias: &RepositoryAlias) -> Vec<StatsReading> {
        let mut of: Vec<StatsReading> = self
            .kept()
            .iter()
            .filter(|reading| &reading.alias == alias)
            .cloned()
            .collect();
        of.sort_by_key(|reading| reading.taken_at);
        of
    }
}

#[async_trait]
impl StatsStore for InMemoryStatsStore {
    async fn record(&self, reading: &StatsReading) -> Result<(), StoreError> {
        let stats = reading.stats;
        // The real store keeps each figure in a signed 64-bit column.
        let largest = i64::MAX.unsigned_abs();
        if [
            stats.snapshots,
            stats.total_size,
            stats.stored_size,
            stats.raw_size,
        ]
        .into_iter()
        .any(|figure| figure > largest)
        {
            return Err(StoreError::Unavailable {
                detail: "a figure is more than the store keeps".to_owned(),
            });
        }
        let mut reading = reading.clone();
        reading.taken_at = Timestamp::from_millisecond(reading.taken_at.as_millisecond())
            .unwrap_or(reading.taken_at);
        let mut kept = self.kept();
        kept.retain(|other| other.alias != reading.alias || reading.keeps(other.taken_at));
        kept.push(reading);
        Ok(())
    }

    async fn latest(&self, alias: &RepositoryAlias) -> Result<Option<StatsReading>, StoreError> {
        Ok(self.of(alias).pop())
    }

    async fn history(&self, alias: &RepositoryAlias) -> Result<Vec<StatsReading>, StoreError> {
        Ok(self.of(alias))
    }
}
