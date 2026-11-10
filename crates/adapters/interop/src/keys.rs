//! The keys of every table of a rustic 0.11.4 profile, split into the ones the
//! import carries over, the ones it refuses and the ones it leaves out with a
//! notice. A table that rustic reads strictly refuses a key that is in none of
//! its lists; `[repository]` and the sections of other purposes are read
//! leniently, as rustic reads them.
//!
//! A parity test compares the lists of every table that is a `rustic_core`
//! struct with what the pinned `rustic_core` has, so an upgrade that adds a
//! key fails until someone decides which list it is in. The tables that are
//! `rustic` CLI structs (`[global]`, `[forget]`'s own keys, the hooks) are not
//! a library and can't be compared; their lists are copied from
//! `rustic-rs` 0.11.4's `config.rs`, `commands/backup.rs` and
//! `commands/forget.rs`.

/// The tables of the file. `init`, `copy`, `mount` and `webdav` are read
/// leniently; the others are strict.
pub(crate) const TOP_LEVEL: &[&str] = &[
    "global",
    "repository",
    "init",
    "snapshot-filter",
    "backup",
    "copy",
    "forget",
    "mount",
    "webdav",
];

/// `[snapshot-filter]`: `SnapshotFilter` of `rustic-rs`.
pub(crate) const SNAPSHOT_FILTER: &[&str] = &[
    "filter-hosts",
    "filter-labels",
    "filter-paths",
    "filter-paths-exact",
    "filter-tags",
    "filter-tags-exact",
    "filter-after",
    "filter-before",
    "filter-size",
    "filter-size-added",
    "filter-last",
    "filter-fn",
    "filter-jq",
];

/// `[global]`: `GlobalOptions` of `rustic-rs`, the flattened logging and
/// progress options included.
pub(crate) const GLOBAL: &[&str] = &[
    "profile-substitute-env",
    "use-profiles",
    "group-by",
    "dry-run",
    "dry-run-warmup",
    "check-index",
    "log-level",
    "log-level-logfile",
    "log-level-dryrun",
    "log-level-dependencies",
    "log-file",
    "no-progress",
    "json-progress",
    "progress-interval",
    "hooks",
    "env",
    "prometheus",
    "prometheus-user",
    "prometheus-pass",
    "metrics-labels",
    "opentelemetry",
    "show-time-offset",
];
/// What `[global]` gives the import.
pub(crate) const GLOBAL_MAPPED: &[&str] = &["use-profiles", "group-by", "hooks", "env"];

/// `Hooks` of `rustic-rs`: every table of hooks has these four keys.
pub(crate) const HOOKS: &[&str] = &["run-before", "run-after", "run-failed", "run-finally"];

/// `[repository]`: what `rustic_backend`'s `BackendOptions`, `rustic_core`'s
/// `RepositoryOptions` and `CredentialOptions` and `rustic-rs`'s hooks have,
/// split into the keys the import carries over and the ones it leaves out.
pub(crate) const REPOSITORY_MAPPED: &[&str] = &[
    "repository",
    "repo-hot",
    "options",
    "options-hot",
    "options-cold",
    "password",
    "password-file",
    "password-command",
    "hooks",
];
/// Not read by the import, which is lenient here: what the parity test
/// compares with `rustic_core`'s `RepositoryOptions`.
#[cfg(test)]
pub(crate) const REPOSITORY_LEFT_OUT: &[&str] = &[
    "key",
    "key-file",
    "key-command",
    "no-cache",
    "cache-dir",
    "warm-up",
    "warm-up-command",
    "warm-up-wait-command",
    "warm-up-wait",
    "warm-up-batch",
];

/// `SnapshotOptions` of `rustic_core`, flattened into `[backup]`.
pub(crate) const SNAPSHOT_MAPPED: &[&str] = &["label", "tags", "host"];
pub(crate) const SNAPSHOT_LEFT_OUT: &[&str] = &[
    "description",
    "description-from",
    "time",
    "delete-never",
    "delete-after",
    "command",
];

/// `Excludes` of `rustic_core`.
pub(crate) const EXCLUDES_MAPPED: &[&str] = &["globs", "iglobs", "glob-files", "iglob-files"];

/// `LocalSourceFilterOptions` of `rustic_core`.
pub(crate) const FILTER_MAPPED: &[&str] = &[
    "git-ignore",
    "no-require-git",
    "custom-ignorefiles",
    "exclude-if-present",
    "exclude-if-xattr",
    "one-file-system",
];
pub(crate) const FILTER_LEFT_OUT: &[&str] = &["exclude-larger-than"];

/// `ParentOptions` of `rustic_core`: how a backup picks its parent snapshot.
pub(crate) const PARENT_LEFT_OUT: &[&str] = &[
    "group-by",
    "parents",
    "skip-if-unchanged",
    "force",
    "ignore-ctime",
    "ignore-inode",
];

/// `LocalSourceSaveOptions` of `rustic_core`.
pub(crate) const SAVE_LEFT_OUT: &[&str] = &[
    "set-atime",
    "set-ctime",
    "set-devid",
    "set-blockdev",
    "set-xattrs",
];

/// The keys `BackupCmd` of `rustic-rs` has of its own, that a snapshot entry
/// has too (it is the same struct). `name` and `sources` are refused in
/// `[backup]`, `snapshots` in an entry, and `stdin-command` everywhere:
/// what it backs up is not a path.
pub(crate) const BACKUP_OWN_MAPPED: &[&str] = &["hooks"];
pub(crate) const BACKUP_OWN_LEFT_OUT: &[&str] = &[
    "stdin-filename",
    "as-path",
    "no-scan",
    "json",
    "long",
    "init",
    "options",
    "metrics-job",
    "metrics-labels",
];

/// The keys an entry names itself by, or the entries are in: not a setting.
pub(crate) const NAME: &str = "name";
pub(crate) const SOURCES: &str = "sources";
pub(crate) const SNAPSHOTS: &str = "snapshots";
pub(crate) const STDIN_COMMAND: &str = "stdin-command";

/// Every key `[backup]` and an entry take, whichever list it is in.
pub(crate) fn backup_known() -> Vec<&'static str> {
    [
        &[NAME, SOURCES, SNAPSHOTS, STDIN_COMMAND][..],
        SNAPSHOT_MAPPED,
        SNAPSHOT_LEFT_OUT,
        EXCLUDES_MAPPED,
        FILTER_MAPPED,
        FILTER_LEFT_OUT,
        PARENT_LEFT_OUT,
        SAVE_LEFT_OUT,
        BACKUP_OWN_MAPPED,
        BACKUP_OWN_LEFT_OUT,
    ]
    .into_iter()
    .flatten()
    .copied()
    .collect()
}

/// Whether `[backup]` or an entry gives the import the key.
pub(crate) fn backup_mapped(key: &str) -> bool {
    [
        SNAPSHOT_MAPPED,
        EXCLUDES_MAPPED,
        FILTER_MAPPED,
        BACKUP_OWN_MAPPED,
    ]
    .iter()
    .any(|keys| keys.contains(&key))
}

/// `KeepOptions` of `rustic_core`, flattened into `[forget]`.
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
/// A rule that keeps particular snapshots: refused when it names any.
pub(crate) const RETENTION_REFUSED: &[&str] = &["keep-tags", "keep-ids"];
pub(crate) const RETENTION_LEFT_OUT: &[&str] = &["keep-none", "delete-unchanged"];

/// What `ForgetOptions` of `rustic-rs` has of its own: `[forget]` is these,
/// `KeepOptions` and `SnapshotFilter`.
pub(crate) const FORGET_OWN_MAPPED: &[&str] = &["group-by", "prune"];

/// Every key `[forget]` takes.
pub(crate) fn forget_known() -> Vec<&'static str> {
    [
        FORGET_OWN_MAPPED,
        RETENTION_MAPPED,
        RETENTION_REFUSED,
        RETENTION_LEFT_OUT,
        SNAPSHOT_FILTER,
    ]
    .into_iter()
    .flatten()
    .copied()
    .collect()
}

/// Whether `[forget]` gives the import the key.
pub(crate) fn forget_mapped(key: &str) -> bool {
    FORGET_OWN_MAPPED.contains(&key)
        || RETENTION_MAPPED.contains(&key)
        || RETENTION_REFUSED.contains(&key)
}

/// Parity with the pinned `rustic_core`: every key of each of its structs
/// that a profile table flattens in is in one of the lists above, and the
/// lists name nothing the struct doesn't have. A pin upgrade that adds a key
/// fails here until someone decides what the import does with it.
#[cfg(test)]
mod parity {
    use std::collections::BTreeSet;

    use rustic_core::{
        Excludes, KeepOptions, LocalSourceFilterOptions, LocalSourceSaveOptions, ParentOptions,
        RepositoryOptions, SnapshotOptions,
    };
    use serde::Deserialize;
    use serde::de::{self, Deserializer, Visitor};

    use super::*;

    /// A deserializer that only listens for the field list serde passes to
    /// `deserialize_struct`, and then refuses.
    struct Recorder<'a>(&'a mut Vec<&'static str>);

    impl<'de> Deserializer<'de> for Recorder<'_> {
        type Error = de::value::Error;

        fn deserialize_any<V: Visitor<'de>>(self, _visitor: V) -> Result<V::Value, Self::Error> {
            Err(de::Error::custom("a struct was expected"))
        }

        fn deserialize_struct<V: Visitor<'de>>(
            self,
            _name: &'static str,
            fields: &'static [&'static str],
            _visitor: V,
        ) -> Result<V::Value, Self::Error> {
            self.0.extend_from_slice(fields);
            Err(de::Error::custom("only the field list was wanted"))
        }

        serde::forward_to_deserialize_any! {
            bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string bytes
            byte_buf option unit unit_struct newtype_struct seq tuple tuple_struct map enum
            identifier ignored_any
        }
    }

    /// The keys serde derived for `T`, which is what its file format accepts.
    fn upstream<T: for<'de> Deserialize<'de>>() -> BTreeSet<&'static str> {
        let mut keys = Vec::new();
        let _ = T::deserialize(Recorder(&mut keys));
        keys.into_iter().collect()
    }

    fn ours(lists: &[&[&'static str]]) -> BTreeSet<&'static str> {
        lists.iter().flat_map(|list| list.iter().copied()).collect()
    }

    fn same<T: for<'de> Deserialize<'de>>(what: &str, lists: &[&[&'static str]]) {
        let upstream = upstream::<T>();
        assert!(!upstream.is_empty(), "{what}: no keys were found upstream");
        let ours = ours(lists);
        let undecided: Vec<_> = upstream.difference(&ours).collect();
        let unknown: Vec<_> = ours.difference(&upstream).collect();
        assert!(
            undecided.is_empty(),
            "{what}: upstream has keys nobody decided on: {undecided:?}"
        );
        assert!(
            unknown.is_empty(),
            "{what}: a list names keys upstream doesn't have: {unknown:?}"
        );
    }

    #[test]
    fn the_snapshot_options_are_all_decided() {
        same::<SnapshotOptions>("snapshot options", &[SNAPSHOT_MAPPED, SNAPSHOT_LEFT_OUT]);
    }

    #[test]
    fn the_excludes_are_all_decided() {
        same::<Excludes>("excludes", &[EXCLUDES_MAPPED]);
    }

    #[test]
    fn the_source_filter_is_all_decided() {
        same::<LocalSourceFilterOptions>("source filter", &[FILTER_MAPPED, FILTER_LEFT_OUT]);
    }

    #[test]
    fn the_parent_and_save_options_are_all_decided() {
        same::<ParentOptions>("parent options", &[PARENT_LEFT_OUT]);
        same::<LocalSourceSaveOptions>("save options", &[SAVE_LEFT_OUT]);
    }

    #[test]
    fn the_keep_options_are_all_decided() {
        same::<KeepOptions>(
            "keep options",
            &[RETENTION_MAPPED, RETENTION_REFUSED, RETENTION_LEFT_OUT],
        );
    }

    #[test]
    fn the_repository_options_are_all_decided() {
        let upstream = upstream::<RepositoryOptions>();
        let ours = ours(&[REPOSITORY_MAPPED, REPOSITORY_LEFT_OUT]);
        let undecided: Vec<_> = upstream.difference(&ours).collect();
        assert!(
            undecided.is_empty(),
            "repository options: upstream has keys nobody decided on: {undecided:?}"
        );
    }
}
