//! The `StatsStore` port (technical design §9 `repo_stats`, §16 decision 104):
//! the readings of each repository's stats, kept a year so they can be
//! charted.

use async_trait::async_trait;
use domain::RepositoryAlias;
use domain::maintenance::StatsReading;

use super::StoreError;

/// Keeps the readings of repositories' stats.
#[async_trait]
pub trait StatsStore: Send + Sync {
    /// Keeps `reading`, and drops the readings of its repository that it no
    /// longer keeps ([`StatsReading::keeps`]): those taken more than a year
    /// before it. Both happen or neither does.
    async fn record(&self, reading: &StatsReading) -> Result<(), StoreError>;

    /// The reading of `alias` taken last, the one kept last among readings
    /// taken at the same instant; `None` when none is kept.
    async fn latest(&self, alias: &RepositoryAlias) -> Result<Option<StatsReading>, StoreError>;

    /// Every reading of `alias` kept, oldest first, in the order they were
    /// kept among readings taken at the same instant.
    async fn history(&self, alias: &RepositoryAlias) -> Result<Vec<StatsReading>, StoreError>;
}
