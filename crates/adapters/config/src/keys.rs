//! The keys of every `rustic_core` struct the file's settings map onto, split
//! into the ones the file takes and the ones it deliberately doesn't. A parity
//! test compares each pair with what the pinned `rustic_core` has, so an
//! upgrade that adds a key fails until someone decides which list it is in.

// The tables that read the file use these once they exist; until then only
// the parity tests do.
#![allow(dead_code)]

/// `KeepOptions` keys a plan's `[plans.retention]` takes.
pub(crate) const RETENTION_MAPPED: &[&str] = &[];
/// `KeepOptions` keys it doesn't: Ladderback picks snapshots by its own tags.
pub(crate) const RETENTION_UNSUPPORTED: &[&str] = &[];

/// `Excludes` keys a plan takes.
pub(crate) const EXCLUDES_MAPPED: &[&str] = &[];
pub(crate) const EXCLUDES_UNSUPPORTED: &[&str] = &[];

/// `LocalSourceFilterOptions` keys a plan takes.
pub(crate) const FILTER_MAPPED: &[&str] = &[];
pub(crate) const FILTER_UNSUPPORTED: &[&str] = &[];

/// `SnapshotOptions` keys a plan takes.
pub(crate) const SNAPSHOT_MAPPED: &[&str] = &[];
pub(crate) const SNAPSHOT_UNSUPPORTED: &[&str] = &[];
