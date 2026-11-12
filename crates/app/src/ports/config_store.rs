use async_trait::async_trait;
use domain::planning::{AgentConfig, NotRecorded, RepositoryConfig, ValueKind, Violation};
use domain::{IdKind, RepositoryAlias, RepositoryId};

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
/// - After [`record_id`](Self::record_id) returns `Ok`, `load` returns the
///   configuration [`AgentConfig::record_id`] makes of the one stored before,
///   and nothing else changed; after an `Err`, the stored configuration is
///   as it was.
#[async_trait]
pub trait ConfigStore: Send + Sync {
    /// The stored configuration, or the empty one when none was ever saved.
    async fn load(&self) -> Result<AgentConfig, ConfigError>;

    /// Stores `config` in place of whatever was stored.
    async fn save(&self, config: &AgentConfig) -> Result<(), ConfigError>;

    /// Records `id`, the repository a job opened, as the repository ID of
    /// the entry `alias`, which the job read as `read`, and changes nothing
    /// else (§6.1, decision 57). The daemon does this on its own, in a file
    /// a person may have written by hand, so a store keeps everything else
    /// it holds as it is, down to how it is written, and writes nothing when
    /// it can't: [`AgentConfig::record_id`] decides whether the ID is
    /// recorded, and the store whether it can add it alone.
    async fn record_id(
        &self,
        alias: &RepositoryAlias,
        read: &RepositoryConfig,
        id: &RepositoryId,
    ) -> Result<(), RecordError>;
}

/// Why [`ConfigStore::record_id`] recorded nothing. None of them is a
/// failure of the job that opened the repository: the next one tries again.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RecordError {
    /// The configuration says not to record it.
    NotRecorded(NotRecorded),
    /// The store can't add the ID without changing something else, so it
    /// wrote nothing.
    NotInPlace(NotInPlace),
    /// The configuration couldn't be read or written.
    Store(ConfigError),
}

/// Why a store couldn't add a repository ID and nothing else.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NotInPlace {
    /// The entry is written in a form the store doesn't add a key to.
    Form,
    /// The file with the ID added wouldn't read back as the configuration
    /// with the ID and nothing else changed.
    ReadBack,
    /// The file with the ID added would be larger than the store reads.
    TooLarge,
    /// The file changed while the store added the ID.
    ChangedMeanwhile,
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
    /// file it is, when the store can tell. `problem` says what is wrong as a
    /// code, and `detail` holds only what the parser said about it, with every
    /// value left out; it is empty when `problem` says it all.
    Invalid {
        file: String,
        key: Option<String>,
        at: Option<Position>,
        problem: Problem,
        detail: String,
    },
}

/// What is wrong with a file that is not a valid configuration. A code, never
/// a sentence: the words are the caller's to choose (technical design §8.4).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Problem {
    /// The file is not UTF-8 text.
    NotUtf8,
    /// The file is not TOML.
    Syntax,
    /// A key the format doesn't have, at the setting named by the error's key.
    UnknownKey,
    /// A key the setting needs isn't there: the error's key is the one that
    /// is missing, and its place is the table that should have it.
    MissingKey,
    /// A value of another type than the setting takes: a number where it
    /// wants text, say.
    WrongType,
    /// `schema` is not a whole number that is zero or more.
    SchemaNotAVersion,
    /// The file is in a version older than the oldest this build migrates.
    SchemaTooOld { found: u64, oldest: u64 },
    /// A migration step from an older version failed.
    Migration,
    /// A value that isn't the kind of value the setting is for.
    Value(ValueKind),
    /// A plan ID or a repository alias that isn't one.
    Id(IdKind),
    /// A tag that starts with the prefix Ladderback's own tags have.
    ReservedTag,
    /// The repository's password isn't given as the format needs.
    Password(PasswordProblem),
    /// A notification channel's URL or token isn't given as the format
    /// needs: by exactly one of its `-command`, `-keyring`, `-file` and
    /// `-env` keys (at most one for a token), as a password is, and never as
    /// text in the file.
    Secret(PasswordProblem),
    /// A schedule doesn't say what its moments are as the format needs.
    Cadence(CadenceProblem),
    /// The settings are each valid, but they disagree with one another.
    Violation(Violation),
    /// The configuration can't be written in the format.
    Unwritable,
}

/// How a schedule's moments are wrongly given: it takes exactly one of
/// `cron` and `every`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CadenceProblem {
    /// Neither is there.
    Missing,
    /// Both are there.
    Several,
}

/// How a repository's password is wrongly given: it takes exactly one of
/// `password`, `password-command`, `password-keyring`, `password-file` and
/// `password-env`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PasswordProblem {
    /// None of the five is there.
    Missing,
    /// More than one is there.
    Several,
    /// `password-file` isn't an absolute path.
    RelativeFile,
    /// `password-keyring` isn't `service/name`.
    NotAKeyringEntry,
    /// `password-command` can't be split into words.
    BadCommand,
    /// `password-command` starts its program by a relative path, which the
    /// daemon's working directory would decide.
    RelativeProgram,
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(self, f)
    }
}

impl std::error::Error for ConfigError {}
