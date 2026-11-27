use std::sync::{Mutex, PoisonError};

use async_trait::async_trait;
use domain::Timestamp;
use domain::fleet::{CommandId, HubRun};

use crate::ports::{CommandLedger, Remembered, StoreError};

/// A machine's command ledger in memory, for use-case tests and acceptance
/// scenarios: one instance stands for one database, so a use case built
/// again on it stands for one after a restart.
#[derive(Debug, Default)]
pub struct InMemoryCommandLedger {
    kept: Mutex<Kept>,
}

#[derive(Debug, Default)]
struct Kept {
    commands: Vec<(CommandId, Timestamp)>,
    runs: Vec<HubRun>,
}

impl InMemoryCommandLedger {
    fn kept(&self) -> std::sync::MutexGuard<'_, Kept> {
        self.kept.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

#[async_trait]
impl CommandLedger for InMemoryCommandLedger {
    async fn remember(&self, id: CommandId, now: Timestamp) -> Result<Remembered, StoreError> {
        let _ = (id, now, self.kept());
        todo!()
    }

    async fn ran(&self, run: &HubRun) -> Result<(), StoreError> {
        let _ = run;
        todo!()
    }

    async fn runs(&self) -> Result<Vec<HubRun>, StoreError> {
        todo!()
    }
}
