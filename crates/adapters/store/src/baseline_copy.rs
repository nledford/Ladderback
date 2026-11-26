//! The one-off copy of an operation log the migrations before the baseline
//! schema left into the baseline (§16, decisions 165 and 170).

use std::path::PathBuf;

/// What the copy of an old log into the baseline did, for the daemon's log.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BaselineCopy {
    /// The last migration of the old set the log held.
    pub from_version: i64,
    /// Where the old log is kept: `<log>.before-baseline`.
    pub kept: PathBuf,
    /// The rows the baseline's rules prove are junk, dropped, by table.
    pub dropped: Vec<Dropped>,
}

/// Rows of one table the copy dropped as junk (decision 165): a removed
/// machine's current state, or a second report of an operation's happening.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Dropped {
    pub table: String,
    pub rows: u64,
}
