//! The proposals (technical design §9 `proposals`; decision 89): the
//! `ProposalStore` port on the store's own file.

use std::net::IpAddr;

use app::ports::{Admission, ProposalStore, StoreError, StoredProposal};
use async_trait::async_trait;
use domain::Timestamp;
use domain::planning::{ProposalId, ProposalStatus, Proposer, ProposerRecord, RejectReason};
use sqlx::{Connection, SqliteConnection};

use crate::{SqliteStore, row, sql, unavailable};

/// A number of rows as SQLite counts them, which a `usize` beyond `i64`
/// never reaches: such a limit is no limit.
fn as_count(limit: usize) -> i64 {
    i64::try_from(limit).unwrap_or(i64::MAX)
}

/// A time kept in milliseconds, as `row` reads it back; one SQLite can't
/// have written is no time.
fn time(millisecond: i64) -> Option<Timestamp> {
    Timestamp::from_millisecond(millisecond).ok()
}

/// What the proposer `token_id` has made, as its proposals kept at
/// `made_at` give it.
async fn record(
    tx: &mut SqliteConnection,
    token_id: Option<&str>,
    made_at: Timestamp,
) -> Result<ProposerRecord, StoreError> {
    let pending: i64 = sqlx::query_scalar(sql::COUNT_PENDING_PROPOSALS)
        .bind(token_id)
        .bind(made_at.as_millisecond())
        .fetch_one(&mut *tx)
        .await
        .map_err(unavailable)?;
    let made: Vec<i64> = sqlx::query_scalar(sql::PROPOSALS_MADE)
        .bind(token_id)
        .fetch_all(&mut *tx)
        .await
        .map_err(unavailable)?;
    let last_rejected: Option<i64> = sqlx::query_scalar(sql::LAST_REJECTION)
        .bind(token_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(unavailable)?;
    Ok(ProposerRecord {
        pending: usize::try_from(pending).unwrap_or(usize::MAX),
        made: made.into_iter().filter_map(time).collect(),
        last_rejected: last_rejected.and_then(time),
    })
}

#[async_trait]
impl ProposalStore for SqliteStore {
    /// One transaction, the write lock taken up front, so the record, the
    /// insert and the forgetting see the same proposals: of adds racing for
    /// a proposer's last place, one is kept.
    async fn add(
        &self,
        proposal: &StoredProposal,
        pending_at_most: usize,
        ended_at_most: usize,
    ) -> Result<Admission, StoreError> {
        let (token_id, token_label) = match &proposal.proposer {
            Proposer::Token { id, label } => (Some(id.as_str()), Some(label.as_str())),
            Proposer::NoToken => (None, None),
        };
        let _call = self.call().await;
        let mut conn = self.pool.acquire().await.map_err(unavailable)?;
        let mut tx = conn
            .begin_with("BEGIN IMMEDIATE")
            .await
            .map_err(unavailable)?;
        let record = record(&mut tx, token_id, proposal.created_at).await?;
        if let Err(refused) = record.admits(proposal.created_at, pending_at_most) {
            tx.rollback().await.map_err(unavailable)?;
            return Ok(Admission::Refused(refused));
        }
        sqlx::query(sql::INSERT_PROPOSAL)
            .bind(proposal.id.as_str())
            .bind(token_id)
            .bind(token_label)
            .bind(proposal.base.as_str())
            .bind(proposal.changes.as_str())
            .bind(proposal.rationale.as_str())
            .bind(proposal.status.as_str())
            .bind(proposal.created_at.as_millisecond())
            .bind(proposal.expires_at.as_millisecond())
            .execute(&mut *tx)
            .await
            .map_err(unavailable)?;
        sqlx::query(sql::FORGET_OLD_PROPOSALS)
            .bind(as_count(ended_at_most))
            .execute(&mut *tx)
            .await
            .map_err(unavailable)?;
        tx.commit().await.map_err(unavailable)?;
        Ok(Admission::Kept)
    }

    async fn get(&self, id: &ProposalId) -> Result<Option<StoredProposal>, StoreError> {
        let _call = self.call().await;
        let found = sqlx::query(sql::GET_PROPOSAL)
            .bind(id.as_str())
            .fetch_optional(&self.pool)
            .await
            .map_err(unavailable)?;
        found.as_ref().map(row::proposal).transpose()
    }

    async fn all(&self) -> Result<Vec<StoredProposal>, StoreError> {
        let _call = self.call().await;
        let rows = sqlx::query(sql::ALL_PROPOSALS)
            .fetch_all(&self.pool)
            .await
            .map_err(unavailable)?;
        rows.iter().map(row::proposal).collect()
    }

    /// One statement, its `WHERE` naming the status it changes from, so of
    /// changes racing from one status SQLite makes exactly one. So are
    /// [`reject`](Self::reject), [`approve`](Self::approve) and
    /// [`applied`](Self::applied).
    async fn change_status(
        &self,
        id: &ProposalId,
        from: ProposalStatus,
        to: ProposalStatus,
    ) -> Result<bool, StoreError> {
        let _call = self.call().await;
        let done = sqlx::query(sql::CHANGE_PROPOSAL_STATUS)
            .bind(to.as_str())
            .bind(id.as_str())
            .bind(from.as_str())
            .execute(&self.pool)
            .await
            .map_err(unavailable)?;
        Ok(done.rows_affected() > 0)
    }

    async fn reject(
        &self,
        id: &ProposalId,
        at: Timestamp,
        reasons: &[RejectReason],
    ) -> Result<bool, StoreError> {
        let texts: Vec<&str> = reasons.iter().map(RejectReason::as_str).collect();
        let reasons = serde_json::to_string(&texts).map_err(|error| StoreError::Unavailable {
            detail: error.to_string(),
        })?;
        let _call = self.call().await;
        let done = sqlx::query(sql::REJECT_PROPOSAL)
            .bind(at.as_millisecond())
            .bind(reasons)
            .bind(id.as_str())
            .execute(&self.pool)
            .await
            .map_err(unavailable)?;
        Ok(done.rows_affected() > 0)
    }

    async fn approve(
        &self,
        id: &ProposalId,
        at: Timestamp,
        from: Option<IpAddr>,
    ) -> Result<bool, StoreError> {
        let _call = self.call().await;
        let done = sqlx::query(sql::APPROVE_PROPOSAL)
            .bind(at.as_millisecond())
            .bind(from.map(|address| address.to_string()))
            .bind(id.as_str())
            .execute(&self.pool)
            .await
            .map_err(unavailable)?;
        Ok(done.rows_affected() > 0)
    }

    async fn applied(&self, id: &ProposalId, applied_as: Option<&str>) -> Result<bool, StoreError> {
        let _call = self.call().await;
        let done = sqlx::query(sql::PROPOSAL_APPLIED)
            .bind(applied_as)
            .bind(id.as_str())
            .execute(&self.pool)
            .await
            .map_err(unavailable)?;
        Ok(done.rows_affected() > 0)
    }
}
