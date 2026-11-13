//! The tables of the file, one struct for each, as serde reads and writes
//! them. They are strict: a key the format doesn't have is an error, and every
//! key is optional here, so that what a table lacks is told apart from what it
//! holds and the missing key can be named. They hold text and numbers as the
//! file does; `read` turns them into the domain's types and `write` back.

use std::collections::BTreeMap;
use std::fmt;
use std::marker::PhantomData;

use serde::de::value::MapAccessDeserializer;
use serde::de::{self, Deserializer, MapAccess, Visitor};
use serde::ser::{SerializeMap, Serializer};
use serde::{Deserialize, Serialize};

/// A table of keys and text, such as a repository's `options`.
pub(crate) type Options = BTreeMap<String, String>;

/// A struct for one table of the file, every key of it optional and left out
/// when written if it isn't there.
macro_rules! table {
    ($(#[$meta:meta])* $name:ident { $($field:ident: $ty:ty),* $(,)? }) => {
        $(#[$meta])*
        #[derive(Default, Deserialize, Serialize)]
        #[serde(rename_all = "kebab-case", deny_unknown_fields)]
        pub(crate) struct $name {
            $(
                #[serde(skip_serializing_if = "Option::is_none")]
                pub $field: Option<$ty>,
            )*
        }
    };
}

table! {
    /// The whole file.
    FileTable {
        // Read by the schema probe before this table is, and written by `render`.
        schema: i64,
        agent: AgentTable,
        hooks: HooksTable,
        repositories: Repositories,
        plans: Vec<PlanTable>,
        notifications: NotificationsTable,
    }
}

table! {
    AgentTable {
        hostname: String,
        concurrency: i64,
        nice: i64,
        allow_hub_restore: bool,
        locale: String,
    }
}

table! {
    HooksTable {
        run_before: Vec<HookValue>,
        run_after: Vec<HookValue>,
        run_failed: Vec<HookValue>,
        run_finally: Vec<HookValue>,
    }
}

table! {
    /// One repository. Exactly one of the five `password` keys is set.
    RepositoryTable {
        repository: String,
        repository_id: String,
        repo_hot: String,
        password: String,
        password_command: String,
        password_keyring: String,
        password_file: String,
        password_env: String,
        maintenance: String,
        group_by: String,
        options: Options,
        options_hot: Options,
        options_cold: Options,
        hooks: HooksTable,
        locations: Vec<LocationTable>,
    }
}

table! {
    /// One location of a repository after its first (decision 80): the keys
    /// of a location, and no other.
    LocationTable {
        repository: String,
        repo_hot: String,
        options: Options,
        options_hot: Options,
        options_cold: Options,
    }
}

table! {
    /// One plan: its own keys, the `rustic` ones it takes, and its tables.
    PlanTable {
        id: String,
        repository: String,
        host: String,
        label: String,
        sources: Vec<String>,
        tags: Vec<String>,
        globs: Vec<String>,
        iglobs: Vec<String>,
        glob_files: Vec<String>,
        iglob_files: Vec<String>,
        git_ignore: bool,
        no_require_git: bool,
        custom_ignorefiles: Vec<String>,
        exclude_if_present: Vec<String>,
        exclude_if_xattr: Vec<String>,
        one_file_system: bool,
        schedule: ScheduleTable,
        overlap: String,
        retry: RetryTable,
        hooks: HooksTable,
        retention: RetentionTable,
    }
}

table! {
    /// A schedule. Exactly one of `cron` and `every` is set.
    ScheduleTable {
        cron: String,
        every: String,
        timezone: String,
        jitter: String,
        catch_up: bool,
        on_ac_power: bool,
    }
}

table! {
    RetryTable {
        attempts: i64,
        backoff: String,
    }
}

table! {
    RetentionTable {
        keep_last: i64,
        keep_minutely: i64,
        keep_hourly: i64,
        keep_daily: i64,
        keep_weekly: i64,
        keep_monthly: i64,
        keep_quarter_yearly: i64,
        keep_half_yearly: i64,
        keep_yearly: i64,
        keep_within: String,
        keep_within_minutely: String,
        keep_within_hourly: String,
        keep_within_daily: String,
        keep_within_weekly: String,
        keep_within_monthly: String,
        keep_within_quarter_yearly: String,
        keep_within_half_yearly: String,
        keep_within_yearly: String,
    }
}

table! {
    /// A hook given as a table, for one that needs more than a command line.
    HookEntryTable {
        command: String,
        args: Vec<String>,
        on_failure: String,
    }
}

table! {
    /// `[notifications]`: the channels and the rules, each by its name.
    NotificationsTable {
        channels: BTreeMap<String, ChannelTable>,
        rules: BTreeMap<String, RuleTable>,
    }
}

table! {
    /// One channel. Exactly one of the four `url` keys is set; a channel
    /// that signs in sets one `token` key, or `username` and one `password`
    /// key. No key holds a secret as text (I4).
    ChannelTable {
        kind: String,
        url_command: String,
        url_keyring: String,
        url_file: String,
        url_env: String,
        token_command: String,
        token_keyring: String,
        token_file: String,
        token_env: String,
        username: String,
        password_command: String,
        password_keyring: String,
        password_file: String,
        password_env: String,
    }
}

table! {
    /// One rule: the events it sends, its channel, the plans it is for and
    /// its template.
    RuleTable {
        events: Vec<String>,
        channel: String,
        plans: Vec<String>,
        template: String,
    }
}

/// A hook: a command line as one string, or a table.
#[derive(Serialize)]
#[serde(untagged)]
pub(crate) enum HookValue {
    Text(String),
    Table(HookEntryTable),
}

impl<'de> Deserialize<'de> for HookValue {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Hook;

        impl<'de> Visitor<'de> for Hook {
            type Value = HookValue;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a command line or a table")
            }

            fn visit_str<E: de::Error>(self, text: &str) -> Result<HookValue, E> {
                Ok(HookValue::Text(text.to_owned()))
            }

            fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<HookValue, A::Error> {
                HookEntryTable::deserialize(MapAccessDeserializer::new(map)).map(HookValue::Table)
            }
        }

        deserializer.deserialize_any(Hook)
    }
}

/// The `[repositories]` table: each name with its table, in the order of the
/// file, so that the first mistake reported is the first one in it.
#[derive(Default)]
pub(crate) struct Repositories(pub Vec<(String, RepositoryTable)>);

impl<'de> Deserialize<'de> for Repositories {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Named<T>(PhantomData<T>);

        impl<'de> Visitor<'de> for Named<RepositoryTable> {
            type Value = Repositories;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a table of repositories")
            }

            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Repositories, A::Error> {
                let mut all = Vec::new();
                while let Some(entry) = map.next_entry::<String, RepositoryTable>()? {
                    all.push(entry);
                }
                Ok(Repositories(all))
            }
        }

        deserializer.deserialize_map(Named(PhantomData))
    }
}

impl Serialize for Repositories {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(self.0.len()))?;
        for (alias, table) in &self.0 {
            map.serialize_entry(alias, table)?;
        }
        map.end()
    }
}
