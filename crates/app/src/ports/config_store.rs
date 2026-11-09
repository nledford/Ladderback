use async_trait::async_trait;
use domain::planning::AgentConfig;

/// Keeps the agent's configuration: the plans, repositories and settings the
/// user wrote, as one whole (technical design §3.2 and §6.1).
///
/// The real store is a TOML file, and a person edits it as well as the agent,
/// so `load` is where a file nobody vetted comes in, and every way it can be
/// wrong is a [`ConfigError`] that says what, in which file and where.
///
/// Contract (checked by `contract::config_store`):
/// - [`load`](Self::load) on a store that was never saved to returns the
///   empty configuration, [`AgentConfig::default`]: an agent that was never
///   configured has nothing to do, and isn't broken.
/// - After [`save`](Self::save), `load` returns a configuration equal to the
///   one saved, down to the order of each list, and does so again for as long
///   as nothing else is saved.
/// - The last `save` wins: it replaces everything an earlier one stored.
#[async_trait]
pub trait ConfigStore: Send + Sync {
    /// The stored configuration, or the empty one when none was ever saved.
    async fn load(&self) -> Result<AgentConfig, ConfigError>;

    /// Stores `config` in place of whatever was stored.
    async fn save(&self, config: &AgentConfig) -> Result<(), ConfigError>;
}

/// Where in a file a [`ConfigError::Invalid`] was found: the line and the
/// column, both counted from 1.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Position {
    pub line: usize,
    pub column: usize,
}

/// Why the store couldn't give or take the configuration. `file` is the
/// path as the user knows it, and `detail` is the store's own words for what
/// went wrong, which is for a person to read and never for a program to match.
///
/// Neither ever holds a secret: a store reports where a value is and what is
/// wrong with it, and leaves the value out.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConfigError {
    /// The file couldn't be read or written.
    Unavailable { file: String, detail: String },
    /// The file is longer than `limit` bytes, which is far more than any
    /// configuration needs, so it isn't read.
    TooLarge { file: String, limit: u64 },
    /// The file doesn't say which version of the format it is in, so it can't
    /// be told apart from a file of another kind.
    SchemaMissing { file: String },
    /// The file is in a version of the format that is newer than this build
    /// reads. It isn't touched: a newer build wrote it, and may be back.
    SchemaNewer {
        file: String,
        found: u64,
        supported: u64,
    },
    /// The file is not a valid configuration. `key` is the setting at fault,
    /// as a path such as `plans[1].schedule.cron`, and `at` where in the
    /// file it is, when the store can tell.
    Invalid {
        file: String,
        key: Option<String>,
        at: Option<Position>,
        detail: String,
    },
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(self, f)
    }
}

impl std::error::Error for ConfigError {}
