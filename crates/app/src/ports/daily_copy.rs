//! The daily copy of the database (technical design §9; §16, decision
//! 174): a whole copy of the operation log, beside it in the data folder,
//! taken while the daemon serves and put in place of the one before only
//! once it is written.

use async_trait::async_trait;
use domain::Timestamp;

/// Why a daily copy wasn't taken, or when the one there is was taken
/// couldn't be read. `detail` is the database's or the operating system's
/// own words, which hold paths and never a secret: the database holds
/// none, only digests (I4).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DailyCopyError {
    /// The database couldn't be reached: it is closed, or no connection
    /// came free in time. The copy was skipped.
    Unavailable { detail: String },
    /// The copy couldn't be written, synced or put in place, or the one
    /// there is couldn't be read. The one before, if any, stays as it was.
    Failed { detail: String },
    /// The copy took the place of the one before, but its folder couldn't
    /// be synced, so the rename may not survive a power loss. It counts as
    /// taken: the next is due a day later.
    Unsynced { detail: String },
}

impl std::fmt::Display for DailyCopyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(self, f)
    }
}

impl std::error::Error for DailyCopyError {}

/// Where the daily copy is kept.
#[async_trait]
pub trait DailyCopy: Send + Sync {
    /// When the copy there is was taken; `None` when there is none.
    async fn taken_at(&self) -> Result<Option<Timestamp>, DailyCopyError>;

    /// Takes a copy of the database in place of the one there is. It
    /// replaces that one only once it is whole: a copy that fails, or is
    /// dropped, before then leaves the one before as it was.
    async fn take(&self) -> Result<(), DailyCopyError>;
}
