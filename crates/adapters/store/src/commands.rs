//! A paired machine's ledger of its hub's commands (§16, decisions 120 and
//! 176; tables `hub_commands` and `hub_runs`): the `CommandLedger` port on
//! the store's own file.

use app::ports::{CommandLedger, Remembered, StoreError};
use async_trait::async_trait;
use domain::Timestamp;
use domain::fleet::{CommandId, HubRun};

use crate::SqliteStore;

#[async_trait]
impl CommandLedger for SqliteStore {
    async fn remember(&self, _id: CommandId, _now: Timestamp) -> Result<Remembered, StoreError> {
        todo!()
    }

    async fn ran(&self, _run: &HubRun) -> Result<(), StoreError> {
        todo!()
    }

    async fn runs(&self) -> Result<Vec<HubRun>, StoreError> {
        todo!()
    }
}
