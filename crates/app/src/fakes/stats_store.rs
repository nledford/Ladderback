use std::sync::Mutex;

use async_trait::async_trait;
use domain::RepositoryAlias;
use domain::maintenance::StatsReading;

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
}

#[async_trait]
impl StatsStore for InMemoryStatsStore {
    async fn record(&self, reading: &StatsReading) -> Result<(), StoreError> {
        let _ = (reading, &self.kept);
        todo!()
    }

    async fn latest(&self, alias: &RepositoryAlias) -> Result<Option<StatsReading>, StoreError> {
        let _ = alias;
        todo!()
    }

    async fn history(&self, alias: &RepositoryAlias) -> Result<Vec<StatsReading>, StoreError> {
        let _ = alias;
        todo!()
    }
}
