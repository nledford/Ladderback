//! `ConfigStore` over the file: load, migrate, write and keep history.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use app::ports::{Clock, ConfigError, ConfigStore};
use async_trait::async_trait;
use domain::planning::AgentConfig;

use crate::schema::Schemas;

/// The agent's configuration in a TOML file, with the earlier versions of the
/// file kept in a `history` folder next to it.
pub struct FileConfigStore {
    #[allow(dead_code)]
    path: PathBuf,
    #[allow(dead_code)]
    clock: Arc<dyn Clock>,
    #[allow(dead_code)]
    schemas: Schemas,
    /// One load or save at a time: a load that migrates must not undo a save
    /// that comes while it works.
    #[allow(dead_code)]
    gate: Arc<Mutex<()>>,
}

impl FileConfigStore {
    /// A store over the file at `path`, which needn't exist yet, reading the
    /// versions of the format this build knows. `clock` names the history.
    pub fn new(path: impl Into<PathBuf>, clock: Arc<dyn Clock>) -> Self {
        Self {
            path: path.into(),
            clock,
            schemas: Schemas::production(),
            gate: Arc::new(Mutex::new(())),
        }
    }

    /// The same store reading other versions of the format.
    #[must_use]
    pub fn with_schemas(mut self, schemas: Schemas) -> Self {
        self.schemas = schemas;
        self
    }
}

#[async_trait]
impl ConfigStore for FileConfigStore {
    async fn load(&self) -> Result<AgentConfig, ConfigError> {
        todo!("read, parse, migrate, keep the old file")
    }

    async fn save(&self, _config: &AgentConfig) -> Result<(), ConfigError> {
        todo!("render, keep the old file, write atomically")
    }
}
