//! The proposals (technical design §9 `proposals`; decision 89): the
//! `ProposalStore` port on the store's own file.

use app::ports::{Added, ProposalStore, StoreError, StoredProposal};
use async_trait::async_trait;
use domain::planning::{ProposalId, ProposalStatus, Proposer};
use sqlx::Connection;

use crate::{SqliteStore, row, sql, unavailable};

/// A number of rows as SQLite counts them, which a `usize` beyond `i64`
/// never reaches: such a limit is no limit.
fn as_count(limit: usize) -> i64 {
    i64::try_from(limit).unwrap_or(i64::MAX)
}

#[async_trait]
impl ProposalStore for SqliteStore {
    /// One transaction, the write lock taken up front, so the count, the
    /// insert and the forgetting see the same proposals: of adds racing for
    /// a proposer's last place, one is kept.
    async fn add(
        &self,
        proposal: &StoredProposal,
        pending_at_most: usize,
        ended_at_most: usize,
    ) -> Result<Added, StoreError> {
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
        let pending: i64 = sqlx::query_scalar(sql::COUNT_PENDING_PROPOSALS)
            .bind(token_id)
            .bind(proposal.created_at.as_millisecond())
            .fetch_one(&mut *tx)
            .await
            .map_err(unavailable)?;
        if pending >= as_count(pending_at_most) {
            tx.rollback().await.map_err(unavailable)?;
            return Ok(Added::Full);
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
        Ok(Added::Kept)
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
    /// changes racing from one status SQLite makes exactly one.
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
}
