//! The tables of a profile as serde reads them: only the keys the import
//! carries over and the one it must see to refuse, every one of them
//! optional. A key rustic doesn't know is refused before these are read
//! (`parse`), and a key the import leaves out is not read at all, so they
//! are not here.

use std::collections::BTreeMap;
use std::fmt;

use serde::Deserialize;
use serde::de::value::MapAccessDeserializer;
use serde::de::{self, Deserializer, MapAccess, Visitor};

/// A table of keys and text, such as a repository's `options`.
pub(crate) type Options = BTreeMap<String, String>;

#[derive(Default, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) struct FileTable {
    pub global: Option<GlobalTable>,
    pub repository: Option<RepositoryTable>,
    pub backup: Option<BackupTable>,
    pub forget: Option<ForgetTable>,
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) struct GlobalTable {
    pub profile_substitute_env: Option<bool>,
    pub use_profiles: Option<Vec<String>>,
    pub group_by: Option<String>,
    pub hooks: Option<HooksTable>,
    pub env: Option<Options>,
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) struct HooksTable {
    pub run_before: Option<Vec<CommandValue>>,
    pub run_after: Option<Vec<CommandValue>>,
    pub run_failed: Option<Vec<CommandValue>>,
    pub run_finally: Option<Vec<CommandValue>>,
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) struct RepositoryTable {
    pub repository: Option<String>,
    pub repo_hot: Option<String>,
    pub options: Option<Options>,
    pub options_hot: Option<Options>,
    pub options_cold: Option<Options>,
    pub password: Option<String>,
    pub password_file: Option<String>,
    pub password_command: Option<CommandValue>,
    pub hooks: Option<HooksTable>,
}

/// `[backup]` and each `[[backup.snapshots]]` entry, which are one struct in
/// rustic.
#[derive(Default, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) struct BackupTable {
    pub name: Option<String>,
    pub sources: Option<Vec<String>>,
    pub label: Option<String>,
    pub host: Option<String>,
    pub tags: Option<Vec<String>>,
    pub globs: Option<Vec<String>>,
    pub iglobs: Option<Vec<String>>,
    pub glob_files: Option<Vec<String>>,
    pub iglob_files: Option<Vec<String>>,
    pub git_ignore: Option<bool>,
    pub no_require_git: Option<bool>,
    pub custom_ignorefiles: Option<Vec<String>>,
    pub exclude_if_present: Option<Vec<String>>,
    pub exclude_if_xattr: Option<Vec<String>>,
    pub one_file_system: Option<bool>,
    pub hooks: Option<HooksTable>,
    pub snapshots: Option<Vec<BackupTable>>,
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) struct ForgetTable {
    pub group_by: Option<String>,
    pub prune: Option<bool>,
    pub keep_tags: Option<Vec<String>>,
    pub keep_ids: Option<Vec<String>>,
    pub keep_last: Option<i64>,
    pub keep_minutely: Option<i64>,
    pub keep_hourly: Option<i64>,
    pub keep_daily: Option<i64>,
    pub keep_weekly: Option<i64>,
    pub keep_monthly: Option<i64>,
    pub keep_quarter_yearly: Option<i64>,
    pub keep_half_yearly: Option<i64>,
    pub keep_yearly: Option<i64>,
    pub keep_within: Option<String>,
    pub keep_within_minutely: Option<String>,
    pub keep_within_hourly: Option<String>,
    pub keep_within_daily: Option<String>,
    pub keep_within_weekly: Option<String>,
    pub keep_within_monthly: Option<String>,
    pub keep_within_quarter_yearly: Option<String>,
    pub keep_within_half_yearly: Option<String>,
    pub keep_within_yearly: Option<String>,
}

/// A command rustic runs: a command line as one string, or a table of the
/// program, its arguments and what a failure means. `CommandInput` in
/// `rustic_core` reads both.
pub(crate) enum CommandValue {
    Text(String),
    Table(CommandTable),
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) struct CommandTable {
    pub command: Option<String>,
    pub args: Option<Vec<String>>,
    pub on_failure: Option<String>,
}

impl<'de> Deserialize<'de> for CommandValue {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Command;

        impl<'de> Visitor<'de> for Command {
            type Value = CommandValue;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a command line or a table")
            }

            fn visit_str<E: de::Error>(self, text: &str) -> Result<CommandValue, E> {
                Ok(CommandValue::Text(text.to_owned()))
            }

            fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<CommandValue, A::Error> {
                CommandTable::deserialize(MapAccessDeserializer::new(map)).map(CommandValue::Table)
            }
        }

        deserializer.deserialize_any(Command)
    }
}
