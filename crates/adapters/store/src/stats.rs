//! The readings of repositories' stats (technical design §9 `repo_stats`,
//! §16 decision 104): the `StatsStore` port on the store's own file.

use app::ports::{StatsStore, StoreError};
use async_trait::async_trait;
use domain::maintenance::{READINGS_KEPT_FOR, StatsReading};
use domain::{RepositoryAlias, RepositoryId};
use sqlx::Connection;

use crate::report::cap;
use crate::{SqliteStore, row, sql, unavailable};

#[async_trait]
impl StatsStore for SqliteStore {
    /// One transaction, so a crash keeps the reading and drops the old ones,
    /// or does neither.
    async fn record(&self, reading: &StatsReading) -> Result<(), StoreError> {
        let stats = &reading.stats;
        let snapshots = column("snapshots", stats.snapshots)?;
        let total = column("total_bytes", stats.total_size)?;
        let stored = column("stored_bytes", stats.stored_size)?;
        let raw = column("raw_bytes", stats.raw_size)?;
        let taken_at = reading.taken_at.as_millisecond();
        // The readings taken more than a year before this one, to the
        // millisecond the store keeps: a year is whole milliseconds.
        let year = i64::try_from(READINGS_KEPT_FOR.as_millis()).unwrap_or(i64::MAX);
        let oldest = taken_at.saturating_sub(year);
        let _call = self.call().await;
        let mut conn = self.pool.acquire().await.map_err(unavailable)?;
        let mut tx = conn
            .begin_with("BEGIN IMMEDIATE")
            .await
            .map_err(unavailable)?;
        sqlx::query(sql::DROP_OLD_STATS)
            .bind(reading.alias.as_str())
            .bind(oldest)
            .execute(&mut *tx)
            .await
            .map_err(unavailable)?;
        sqlx::query(sql::INSERT_STATS)
            .bind(reading.alias.as_str())
            .bind(reading.repository.as_ref().map(RepositoryId::as_str))
            .bind(taken_at)
            .bind(snapshots)
            .bind(total)
            .bind(stored)
            .bind(raw)
            .execute(&mut *tx)
            .await
            .map_err(unavailable)?;
        tx.commit().await.map_err(unavailable)
    }

    async fn latest(&self, alias: &RepositoryAlias) -> Result<Option<StatsReading>, StoreError> {
        let _call = self.call().await;
        let row = sqlx::query(sql::LATEST_STATS)
            .bind(alias.as_str())
            .fetch_optional(&self.pool)
            .await
            .map_err(unavailable)?;
        row.as_ref().map(row::stats_reading).transpose()
    }

    async fn history(&self, alias: &RepositoryAlias) -> Result<Vec<StatsReading>, StoreError> {
        let _call = self.call().await;
        let rows = sqlx::query(sql::STATS_HISTORY)
            .bind(alias.as_str())
            .fetch_all(&self.pool)
            .await
            .map_err(unavailable)?;
        rows.iter().map(row::stats_reading).collect()
    }
}

/// A figure as its column keeps it: one past what a signed 64-bit integer
/// holds (8 EiB) is refused, never wrapped.
fn column(name: &str, figure: u64) -> Result<i64, StoreError> {
    i64::try_from(figure).map_err(|_| StoreError::Unavailable {
        detail: cap(format!(
            "a stats reading: {name} {figure} is more than the store keeps"
        )),
    })
}
