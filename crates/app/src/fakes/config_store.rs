use std::sync::{Mutex, MutexGuard, PoisonError};

use async_trait::async_trait;
use domain::planning::AgentConfig;

use crate::ports::{ConfigError, ConfigStore};

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
        drop(self.saved());
        todo!()
    }

    async fn save(&self, _config: &AgentConfig) -> Result<(), ConfigError> {
        todo!()
    }
}
