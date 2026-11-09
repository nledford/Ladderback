//! `ConfigStore` over the file: load, migrate, write and keep history.

use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};

use app::ports::{Clock, ConfigError, ConfigStore, Problem};
use async_trait::async_trait;
use domain::planning::AgentConfig;

use crate::files::{self, Contents};
use crate::schema::{MAX_CONFIG_BYTES, Schemas, parse, render_as};

/// The agent's configuration in a TOML file, with the earlier versions of the
/// file kept in a `history` folder next to it.
pub struct FileConfigStore {
    path: PathBuf,
    clock: Arc<dyn Clock>,
    schemas: Schemas,
    /// One load or save at a time: a load that migrates must not undo a save
    /// that comes while it works.
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

    /// Runs `work` on a thread for blocking calls, behind the gate.
    async fn exclusively<T, F>(&self, work: F) -> Result<T, ConfigError>
    where
        T: Send + 'static,
        F: FnOnce(&Job) -> Result<T, ConfigError> + Send + 'static,
    {
        let job = Job {
            path: self.path.clone(),
            clock: self.clock.clone(),
            schemas: self.schemas.clone(),
        };
        let gate = self.gate.clone();
        tokio::task::spawn_blocking(move || {
            // A panic in an earlier job leaves nothing half-done that this one
            // would trip on: the file is only ever replaced whole.
            let _turn = gate.lock().unwrap_or_else(PoisonError::into_inner);
            work(&job)
        })
        .await
        .unwrap_or_else(|_| {
            Err(ConfigError::Unavailable {
                file: self.path.display().to_string(),
                detail: "the work on the file didn't finish".to_owned(),
            })
        })
    }
}

/// What a load or a save needs, owned so it can move to the blocking thread.
struct Job {
    path: PathBuf,
    clock: Arc<dyn Clock>,
    schemas: Schemas,
}

impl Job {
    fn name(&self) -> String {
        self.path.display().to_string()
    }

    fn unavailable(&self, error: &std::io::Error) -> ConfigError {
        ConfigError::Unavailable {
            file: self.name(),
            detail: error.to_string(),
        }
    }

    fn too_large(&self) -> ConfigError {
        ConfigError::TooLarge {
            file: self.name(),
            limit: MAX_CONFIG_BYTES,
        }
    }

    /// The file as it is: its bytes, or `None` when there is no file.
    fn current_bytes(&self) -> Result<Option<Vec<u8>>, ConfigError> {
        match files::read(&self.path).map_err(|error| self.unavailable(&error))? {
            Contents::Missing => Ok(None),
            Contents::Bytes(bytes) => Ok(Some(bytes)),
            Contents::TooLarge => Err(self.too_large()),
        }
    }

    /// Keeps `old` in the history, then replaces the file with `text`.
    fn replace(&self, old: Option<&[u8]>, text: &str) -> Result<(), ConfigError> {
        if let Some(old) = old {
            files::keep(&self.path, old, self.clock.now())
                .map_err(|error| self.unavailable(&error))?;
        }
        files::write(&self.path, text.as_bytes()).map_err(|error| self.unavailable(&error))
    }

    fn load(&self) -> Result<AgentConfig, ConfigError> {
        let Some(bytes) = self.current_bytes()? else {
            return Ok(AgentConfig::default());
        };
        let text = String::from_utf8(bytes).map_err(|_| ConfigError::Invalid {
            file: self.name(),
            key: None,
            at: None,
            problem: Problem::NotUtf8,
            detail: String::new(),
        })?;
        let parsed = parse(&self.path, &text, &self.schemas)?;
        if parsed.migrated {
            // The file is only replaced once it is known to be valid, and
            // only after the version the user wrote is safe in the history.
            let current = render_as(&self.path, &parsed.config, self.schemas.current)?;
            self.replace(Some(text.as_bytes()), &current)?;
        }
        Ok(parsed.config)
    }

    fn save(&self, config: &AgentConfig) -> Result<(), ConfigError> {
        let text = render_as(&self.path, config, self.schemas.current)?;
        let old = self.current_bytes()?;
        self.replace(old.as_deref(), &text)
    }
}

#[async_trait]
impl ConfigStore for FileConfigStore {
    async fn load(&self) -> Result<AgentConfig, ConfigError> {
        self.exclusively(|job| job.load()).await
    }

    async fn save(&self, config: &AgentConfig) -> Result<(), ConfigError> {
        let config = config.clone();
        self.exclusively(move |job| job.save(&config)).await
    }
}
