//! The readings of repositories' stats (technical design §9 `repo_stats`,
//! §16 decision 104): the `StatsStore` port on the store's own file.

use app::ports::{StatsStore, StoreError};
use async_trait::async_trait;
use domain::RepositoryAlias;
use domain::maintenance::StatsReading;

use crate::SqliteStore;

#[async_trait]
impl StatsStore for SqliteStore {
    async fn record(&self, reading: &StatsReading) -> Result<(), StoreError> {
        let _ = reading;
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
