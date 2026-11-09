//! The versions of the file format, and the two pure functions that read a
//! file's text into a configuration and write one back.

use std::path::Path;

use app::ports::ConfigError;
use domain::planning::AgentConfig;

/// The most a configuration file may hold. It is far more than any
/// configuration needs: a file over it is refused unread, so a file that never
/// ends can't fill the memory.
pub const MAX_CONFIG_BYTES: u64 = 1024 * 1024;

/// How many earlier versions of the file are kept in its history.
pub const KEPT_VERSIONS: usize = 10;

/// The version of the format this build writes, and the newest it reads.
pub const CURRENT_SCHEMA: u64 = 1;

/// Why a migration step gave up. `detail` is for a person, and never holds a
/// value from the file: a file may hold a secret.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MigrationError {
    pub detail: String,
}

/// One migration: it turns the table of a file in version `n` into the table
/// of version `n + 1`, and does nothing else.
pub type Step = fn(&mut toml::Table) -> Result<(), MigrationError>;

/// The versions of the format a build reads: `current`, and a `Step` for each
/// older one it can still migrate from, oldest first. The oldest version
/// read is `current` less the number of steps.
#[derive(Clone, Debug)]
pub struct Schemas {
    pub current: u64,
    pub steps: Vec<Step>,
}

impl Schemas {
    /// The versions this build knows: only the first one, so no steps yet.
    pub fn production() -> Self {
        Self {
            current: CURRENT_SCHEMA,
            steps: Vec::new(),
        }
    }

    pub fn oldest(&self) -> u64 {
        self.current
            .saturating_sub(u64::try_from(self.steps.len()).unwrap_or(u64::MAX))
    }
}

/// What a file held.
#[derive(Debug)]
pub struct Parsed {
    pub config: AgentConfig,
    /// Whether the file was in an older version and was migrated in memory.
    pub migrated: bool,
}

/// Reads the text of the file at `file`, which is only named in errors.
pub fn parse(_file: &Path, _text: &str, _schemas: &Schemas) -> Result<Parsed, ConfigError> {
    todo!("the schema probe, the migrations and the strict tables")
}

/// Writes `config` in the current version of the format: whatever is the
/// default is left out, so the text holds what the user chose.
pub fn render(_file: &Path, _config: &AgentConfig) -> Result<String, ConfigError> {
    todo!("the strict tables, written")
}
