use std::sync::{Mutex, PoisonError};

use async_trait::async_trait;
use domain::fleet::{COMMANDS_REMEMBERED_FOR, CommandId, HubRun, MAX_COMMANDS_REMEMBERED};
use domain::{SignedDuration, Timestamp};

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
        let mut kept = self.kept();
        kept.commands
            .retain(|(_, at)| now.duration_since(*at) < COMMANDS_REMEMBERED_FOR);
        if kept.commands.iter().any(|(kept, _)| *kept == id) {
            return Ok(Remembered::Seen);
        }
        if kept.commands.len() >= MAX_COMMANDS_REMEMBERED
            && let Some(oldest) = kept.commands.iter().map(|(_, at)| *at).min()
        {
            return Ok(Remembered::Full { oldest });
        }
        kept.commands.push((id, now));
        Ok(Remembered::New)
    }

    async fn ran(&self, run: &HubRun) -> Result<(), StoreError> {
        let mut kept = self.kept();
        kept.runs
            .retain(|kept| run.at.duration_since(kept.at) < SignedDuration::from_hours(1));
        kept.runs.push(run.clone());
        Ok(())
    }

    async fn runs(&self) -> Result<Vec<HubRun>, StoreError> {
        let mut runs = self.kept().runs.clone();
        runs.sort_by_key(|run| run.at);
        Ok(runs)
    }
}
