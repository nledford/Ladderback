use std::sync::{Mutex, MutexGuard, PoisonError};

use async_trait::async_trait;
use domain::planning::{AgentConfig, RepositoryConfig};
use domain::{RepositoryAlias, RepositoryId};

use crate::ports::{ConfigError, ConfigStore, RecordError};

/// A configuration that lives in memory, for use-case tests and acceptance
/// scenarios. It keeps what the real store keeps, and nothing else: it never
/// fails, because a file is the only part of the real store that can.
#[derive(Default)]
pub struct InMemoryConfigStore {
    saved: Mutex<Option<AgentConfig>>,
}

impl InMemoryConfigStore {
    pub fn new() -> Self {
        Self::default()
    }

    // The configuration is only ever replaced whole, so a panic elsewhere
    // can't leave it half-written.
    fn saved(&self) -> MutexGuard<'_, Option<AgentConfig>> {
        self.saved.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

#[async_trait]
impl ConfigStore for InMemoryConfigStore {
    async fn load(&self) -> Result<AgentConfig, ConfigError> {
        Ok(self.saved().clone().unwrap_or_default())
    }

    async fn save(&self, config: &AgentConfig) -> Result<(), ConfigError> {
        *self.saved() = Some(config.clone());
        Ok(())
    }

    async fn record_id(
        &self,
        alias: &RepositoryAlias,
        read: &RepositoryConfig,
        id: &RepositoryId,
    ) -> Result<(), RecordError> {
        let mut saved = self.saved();
        let empty = AgentConfig::default();
        let current = saved.as_ref().unwrap_or(&empty);
        let recorded = current
            .record_id(alias, read, id)
            .map_err(RecordError::NotRecorded)?;
        *saved = Some(recorded);
        Ok(())
    }
}
