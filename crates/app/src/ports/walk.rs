//! The `WalkSources` port: a plan dry run's walk of a plan's sources
//! through its source filter (§16, decision 89, point 13). The walk reads
//! the disk and rustic owns the filter, so it happens in the engine
//! adapter, as a backup walks. It counts what it reads and never names a
//! path.

use std::sync::Arc;

use async_trait::async_trait;
use domain::SignedDuration;
use domain::planning::{Source, SourceFilter};

/// How far one walk goes: at most `entries` entries read below the
/// sources (files, folders and links alike), for at most `time`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WalkLimits {
    pub entries: u64,
    pub time: SignedDuration,
}

/// Asked before each entry: whether the walk must stop now to give way.
pub type GiveWay = Arc<dyn Fn() -> bool + Send + Sync>;

/// What a walk counted: the files (every entry but a folder) and their
/// bytes it would back up, the entries it couldn't read, the warnings the
/// engine raised (counted, never logged: they can name a path), and why
/// it stopped before the end, if it did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Walked {
    pub files: u64,
    pub bytes: u64,
    pub unreadable: u64,
    pub warnings: u64,
    pub stopped: Option<WalkStop>,
}

/// Why a walk stopped before the end.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WalkStop {
    /// It read its limit of entries, and there were more.
    Entries,
    /// Its time ran out.
    Time,
    /// It gave way ([`GiveWay`]).
    GaveWay,
}

/// Why a walk didn't start.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WalkError {
    /// The source filter isn't one the engine can apply: a glob that is
    /// none, or a glob file it can't read, that isn't a regular file or
    /// that is larger than the engine reads.
    Filter,
    /// The engine failed; `detail` is its own message.
    Engine { detail: String },
}

/// Walks sources as a backup would, counting and naming nothing.
#[async_trait]
pub trait WalkSources: Send + Sync {
    /// Walks `sources`, each resolved already, through `filter`, within
    /// `limits`, asking `give_way` before each entry. An entry it can't
    /// read is counted, not a failure.
    async fn walk(
        &self,
        sources: &[Source],
        filter: &SourceFilter,
        limits: WalkLimits,
        give_way: GiveWay,
    ) -> Result<Walked, WalkError>;
}
