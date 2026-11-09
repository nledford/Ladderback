//! The keys of every `rustic_core` struct the file's settings map onto, split
//! into the ones the file takes and the ones it deliberately doesn't. A parity
//! test compares each pair with what the pinned `rustic_core` has, so an
//! upgrade that adds a key fails until someone decides which list it is in, and
//! another compares the lists with the tables that read the file.

/// `KeepOptions` keys a plan's `[plans.retention]` takes.
pub(crate) const RETENTION_MAPPED: &[&str] = &[
    "keep-last",
    "keep-minutely",
    "keep-hourly",
    "keep-daily",
    "keep-weekly",
    "keep-monthly",
    "keep-quarter-yearly",
    "keep-half-yearly",
    "keep-yearly",
    "keep-within",
    "keep-within-minutely",
    "keep-within-hourly",
    "keep-within-daily",
    "keep-within-weekly",
    "keep-within-monthly",
    "keep-within-quarter-yearly",
    "keep-within-half-yearly",
    "keep-within-yearly",
];
/// `KeepOptions` keys it doesn't: Ladderback picks the snapshots of a plan by
/// its own tags, and keeping nothing is not a policy.
pub(crate) const RETENTION_UNSUPPORTED: &[&str] =
    &["keep-tags", "keep-ids", "keep-none", "delete-unchanged"];

/// `Excludes` keys a plan takes.
pub(crate) const EXCLUDES_MAPPED: &[&str] = &["globs", "iglobs", "glob-files", "iglob-files"];
pub(crate) const EXCLUDES_UNSUPPORTED: &[&str] = &[];

/// `LocalSourceFilterOptions` keys a plan takes.
pub(crate) const FILTER_MAPPED: &[&str] = &[
    "git-ignore",
    "no-require-git",
    "custom-ignorefiles",
    "exclude-if-present",
    "exclude-if-xattr",
    "one-file-system",
];
pub(crate) const FILTER_UNSUPPORTED: &[&str] = &["exclude-larger-than"];

/// `SnapshotOptions` keys a plan takes.
pub(crate) const SNAPSHOT_MAPPED: &[&str] = &["label", "tags", "host"];
pub(crate) const SNAPSHOT_UNSUPPORTED: &[&str] = &[
    "description",
    "description-from",
    "time",
    "delete-never",
    "delete-after",
    "command",
];
