//! The SQLite store against the operation store's, the schedule store's,
//! the API token store's, the session store's, the stats store's and the
//! proposal store's contract suites, the ones the in-memory fakes pass too,
//! so the fakes can't drift from the real thing.

mod support;

use std::sync::{Arc, Mutex};

use app::contract;
use app::ports::{ApiTokenStore, ProposalStore, ScheduleStore, SessionStore, StatsStore};
use async_trait::async_trait;
use support::*;

#[tokio::test]
async fn sqlite_store_meets_the_operation_store_contract() {
    let scratch = Scratch::new();
    let store = scratch.open().await;
    within(
        120,
        "the contract suite",
        contract::operation_store::run(&store),
    )
    .await;
    store.close().await;
}

#[tokio::test]
async fn sqlite_store_recovers_its_unfinished_operations() {
    let scratch = Scratch::new();
    let store = scratch.open().await;
    within(
        120,
        "the recovery contract suite",
        contract::operation_store::run_recovery(&store),
    )
    .await;
    store.close().await;
}

/// A new scratch database for each `empty`, and the last one opened again
/// for `reopened`, as the daemon opens its file on a restart.
#[derive(Default)]
struct SqliteFiles(Mutex<Vec<Scratch>>);

impl SqliteFiles {
    async fn new_store(&self) -> store::SqliteStore {
        let scratch = Scratch::new();
        let store = scratch.open().await;
        self.0.lock().unwrap().push(scratch);
        store
    }

    /// A new scratch database, on a hub that has registered `machines`.
    async fn new_store_knowing(
        &self,
        machines: impl IntoIterator<Item = domain::AgentId>,
    ) -> store::SqliteStore {
        let store = self.new_store().await;
        let path = self
            .0
            .lock()
            .unwrap()
            .last()
            .expect("a store was opened")
            .path();
        let mut raw = raw_connection(&path).await;
        for machine in machines {
            register_machine(&mut raw, machine).await;
        }
        store
    }

    async fn last_store(&self) -> store::SqliteStore {
        let path = self
            .0
            .lock()
            .unwrap()
            .last()
            .expect("a store was opened")
            .path();
        store::SqliteStore::open(&path)
            .await
            .expect("the scratch database opens again")
    }
}

#[async_trait]
impl contract::schedule_store::ScheduleHarness for SqliteFiles {
    async fn empty(&self) -> Arc<dyn ScheduleStore> {
        Arc::new(self.new_store().await)
    }

    async fn reopened(&self) -> Arc<dyn ScheduleStore> {
        Arc::new(self.last_store().await)
    }
}

#[async_trait]
impl contract::api_token_store::ApiTokenHarness for SqliteFiles {
    async fn empty(&self) -> Arc<dyn ApiTokenStore> {
        Arc::new(self.new_store().await)
    }

    async fn reopened(&self) -> Arc<dyn ApiTokenStore> {
        Arc::new(self.last_store().await)
    }
}

#[async_trait]
impl contract::session_store::SessionHarness for SqliteFiles {
    async fn empty(&self) -> Arc<dyn SessionStore> {
        Arc::new(self.new_store().await)
    }

    async fn reopened(&self) -> Arc<dyn SessionStore> {
        Arc::new(self.last_store().await)
    }
}

#[async_trait]
impl contract::stats_store::StatsHarness for SqliteFiles {
    async fn empty(&self) -> Arc<dyn StatsStore> {
        Arc::new(self.new_store().await)
    }

    async fn reopened(&self) -> Arc<dyn StatsStore> {
        Arc::new(self.last_store().await)
    }
}

#[tokio::test]
async fn sqlite_store_meets_the_schedule_store_contract() {
    within(
        120,
        "the schedule store contract suite",
        contract::schedule_store::run(&SqliteFiles::default()),
    )
    .await;
}

#[tokio::test]
async fn sqlite_store_keeps_each_plans_pending_retry() {
    within(
        120,
        "the pending retry contract suite",
        contract::schedule_store::run_retries(&SqliteFiles::default()),
    )
    .await;
}

#[tokio::test]
async fn sqlite_store_keeps_the_last_release_check() {
    within(
        120,
        "the release check contract suite",
        contract::schedule_store::run_release_check(&SqliteFiles::default()),
    )
    .await;
}

#[tokio::test]
async fn sqlite_store_meets_the_api_token_store_contract() {
    within(
        120,
        "the API token store contract suite",
        contract::api_token_store::run(&SqliteFiles::default()),
    )
    .await;
}

#[tokio::test]
async fn sqlite_store_meets_the_session_store_contract() {
    within(
        120,
        "the session store contract suite",
        contract::session_store::run(&SqliteFiles::default()),
    )
    .await;
}

#[tokio::test]
async fn sqlite_store_meets_the_stats_store_contract() {
    within(
        120,
        "the stats store contract suite",
        contract::stats_store::run(&SqliteFiles::default()),
    )
    .await;
}

#[async_trait]
impl contract::proposal_store::ProposalHarness for SqliteFiles {
    async fn empty(&self) -> Arc<dyn ProposalStore> {
        Arc::new(self.new_store().await)
    }

    async fn reopened(&self) -> Arc<dyn ProposalStore> {
        Arc::new(self.last_store().await)
    }
}

#[tokio::test]
async fn sqlite_store_meets_the_proposal_store_contract() {
    within(
        120,
        "the proposal store contract suite",
        contract::proposal_store::run(&SqliteFiles::default()),
    )
    .await;
}

#[async_trait]
impl contract::agent_registry::AgentRegistryHarness for SqliteFiles {
    async fn empty(&self) -> Arc<dyn app::ports::AgentRegistry> {
        Arc::new(self.new_store().await)
    }

    async fn reopened(&self) -> Arc<dyn app::ports::AgentRegistry> {
        Arc::new(self.last_store().await)
    }
}

#[tokio::test]
async fn sqlite_store_meets_the_agent_registry_contract() {
    within(
        120,
        "the agent registry contract suite",
        contract::agent_registry::run(&SqliteFiles::default()),
    )
    .await;
}

#[async_trait]
impl contract::outbox::OutboxHarness for SqliteFiles {
    async fn empty(&self) -> Arc<dyn app::ports::Outbox> {
        Arc::new(self.new_store().await)
    }

    async fn reopened(&self) -> Arc<dyn app::ports::Outbox> {
        Arc::new(self.last_store().await)
    }
}

#[tokio::test]
async fn sqlite_store_meets_the_outbox_contract() {
    within(
        120,
        "the outbox contract suite",
        contract::outbox::run(&SqliteFiles::default()),
    )
    .await;
}

#[async_trait]
impl contract::outbox::ReportInboxHarness for SqliteFiles {
    async fn empty(&self) -> Arc<dyn app::ports::ReportInbox> {
        Arc::new(self.new_store_knowing(contract::outbox::MACHINES).await)
    }

    async fn reopened(&self) -> Arc<dyn app::ports::ReportInbox> {
        Arc::new(self.last_store().await)
    }
}

#[tokio::test]
async fn sqlite_store_meets_the_report_inbox_contract() {
    within(
        120,
        "the report inbox contract suite",
        contract::outbox::run_inbox(&SqliteFiles::default()),
    )
    .await;
}

#[async_trait]
impl contract::ownership_registry::OwnershipRegistryHarness for SqliteFiles {
    async fn empty(&self) -> Arc<dyn app::ports::OwnershipRegistry> {
        Arc::new(
            self.new_store_knowing(contract::ownership_registry::machines())
                .await,
        )
    }

    async fn reopened(&self) -> Arc<dyn app::ports::OwnershipRegistry> {
        Arc::new(self.last_store().await)
    }
}

#[tokio::test]
async fn sqlite_store_meets_the_ownership_registry_contract() {
    within(
        120,
        "the ownership registry contract suite",
        contract::ownership_registry::run(&SqliteFiles::default()),
    )
    .await;
}

#[async_trait]
impl contract::lease_cache::LeaseCacheHarness for SqliteFiles {
    async fn empty(&self) -> Arc<dyn app::ports::LeaseCache> {
        Arc::new(self.new_store().await)
    }

    async fn reopened(&self) -> Arc<dyn app::ports::LeaseCache> {
        Arc::new(self.last_store().await)
    }
}

#[tokio::test]
async fn sqlite_store_meets_the_lease_cache_contract() {
    within(
        120,
        "the lease cache contract suite",
        contract::lease_cache::run(&SqliteFiles::default()),
    )
    .await;
}

#[async_trait]
impl contract::daily_copy::DailyCopyHarness for SqliteFiles {
    async fn empty(&self) -> Arc<dyn app::ports::DailyCopy> {
        Arc::new(self.new_store().await)
    }

    async fn reopened(&self) -> Arc<dyn app::ports::DailyCopy> {
        Arc::new(self.last_store().await)
    }
}

#[tokio::test]
async fn sqlite_store_meets_the_daily_copy_contract() {
    within(
        120,
        "the daily copy contract suite",
        contract::daily_copy::run(&SqliteFiles::default()),
    )
    .await;
}

#[async_trait]
impl contract::drive_flush::DriveFlushHarness for SqliteFiles {
    async fn open(&self) -> Arc<dyn app::ports::DriveFlush> {
        Arc::new(self.new_store().await)
    }

    async fn closed(&self) -> Arc<dyn app::ports::DriveFlush> {
        let store = self.new_store().await;
        store.close().await;
        Arc::new(store)
    }
}

#[tokio::test]
async fn sqlite_store_meets_the_drive_flush_contract() {
    within(
        120,
        "the drive flush contract suite",
        contract::drive_flush::run(&SqliteFiles::default()),
    )
    .await;
}
