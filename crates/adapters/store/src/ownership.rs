//! The hub's registry of maintenance owners (technical design §7.3, I1;
//! migration `0022_repository_ownership`): the `OwnershipRegistry` port on
//! the store's own file, built only with the hub (decision 122). A whole
//! report is decided in one `BEGIN IMMEDIATE` transaction, so of claims
//! that race, the first to write is granted and the others see its lease.
//! Times are kept in milliseconds, and `now` is cut to a millisecond before
//! anything is decided, so a lease reads back as it was given.

use app::ports::{ClaimDecision, OwnershipRegistry, StoreError};
use async_trait::async_trait;
use domain::maintenance::{
    Lease, LeaseEpoch, ReleaseRefused, RepositoryOwnership, RepositoryReports, StartWindow,
    report_counts,
};
use domain::{AgentId, RepositoryId, Timestamp, Uuid};
use sqlx::sqlite::SqliteRow;
use sqlx::{Connection, Row, SqliteConnection};

use crate::row::agent_id_text;
use crate::{SqliteStore, unavailable};

const FORGET_REPORT: &str = "DELETE FROM ownership_reports WHERE agent_id = ?1";

const TAKE_REPORT: &str =
    "INSERT INTO ownership_reports (agent_id, repo_id, reported_at) VALUES (?1, ?2, ?3)";

const OTHERS_REPORTS: &str =
    "SELECT reported_at FROM ownership_reports WHERE repo_id = ?1 AND agent_id <> ?2";

const USES: &str = "SELECT agent_id, repo_id FROM ownership_reports";

const OWNER: &str = "SELECT last_epoch, holder, epoch, expires_at \
     FROM repository_owners WHERE repo_id = ?1";

const KEEP_OWNER: &str = "INSERT INTO repository_owners \
     (repo_id, last_epoch, holder, epoch, expires_at) VALUES (?1, ?2, ?3, ?4, ?5) \
     ON CONFLICT (repo_id) DO UPDATE SET last_epoch = excluded.last_epoch, \
     holder = excluded.holder, epoch = excluded.epoch, expires_at = excluded.expires_at";

#[async_trait]
impl OwnershipRegistry for SqliteStore {
    async fn report(
        &self,
        agent: AgentId,
        reports: &RepositoryReports,
        now: Timestamp,
        window: &StartWindow,
    ) -> Result<Vec<ClaimDecision>, StoreError> {
        let now = Timestamp::from_millisecond(now.as_millisecond()).unwrap_or(now);
        let agent_text = agent_id_text(agent);
        let _call = self.call().await;
        let mut conn = self.pool.acquire().await.map_err(unavailable)?;
        let mut tx = conn
            .begin_with("BEGIN IMMEDIATE")
            .await
            .map_err(unavailable)?;
        sqlx::query(FORGET_REPORT)
            .bind(&agent_text)
            .execute(&mut *tx)
            .await
            .map_err(unavailable)?;
        let mut decided = Vec::with_capacity(reports.entries().len());
        for entry in reports.entries() {
            let repo = entry.repository.as_str();
            sqlx::query(TAKE_REPORT)
                .bind(&agent_text)
                .bind(repo)
                .bind(now.as_millisecond())
                .execute(&mut *tx)
                .await
                .map_err(unavailable)?;
            let others = sqlx::query(OTHERS_REPORTS)
                .bind(repo)
                .bind(&agent_text)
                .fetch_all(&mut *tx)
                .await
                .map_err(unavailable)?;
            let mut others_report = false;
            for row in &others {
                others_report |= report_counts(time_of(row, "reported_at")?, now);
            }
            let before = owner(&mut tx, &entry.repository).await?;
            let mut after = before.clone();
            let outcome = after.claim_after_start(
                window,
                agent,
                entry.request,
                entry.shared(others_report),
                now,
            );
            if after != before {
                keep(&mut tx, &after).await?;
            }
            decided.push(ClaimDecision {
                repository: entry.repository.clone(),
                outcome,
            });
        }
        tx.commit().await.map_err(unavailable)?;
        Ok(decided)
    }

    async fn reported_uses(&self) -> Result<Vec<(AgentId, RepositoryId)>, StoreError> {
        let _call = self.call().await;
        let mut conn = self.pool.acquire().await.map_err(unavailable)?;
        let rows = sqlx::query(USES)
            .fetch_all(&mut *conn)
            .await
            .map_err(unavailable)?;
        rows.iter()
            .map(|row| {
                let agent: String = row.try_get("agent_id").map_err(unavailable)?;
                let repository: String = row.try_get("repo_id").map_err(unavailable)?;
                Ok((
                    Uuid::try_parse(&agent)
                        .map(AgentId::from_uuid)
                        .map_err(|_| corrupt("agent_id"))?,
                    RepositoryId::parse(&repository).map_err(|_| corrupt("repo_id"))?,
                ))
            })
            .collect()
    }

    async fn release(
        &self,
        agent: AgentId,
        repository: &RepositoryId,
        epoch: LeaseEpoch,
    ) -> Result<Result<(), ReleaseRefused>, StoreError> {
        let _call = self.call().await;
        let mut conn = self.pool.acquire().await.map_err(unavailable)?;
        let mut tx = conn
            .begin_with("BEGIN IMMEDIATE")
            .await
            .map_err(unavailable)?;
        let mut ownership = owner(&mut tx, repository).await?;
        let released = ownership.release(agent, epoch);
        if released.is_ok() {
            keep(&mut tx, &ownership).await?;
        }
        tx.commit().await.map_err(unavailable)?;
        Ok(released)
    }

    async fn ownership(
        &self,
        repository: &RepositoryId,
    ) -> Result<RepositoryOwnership, StoreError> {
        let _call = self.call().await;
        let mut conn = self.pool.acquire().await.map_err(unavailable)?;
        owner(&mut conn, repository).await
    }
}

/// The ownership kept of `repository`, or a new one when none is.
async fn owner(
    conn: &mut SqliteConnection,
    repository: &RepositoryId,
) -> Result<RepositoryOwnership, StoreError> {
    let Some(row) = sqlx::query(OWNER)
        .bind(repository.as_str())
        .fetch_optional(conn)
        .await
        .map_err(unavailable)?
    else {
        return Ok(RepositoryOwnership::new(repository.clone()));
    };
    let holder: Option<String> = row.try_get("holder").map_err(unavailable)?;
    let lease = match holder {
        None => None,
        Some(holder) => Some(Lease {
            holder: Uuid::try_parse(&holder)
                .map(AgentId::from_uuid)
                .map_err(|_| corrupt("holder"))?,
            epoch: epoch_of(&row, "epoch")?,
            expires_at: time_of(&row, "expires_at")?,
        }),
    };
    Ok(RepositoryOwnership {
        repository: repository.clone(),
        last_epoch: Some(epoch_of(&row, "last_epoch")?),
        lease,
    })
}

/// Writes `ownership` over the one kept of its repository. One with no
/// epoch given was never granted anything, so it has nothing to keep.
async fn keep(
    conn: &mut SqliteConnection,
    ownership: &RepositoryOwnership,
) -> Result<(), StoreError> {
    let Some(last_epoch) = ownership.last_epoch else {
        return Ok(());
    };
    let lease = ownership.lease;
    sqlx::query(KEEP_OWNER)
        .bind(ownership.repository.as_str())
        .bind(epoch_column(last_epoch)?)
        .bind(lease.map(|lease| agent_id_text(lease.holder)))
        .bind(lease.map(|lease| epoch_column(lease.epoch)).transpose()?)
        .bind(lease.map(|lease| lease.expires_at.as_millisecond()))
        .execute(conn)
        .await
        .map_err(unavailable)?;
    Ok(())
}

/// An epoch as the store keeps it. One past what a column holds is never
/// given: only a tampered database could come that near the last.
fn epoch_column(epoch: LeaseEpoch) -> Result<i64, StoreError> {
    i64::try_from(epoch.get()).map_err(|_| StoreError::Corrupt {
        detail: "repository owners: an epoch past what the store keeps".to_owned(),
    })
}

fn epoch_of(row: &SqliteRow, column: &str) -> Result<LeaseEpoch, StoreError> {
    let epoch: i64 = row.try_get(column).map_err(unavailable)?;
    u64::try_from(epoch)
        .ok()
        .and_then(LeaseEpoch::new)
        .ok_or_else(|| corrupt(column))
}

fn time_of(row: &SqliteRow, column: &str) -> Result<Timestamp, StoreError> {
    let millis: i64 = row.try_get(column).map_err(unavailable)?;
    Timestamp::from_millisecond(millis).map_err(|_| corrupt(column))
}

fn corrupt(column: &str) -> StoreError {
    StoreError::Corrupt {
        detail: format!("repository owners: column {column} doesn't read"),
    }
}
