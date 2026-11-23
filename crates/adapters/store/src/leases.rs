//! A paired machine's leases from its hub and its epoch floors (technical
//! design §7.3, I1; migration `0024_agent_leases`): the `LeaseCache` port on
//! the store's own file, built only with the hub (decision 122). A save
//! replaces both tables in one transaction, so a load never sees half of
//! one. Numbers past `i64::MAX` are refused where they enter, on the wire,
//! so each fits its column.

use app::ports::{LeaseCache, StoreError};
use async_trait::async_trait;
use domain::maintenance::{EpochFloor, HeldLease, HeldLeases, LeaseEpoch};
use domain::{RepositoryId, Timestamp};
use sqlx::sqlite::SqliteRow;
use sqlx::{Connection, Row};

use crate::{SqliteStore, unavailable};

const LEASES: &str = "SELECT repo_id, epoch, ends_at FROM agent_leases";

const FLOORS: &str = "SELECT repo_id, recovery_number, generation, epoch FROM agent_epoch_floors";

const KEEP_LEASE: &str = "INSERT INTO agent_leases (repo_id, epoch, ends_at) VALUES (?1, ?2, ?3)";

const KEEP_FLOOR: &str = "INSERT INTO agent_epoch_floors \
     (repo_id, recovery_number, generation, epoch) VALUES (?1, ?2, ?3, ?4)";

#[async_trait]
impl LeaseCache for SqliteStore {
    async fn load(&self) -> Result<HeldLeases, StoreError> {
        let _call = self.call().await;
        let mut conn = self.pool.acquire().await.map_err(unavailable)?;
        let mut tx = conn.begin().await.map_err(unavailable)?;
        let leases = sqlx::query(LEASES)
            .fetch_all(&mut *tx)
            .await
            .map_err(unavailable)?
            .iter()
            .map(|row| {
                Ok((
                    repository_of(row)?,
                    HeldLease {
                        epoch: epoch_of(row)?,
                        ends_at: Timestamp::from_millisecond(
                            row.try_get("ends_at").map_err(unavailable)?,
                        )
                        .map_err(|_| corrupt("ends_at"))?,
                    },
                ))
            })
            .collect::<Result<Vec<_>, StoreError>>()?;
        let floors = sqlx::query(FLOORS)
            .fetch_all(&mut *tx)
            .await
            .map_err(unavailable)?
            .iter()
            .map(|row| {
                Ok((
                    repository_of(row)?,
                    EpochFloor {
                        recovery_number: number_of(row, "recovery_number")?,
                        generation: number_of(row, "generation")?,
                        epoch: epoch_of(row)?,
                    },
                ))
            })
            .collect::<Result<Vec<_>, StoreError>>()?;
        tx.commit().await.map_err(unavailable)?;
        Ok(HeldLeases::from_parts(leases, floors))
    }

    async fn save(&self, leases: &HeldLeases) -> Result<(), StoreError> {
        let _call = self.call().await;
        let mut conn = self.pool.acquire().await.map_err(unavailable)?;
        let mut tx = conn
            .begin_with("BEGIN IMMEDIATE")
            .await
            .map_err(unavailable)?;
        sqlx::query("DELETE FROM agent_leases")
            .execute(&mut *tx)
            .await
            .map_err(unavailable)?;
        sqlx::query("DELETE FROM agent_epoch_floors")
            .execute(&mut *tx)
            .await
            .map_err(unavailable)?;
        for (repository, lease) in leases.leases() {
            sqlx::query(KEEP_LEASE)
                .bind(repository.as_str())
                .bind(column(lease.epoch.get())?)
                .bind(lease.ends_at.as_millisecond())
                .execute(&mut *tx)
                .await
                .map_err(unavailable)?;
        }
        for (repository, floor) in leases.floors() {
            sqlx::query(KEEP_FLOOR)
                .bind(repository.as_str())
                .bind(column(floor.recovery_number)?)
                .bind(column(floor.generation)?)
                .bind(column(floor.epoch.get())?)
                .execute(&mut *tx)
                .await
                .map_err(unavailable)?;
        }
        tx.commit().await.map_err(unavailable)
    }
}

/// A number as a column holds it. One past `i64::MAX` never reaches here:
/// the wire refuses it.
fn column(number: u64) -> Result<i64, StoreError> {
    i64::try_from(number).map_err(|_| StoreError::Corrupt {
        detail: "agent leases: a number past what the store keeps".to_owned(),
    })
}

fn repository_of(row: &SqliteRow) -> Result<RepositoryId, StoreError> {
    let text: String = row.try_get("repo_id").map_err(unavailable)?;
    RepositoryId::parse(&text).map_err(|_| corrupt("repo_id"))
}

fn number_of(row: &SqliteRow, name: &str) -> Result<u64, StoreError> {
    let number: i64 = row.try_get(name).map_err(unavailable)?;
    u64::try_from(number).map_err(|_| corrupt(name))
}

fn epoch_of(row: &SqliteRow) -> Result<LeaseEpoch, StoreError> {
    LeaseEpoch::new(number_of(row, "epoch")?).ok_or_else(|| corrupt("epoch"))
}

fn corrupt(name: &str) -> StoreError {
    StoreError::Corrupt {
        detail: format!("agent leases: column {name} doesn't read"),
    }
}
