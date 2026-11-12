//! Parity with the pinned `rustic_core` (technical design §6.1): every key of
//! each struct the file's settings map onto is either one the file takes or one
//! it deliberately doesn't, and the file really does take the first and refuse
//! the second. A pin upgrade that adds a key fails here until someone decides.

use std::collections::BTreeSet;
use std::path::Path;

use app::ports::{ConfigError, Problem};
use domain::planning::GroupBy;
use rustic_core::{
    Excludes, KeepOptions, LocalSourceFilterOptions, SnapshotGroupCriterion, SnapshotOptions,
};
use serde::de::{self, Deserializer, Visitor};

use crate::keys::{
    EXCLUDES_MAPPED, EXCLUDES_UNSUPPORTED, FILTER_MAPPED, FILTER_UNSUPPORTED, RETENTION_MAPPED,
    RETENTION_UNSUPPORTED, SNAPSHOT_MAPPED, SNAPSHOT_UNSUPPORTED,
};
use crate::schema::{Schemas, parse, render};

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
fn keys_of<T: de::DeserializeOwned>() -> Vec<&'static str> {
    let mut keys = Vec::new();
    let _ = T::deserialize(Recorder(&mut keys));
    keys.sort_unstable();
    keys
}

/// Every key upstream has is in exactly one of the two lists, and the lists
/// name nothing upstream doesn't have.
fn assert_all_decided(upstream: &[&str], mapped: &[&str], unsupported: &[&str], what: &str) {
    assert!(!upstream.is_empty(), "{what}: no keys were found upstream");
    let mapped_set: BTreeSet<_> = mapped.iter().copied().collect();
    let unsupported_set: BTreeSet<_> = unsupported.iter().copied().collect();
    let both: Vec<_> = mapped_set.intersection(&unsupported_set).collect();
    assert!(both.is_empty(), "{what}: in both lists: {both:?}");
    let decided: BTreeSet<_> = mapped_set.union(&unsupported_set).copied().collect();
    let upstream_set: BTreeSet<_> = upstream.iter().copied().collect();
    let undecided: Vec<_> = upstream_set.difference(&decided).collect();
    let unknown: Vec<_> = decided.difference(&upstream_set).collect();
    assert!(
        undecided.is_empty(),
        "{what}: upstream has keys nobody decided on: {undecided:?}"
    );
    assert!(
        unknown.is_empty(),
        "{what}: a list names keys upstream doesn't have: {unknown:?}"
    );
}

const FILE: &str = "/home/demo/.config/ladderback/config.toml";

fn parsed(text: &str) -> Result<crate::schema::Parsed, ConfigError> {
    parse(Path::new(FILE), text, &Schemas::production())
}

/// A file with one repository and one plan, and `plan_extra` after the plan's
/// own keys: more keys of the plan, or a table of its own.
fn file_with(plan_extra: &str) -> String {
    format!(
        "schema = 1\n\n[repositories.nas]\nrepository = \"/srv/demo-repo\"\n\
         password-env = \"DEMO_PASSWORD\"\n\n[[plans]]\nid = \"home\"\nrepository = \"nas\"\n\
         sources = [\"/home/demo\"]\n{plan_extra}"
    )
}

/// A value of the type the key takes, for a key the file takes.
fn sample(key: &str) -> &'static str {
    match key {
        "keep-last"
        | "keep-minutely"
        | "keep-hourly"
        | "keep-daily"
        | "keep-weekly"
        | "keep-monthly"
        | "keep-quarter-yearly"
        | "keep-half-yearly"
        | "keep-yearly" => "1",
        "git-ignore" | "no-require-git" | "one-file-system" => "true",
        "label" => "\"demo\"",
        "host" => "\"demo-host\"",
        key if key.starts_with("keep-within") => "\"1 day\"",
        "glob-files" | "iglob-files" => "[\"/home/demo/.excludes\"]",
        _ => "[\"demo\"]",
    }
}

/// Where a key sits in the file: `[plans.retention]` for what `KeepOptions`
/// has, and the plan itself for the rest.
fn placed(table: Option<&str>, key: &str, value: &str) -> String {
    match table {
        Some(table) => file_with(&format!("\n[{table}]\n{key} = {value}\n")),
        None => file_with(&format!("{key} = {value}\n")),
    }
}

/// The file takes what the lists say it takes and refuses the rest, whatever
/// the value: that is what "unsupported" means to a user.
fn assert_the_file_follows(upstream: &[&str], mapped: &[&str], table: Option<&str>, what: &str) {
    for key in upstream {
        if mapped.contains(key) {
            let text = placed(table, key, sample(key));
            let result = parsed(&text);
            assert!(result.is_ok(), "{what}: {key} is mapped, got {result:?}");
        } else {
            let text = placed(table, key, "1");
            match parsed(&text) {
                Err(ConfigError::Invalid {
                    problem: Problem::UnknownKey,
                    key: at,
                    ..
                }) => {
                    let tail = at.as_deref().and_then(|at| at.rsplit('.').next());
                    assert_eq!(tail, Some(*key), "{what}");
                }
                other => panic!("{what}: {key} is unsupported, yet the file says {other:?}"),
            }
        }
    }
}

#[test]
fn every_keep_option_is_mapped_or_left_out_on_purpose() {
    assert_all_decided(
        &keys_of::<KeepOptions>(),
        RETENTION_MAPPED,
        RETENTION_UNSUPPORTED,
        "KeepOptions",
    );
}

#[test]
fn every_exclude_is_mapped_or_left_out_on_purpose() {
    assert_all_decided(
        &keys_of::<Excludes>(),
        EXCLUDES_MAPPED,
        EXCLUDES_UNSUPPORTED,
        "Excludes",
    );
}

#[test]
fn every_source_filter_option_is_mapped_or_left_out_on_purpose() {
    assert_all_decided(
        &keys_of::<LocalSourceFilterOptions>(),
        FILTER_MAPPED,
        FILTER_UNSUPPORTED,
        "LocalSourceFilterOptions",
    );
}

#[test]
fn every_snapshot_option_is_mapped_or_left_out_on_purpose() {
    assert_all_decided(
        &keys_of::<SnapshotOptions>(),
        SNAPSHOT_MAPPED,
        SNAPSHOT_UNSUPPORTED,
        "SnapshotOptions",
    );
}

/// The keys of every list, sorted.
fn listed(lists: &[&[&'static str]]) -> Vec<&'static str> {
    let mut keys: Vec<_> = lists.iter().flat_map(|list| list.iter().copied()).collect();
    keys.sort_unstable();
    keys
}

#[test]
fn the_retention_table_has_the_keys_the_list_says_and_no_others() {
    assert_eq!(
        keys_of::<crate::tables::RetentionTable>(),
        listed(&[RETENTION_MAPPED])
    );
}

#[test]
fn a_plans_table_has_its_own_keys_and_the_mapped_ones_and_no_others() {
    let own: &[&str] = &[
        "id",
        "repository",
        "sources",
        "schedule",
        "overlap",
        "retry",
        "hooks",
        "retention",
    ];
    assert_eq!(
        keys_of::<crate::tables::PlanTable>(),
        listed(&[own, EXCLUDES_MAPPED, FILTER_MAPPED, SNAPSHOT_MAPPED])
    );
}

#[test]
fn the_file_takes_the_keep_options_it_maps_and_refuses_the_others() {
    assert_the_file_follows(
        &keys_of::<KeepOptions>(),
        RETENTION_MAPPED,
        Some("plans.retention"),
        "KeepOptions",
    );
}

#[test]
fn the_file_takes_the_excludes_it_maps_and_refuses_the_others() {
    assert_the_file_follows(&keys_of::<Excludes>(), EXCLUDES_MAPPED, None, "Excludes");
}

#[test]
fn the_file_takes_the_source_filters_it_maps_and_refuses_the_others() {
    assert_the_file_follows(
        &keys_of::<LocalSourceFilterOptions>(),
        FILTER_MAPPED,
        None,
        "LocalSourceFilterOptions",
    );
}

#[test]
fn the_file_takes_the_snapshot_options_it_maps_and_refuses_the_others() {
    assert_the_file_follows(
        &keys_of::<SnapshotOptions>(),
        SNAPSHOT_MAPPED,
        None,
        "SnapshotOptions",
    );
}

/// The group-by criteria upstream, by the four flags it has, and the
/// `GroupBy` that has the same four.
fn criterion_and_group_by(mask: u8) -> (SnapshotGroupCriterion, GroupBy) {
    let flag = |bit: u8| mask & (1 << bit) != 0;
    let criterion = SnapshotGroupCriterion::new()
        .hostname(flag(0))
        .label(flag(1))
        .paths(flag(2))
        .tags(flag(3));
    let group_by = GroupBy {
        host: flag(0),
        label: flag(1),
        paths: flag(2),
        tags: flag(3),
    };
    (criterion, group_by)
}

fn group_by_of(text: &str) -> Result<GroupBy, ConfigError> {
    let file = file_with("");
    let with_group_by = file.replace(
        "password-env = \"DEMO_PASSWORD\"\n",
        &format!("password-env = \"DEMO_PASSWORD\"\ngroup-by = \"{text}\"\n"),
    );
    let parsed = parsed(&with_group_by)?;
    parsed
        .config
        .repositories()
        .values()
        .next()
        .map(|repository| repository.group_by)
        .ok_or(ConfigError::Unavailable {
            file: FILE.to_owned(),
            detail: "no repository".to_owned(),
        })
}

#[test]
fn group_by_is_read_as_rustic_reads_it_for_every_combination() {
    for mask in 0u8..16 {
        let (criterion, expected) = criterion_and_group_by(mask);
        let text = criterion.to_string();
        let read = group_by_of(&text);
        assert!(read.is_ok(), "{text:?}: {read:?}");
        assert_eq!(read.ok(), Some(expected), "{text:?}");
    }
}

#[test]
fn group_by_is_written_as_rustic_writes_it_for_every_combination() {
    for mask in 0u8..16 {
        let (criterion, group_by) = criterion_and_group_by(mask);
        let config = config_grouped_by(group_by);
        let text = render(Path::new(FILE), &config);
        assert!(text.is_ok(), "{mask}: {text:?}");
        let table: Result<toml::Table, _> = text.unwrap_or_default().parse();
        let written = table.ok().and_then(|table| {
            table
                .get("repositories")?
                .get("nas")?
                .get("group-by")
                .cloned()
        });
        if group_by == GroupBy::default() {
            assert_eq!(written, None, "the default is left out");
            assert_eq!(criterion.to_string(), "host,label,paths");
        } else {
            assert_eq!(
                written,
                Some(toml::Value::String(criterion.to_string())),
                "{mask}"
            );
        }
    }
}

#[test]
fn what_rustic_refuses_in_group_by_the_file_refuses_and_what_it_takes_the_file_takes() {
    for text in [
        "",
        ",",
        "host,,tags",
        "tags,host",
        "label,host,label",
        "host,colour",
        "Host",
        " host",
        "hostname",
        "host;label",
    ] {
        let upstream = text.parse::<SnapshotGroupCriterion>();
        let ours = group_by_of(text);
        assert_eq!(
            upstream.is_ok(),
            ours.is_ok(),
            "{text:?}: rustic says {upstream:?}, the file says {ours:?}"
        );
    }
}

fn config_grouped_by(group_by: GroupBy) -> domain::planning::AgentConfig {
    use std::collections::BTreeMap;

    use domain::RepositoryAlias;
    use domain::planning::{
        AgentConfig, AgentSettings, Hooks, Maintenance, RepositoryConfig, RepositorySettings,
    };
    use domain::secrets::SecretRef;

    let repository = RepositoryConfig {
        id: None,
        settings: RepositorySettings {
            repository: "/srv/demo-repo".to_owned(),
            repo_hot: None,
            password: SecretRef::Env("DEMO_PASSWORD".to_owned()),
            options: BTreeMap::new(),
            options_hot: BTreeMap::new(),
            options_cold: BTreeMap::new(),
        },
        maintenance: Maintenance::None,
        group_by,
        hooks: Hooks::default(),
    };
    let alias = RepositoryAlias::parse("nas");
    AgentConfig::new(
        AgentSettings::default(),
        Hooks::default(),
        alias
            .into_iter()
            .map(|alias| (alias, repository.clone()))
            .collect(),
        Vec::new(),
    )
    .unwrap_or_default()
}
