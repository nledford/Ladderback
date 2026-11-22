//! The agent's outbox and the hub's reports of its agents (technical design
//! §7.2, §9 `sync_outbox` and `sync_reports`): the `Outbox` and
//! `ReportInbox` ports on the store's own file, built only with the hub
//! (decision 122).

use app::ports::{Appended, Ingested, Outbox, OutboxEntry, ReportInbox, StoreError};
use async_trait::async_trait;
use domain::fleet::{AgentReport, Ingest, OutboxEpoch, ReportSeq};
use domain::{AgentId, Timestamp};

use crate::SqliteStore;

#[async_trait]
impl Outbox for SqliteStore {
    async fn append(&self, report: &AgentReport) -> Result<Appended, StoreError> {
        todo!()
    }

    async fn unacknowledged(
        &self,
        after: Option<ReportSeq>,
        limit: usize,
    ) -> Result<Vec<OutboxEntry>, StoreError> {
        todo!()
    }

    async fn acknowledge(&self, epoch: OutboxEpoch, through: ReportSeq) -> Result<(), StoreError> {
        todo!()
    }
}

#[async_trait]
impl ReportInbox for SqliteStore {
    async fn ingest(
        &self,
        agent: AgentId,
        entry: &OutboxEntry,
        received_at: Timestamp,
    ) -> Result<Ingested, StoreError> {
        todo!()
    }

    async fn reports(&self, agent: AgentId) -> Result<Vec<OutboxEntry>, StoreError> {
        todo!()
    }

    async fn dropped(&self, agent: AgentId) -> Result<u64, StoreError> {
        todo!()
    }
}
