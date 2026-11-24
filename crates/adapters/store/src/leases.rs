//! A paired machine's leases from its hub and its epoch floors, each with
//! the hub's proof of it when the machine keeps one (technical design
//! §7.3, I1; migrations `0024_agent_leases`, `0027_agent_maintenance_paused`
//! and `0028_hub_recovery`): the `LeaseCache` port on the store's own file,
//! built only with the hub (decision 122). A save replaces both tables in one
//! transaction, and a save of the pauses replaces the pauses alone, so a load
//! never sees half of one. Numbers past `i64::MAX` are refused where they
//! enter, on the wire, so each fits its column.

use app::ports::{LeaseCache, StoreError};
use async_trait::async_trait;
use domain::fleet::{SIGNATURE_BYTES, Signature};
use domain::maintenance::{EpochFloor, FloorProof, HeldLease, HeldLeases, LeaseEpoch, Pause};
use domain::{RepositoryId, Timestamp};
use sqlx::sqlite::SqliteRow;
use sqlx::{Connection, Row};

use crate::{SqliteStore, unavailable};

const LEASES: &str = "SELECT repo_id, epoch, ends_at FROM agent_leases";

const FLOORS: &str = "SELECT repo_id, recovery_number, generation, epoch, expires_at, signature \
     FROM agent_epoch_floors";

const PAUSES: &str = "SELECT repo_id, since, alerted FROM agent_maintenance_paused";

const KEEP_PAUSE: &str =
    "INSERT INTO agent_maintenance_paused (repo_id, since, alerted) VALUES (?1, ?2, ?3)";

const KEEP_LEASE: &str = "INSERT INTO agent_leases (repo_id, epoch, ends_at) VALUES (?1, ?2, ?3)";

const KEEP_FLOOR: &str = "INSERT INTO agent_epoch_floors \
     (repo_id, recovery_number, generation, epoch, expires_at, signature) \
     VALUES (?1, ?2, ?3, ?4, ?5, ?6)";

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
        let mut proofs = Vec::new();
        let floors = sqlx::query(FLOORS)
            .fetch_all(&mut *tx)
            .await
            .map_err(unavailable)?
            .iter()
            .map(|row| {
                let repository = repository_of(row)?;
                if let Some(proof) = proof_of(row)? {
                    proofs.push((repository.clone(), proof));
                }
                Ok((
                    repository,
                    EpochFloor {
                        recovery_number: number_of(row, "recovery_number")?,
                        generation: number_of(row, "generation")?,
                        epoch: epoch_of(row)?,
                    },
                ))
            })
            .collect::<Result<Vec<_>, StoreError>>()?;
        let pauses = sqlx::query(PAUSES)
            .fetch_all(&mut *tx)
            .await
            .map_err(unavailable)?
            .iter()
            .map(|row| {
                Ok((
                    repository_of(row)?,
                    Pause {
                        since: Timestamp::from_millisecond(
                            row.try_get("since").map_err(unavailable)?,
                        )
                        .map_err(|_| corrupt("since"))?,
                        alerted: row.try_get("alerted").map_err(unavailable)?,
                    },
                ))
            })
            .collect::<Result<Vec<_>, StoreError>>()?;
        tx.commit().await.map_err(unavailable)?;
        Ok(HeldLeases::from_parts(leases, floors, proofs).with_pauses(pauses))
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
            let proof = leases.proof(repository);
            sqlx::query(KEEP_FLOOR)
                .bind(repository.as_str())
                .bind(column(floor.recovery_number)?)
                .bind(column(floor.generation)?)
                .bind(column(floor.epoch.get())?)
                .bind(proof.map(|proof| proof.expires_at.as_millisecond()))
                .bind(proof.map(|proof| proof.signature.as_bytes().to_vec()))
                .execute(&mut *tx)
                .await
                .map_err(unavailable)?;
        }
        tx.commit().await.map_err(unavailable)
    }

    async fn save_pauses(&self, leases: &HeldLeases) -> Result<(), StoreError> {
        let _call = self.call().await;
        let mut conn = self.pool.acquire().await.map_err(unavailable)?;
        let mut tx = conn
            .begin_with("BEGIN IMMEDIATE")
            .await
            .map_err(unavailable)?;
        sqlx::query("DELETE FROM agent_maintenance_paused")
            .execute(&mut *tx)
            .await
            .map_err(unavailable)?;
        for (repository, pause) in leases.pauses() {
            sqlx::query(KEEP_PAUSE)
                .bind(repository.as_str())
                .bind(pause.since.as_millisecond())
                .bind(pause.alerted)
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

/// The proof a floor's row keeps: its expiry and its signature together,
/// or neither.
fn proof_of(row: &SqliteRow) -> Result<Option<FloorProof>, StoreError> {
    let expires_at: Option<i64> = row.try_get("expires_at").map_err(unavailable)?;
    let signature: Option<Vec<u8>> = row.try_get("signature").map_err(unavailable)?;
    match (expires_at, signature) {
        (None, None) => Ok(None),
        (Some(expires_at), Some(signature)) => {
            let bytes = <[u8; SIGNATURE_BYTES]>::try_from(signature.as_slice())
                .map_err(|_| corrupt("signature"))?;
            Ok(Some(FloorProof {
                expires_at: Timestamp::from_millisecond(expires_at)
                    .map_err(|_| corrupt("expires_at"))?,
                signature: Signature::from_bytes(bytes).map_err(|_| corrupt("signature"))?,
            }))
        }
        _ => Err(corrupt("signature")),
    }
}

fn corrupt(name: &str) -> StoreError {
    StoreError::Corrupt {
        detail: format!("agent leases: column {name} doesn't read"),
    }
}
