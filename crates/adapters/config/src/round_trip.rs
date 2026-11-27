//! The round trip of `config.toml` as properties (technical design §6.1):
//! any configuration Ladderback writes reads back as the configuration it
//! wrote, and writing that again gives the same text; a file it wrote,
//! edited in place to hold another configuration, reads back as that one;
//! and a secret the format can't say is refused at its key, never written
//! wrong. An edit is drawn with the plans of both in the same order: a save
//! that reorders plans isn't made in place, and is written whole instead.
//!
//! The configurations are drawn over every table of the file: `[agent]`,
//! `[hooks]`, repositories with their further locations and the rules of
//! each, plans with schedules, retries, filters, retention and hooks, and
//! notification channels and rules. Free text (labels, globs, options,
//! arguments, network names, environment variables) is drawn from what TOML
//! has to quote or escape, control characters and text that isn't ASCII, and
//! a plan's label is as often empty as missing.
//!
//! Each value is made by the domain's own constructor. The reader holds a
//! file to some rules the domain's types don't, and the app checks them
//! before it saves; the configurations drawn here keep them, and each
//! refusal is pinned by an example of its own (`tests/parse.rs`,
//! `tests/locations.rs`, `tests/notifications.rs`): no setting of a
//! repository names a path on this machine by a relative path, no password
//! or channel secret file is a relative path, a repository with further
//! locations records its ID, and no location of an entry repeats another.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use app::ports::{ConfigError, Problem};
use domain::execution::plan_tags::UserTag;
use domain::notifications::{
    Auth, Channel, ChannelKind, ChannelName, EventKind, Notifications, Rule, RuleName, Template,
    Username,
};
use domain::planning::HubCommands;
use domain::planning::{
    AgentConfig, AgentSettings, Alternative, Cadence, Concurrency, CronExpression, DryRunAllowlist,
    DryRunFolder, GlobFile, GroupBy, HookEntry, Hooks, Hostname, Interval, KeepCount, KeepWithin,
    Locale, Location, LocationRule, Locations, MAX_DURATION, MAX_LOCATIONS, MAX_NETWORK_NAMES,
    MAX_RULE_ALTERNATIVES, MIN_INTERVAL, Maintenance, NameFilter, NetworkKind, NetworkName, Nice,
    OnFailure, Overlap, Plan, RepositoryConfig, RestoreFolder, RetentionPolicy, RetryPolicy,
    Schedule, ServeSettings, Source, SourceFilter, UpdateCheck,
};
use domain::secrets::{CommandLine, FileRead, KeyringEntry, SecretRef, SecretValue};
use domain::{PlanId, RepositoryAlias, RepositoryId};
use jiff::{SignedDuration, Span};
use proptest::collection::{btree_set, vec};
use proptest::option::weighted;
use proptest::prelude::*;
use proptest::sample::{Index, select};
use proptest::test_runner::{Config, RngSeed};

use crate::edit::saved;
use crate::schema::{Schemas, parse, render};
use crate::zone::schedule_zone;

/// The seed every property here draws its cases from, as the domain's do.
const SEED: u64 = 266;

/// proptest's 256 cases from one fixed seed: every run, here or in CI, tries
/// the same cases, so a property fails on every run or on none. A failure is
/// still saved under the crate's `proptest-regressions/` and tried first
/// from then on.
fn config() -> Config {
    Config {
        rng_seed: RngSeed::Fixed(SEED),
        ..Config::default()
    }
}

fn file() -> &'static Path {
    Path::new("/home/demo/.config/ladderback/config.toml")
}

/// Characters TOML has to quote or escape in a string or a key: quotes,
/// backslashes, the punctuation of its syntax and a shell's, control
/// characters (NUL, BEL, ESC, DEL and the C1 control NEL among them), white
/// space and line breaks of several kinds; and text that isn't ASCII:
/// combining, zero-width, right-to-left, outside the basic plane, the byte
/// order mark and the last scalar value.
const AWKWARD: &[char] = &[
    ' ',
    '\t',
    '\n',
    '\r',
    '\0',
    '\u{1}',
    '\u{7}',
    '\u{8}',
    '\u{c}',
    '\u{1b}',
    '\u{1f}',
    '\u{7f}',
    '\u{85}',
    '\u{a0}',
    '"',
    '\'',
    '\\',
    '#',
    '=',
    '[',
    ']',
    '{',
    '}',
    ',',
    '.',
    ':',
    '/',
    '$',
    '~',
    '*',
    '?',
    '!',
    '%',
    '&',
    '|',
    ';',
    '<',
    '>',
    '(',
    ')',
    '`',
    '@',
    '+',
    '-',
    '_',
    'é',
    'ß',
    '日',
    '🦀',
    '\u{301}',
    '\u{200b}',
    '\u{202e}',
    '\u{2028}',
    '\u{feff}',
    '\u{10ffff}',
];

/// Pieces of TOML's syntax, or a shell's, that a writer could take for its
/// own: string delimiters, escapes written out, a table header, a comment,
/// and the references a repository's settings may hold.
const SYNTAX: &[&str] = &[
    "\"\"\"",
    "'''",
    "\r\n",
    "\\u0041",
    "\\n",
    "a = 1",
    "[x]",
    "# note",
    "''",
    "\"\"",
    "${HOME}",
    "$${HOME}",
    "${file:/run/secret}",
];

/// Free text, maybe empty: up to six chunks, each a character of
/// [`AWKWARD`], any character at all, a run of letters and digits, or a
/// piece of [`SYNTAX`].
fn text() -> impl Strategy<Value = String> {
    let chunk = prop_oneof![
        4 => select(AWKWARD).prop_map(String::from),
        2 => "[A-Za-z0-9]{1,8}",
        1 => any::<char>().prop_map(String::from),
        1 => select(SYNTAX).prop_map(str::to_owned),
    ];
    vec(chunk, 0..=6).prop_map(|chunks| chunks.concat())
}

/// [`text`] without control characters, nor any of `refused`.
fn plain(refused: &'static [char]) -> impl Strategy<Value = String> {
    text().prop_map(move |text| {
        text.chars()
            .filter(|c| !c.is_control() && !refused.contains(c))
            .collect()
    })
}

/// An absolute path, which may hold anything after its first `/`.
fn absolute() -> impl Strategy<Value = String> {
    text().prop_map(|text| format!("/{text}"))
}

/// A slug, the form of a plan's ID, an alias and a channel's or rule's name:
/// short ones, ones of digits alone, and ones of the most bytes a slug has.
fn slug() -> impl Strategy<Value = String> {
    prop_oneof![
        8 => "[a-z0-9][a-z0-9-]{0,11}",
        1 => "[0-9]{1,6}",
        1 => "[a-z0-9][a-z0-9-]{63}",
    ]
}

/// `count` slugs, all different, in order.
fn slugs(count: usize) -> impl Strategy<Value = Vec<String>> {
    btree_set(slug(), count).prop_map(|slugs| slugs.into_iter().collect())
}

/// A program a hook or a command secret may start: a name looked up on
/// `PATH`, which may hold anything but a `/`, or an absolute path.
fn command() -> impl Strategy<Value = CommandLine> {
    let program = prop_oneof![
        2 => "[a-z][a-z0-9._-]{0,8}",
        1 => text().prop_map(|text| text.replace('/', "")),
        1 => absolute(),
    ];
    (program, vec(text(), 0..=3)).prop_filter_map("a program with a name", |(program, args)| {
        HookEntry::new(&program, args, OnFailure::Error)
            .ok()
            .map(|entry| entry.command)
    })
}

fn hook() -> impl Strategy<Value = HookEntry> {
    let on_failure = select(vec![OnFailure::Error, OnFailure::Warn, OnFailure::Ignore]);
    (command(), on_failure).prop_map(|(command, on_failure)| HookEntry {
        command,
        on_failure,
    })
}

fn hooks() -> impl Strategy<Value = Hooks> {
    let list = || vec(hook(), 0..=2);
    (list(), list(), list(), list()).prop_map(|(run_before, run_after, run_failed, run_finally)| {
        Hooks {
            run_before,
            run_after,
            run_failed,
            run_finally,
        }
    })
}

fn keyring() -> impl Strategy<Value = KeyringEntry> {
    (text(), text()).prop_filter_map("a service and a name", |(service, name)| {
        KeyringEntry::parse(&format!("{}/{name}", service.replace('/', ""))).ok()
    })
}

/// A secret one of the keys `-command`, `-keyring`, `-file` and `-env`
/// names: every source but text in the file and a whole file.
fn named_secret() -> impl Strategy<Value = SecretRef> {
    prop_oneof![
        command().prop_map(SecretRef::Command),
        keyring().prop_map(SecretRef::Keyring),
        absolute().prop_map(|path| SecretRef::File {
            path,
            read: FileRead::FirstLine,
        }),
        text().prop_map(SecretRef::Env),
    ]
}

/// A repository's password: a [`named_secret`], or the password itself.
fn password() -> impl Strategy<Value = SecretRef> {
    prop_oneof![
        3 => named_secret(),
        1 => text().prop_map(|text| SecretRef::Inline(SecretValue::new(text))),
    ]
}

fn agent() -> impl Strategy<Value = AgentSettings> {
    let hostname = plain(&[]).prop_filter_map("a host name", |text| Hostname::parse(&text).ok());
    let concurrency = prop_oneof![Just(1), Just(i64::from(Concurrency::MAX)), 1..=64_i64];
    let maintenance_concurrency =
        prop_oneof![Just(1), Just(i64::from(Concurrency::MAX)), 1..=64_i64];
    let nice = 0..=i64::from(Nice::MAX);
    let locale = "[a-zA-Z]{1,8}(-[a-zA-Z0-9]{1,8}){0,3}"
        .prop_filter_map("a locale", |text| Locale::parse(&text).ok());
    let restore_folder = absolute().prop_filter_map("a path without NUL", |path| {
        RestoreFolder::parse(&path).ok()
    });
    let dry_run_allowlist = vec(
        absolute().prop_filter_map("a path without NUL", |path| DryRunFolder::parse(&path).ok()),
        0..=3,
    )
    .prop_map(|folders| DryRunAllowlist::new(folders).expect("under the limit"));
    (
        weighted(0.5, hostname),
        concurrency,
        maintenance_concurrency,
        weighted(0.5, nice),
        any::<bool>(),
        prop_oneof![Just(HubCommands::On), Just(HubCommands::Off)],
        weighted(0.5, locale),
        prop_oneof![Just(UpdateCheck::On), Just(UpdateCheck::Off)],
        weighted(0.5, restore_folder),
        dry_run_allowlist,
    )
        .prop_map(
            |(
                hostname,
                concurrency,
                maintenance_concurrency,
                nice,
                allow_hub_restore,
                hub_commands,
                locale,
                update_check,
                restore_folder,
                dry_run_allowlist,
            )| {
                AgentSettings {
                    hostname,
                    concurrency: Concurrency::new(concurrency).expect("drawn in its range"),
                    maintenance_concurrency: Concurrency::new(maintenance_concurrency)
                        .expect("drawn in its range"),
                    nice: nice.map(|nice| Nice::new(nice).expect("drawn in its range")),
                    allow_hub_restore,
                    hub_commands,
                    locale,
                    update_check,
                    restore_folder,
                    dry_run_allowlist,
                }
            },
        )
}

/// A length of time from `min` to `max`, both whole seconds: one of them,
/// whole seconds, milliseconds or nanoseconds.
fn duration(min: SignedDuration, max: SignedDuration) -> impl Strategy<Value = SignedDuration> {
    let (low, high) = (min.as_secs(), max.as_secs());
    prop_oneof![
        1 => Just(min),
        1 => Just(max),
        3 => (low..=high).prop_map(SignedDuration::from_secs),
        2 => (low..high, 0..1_000_i32)
            .prop_map(|(secs, millis)| SignedDuration::new(secs, millis * 1_000_000)),
        2 => (low..high, 0..1_000_000_000_i32).prop_map(|(secs, nanos)| SignedDuration::new(secs, nanos)),
    ]
}

/// A field of a cron expression from `min` to `max`.
fn cron_field(min: u32, max: u32) -> impl Strategy<Value = String> {
    prop_oneof![
        Just("*".to_owned()),
        (min..=max).prop_map(|n| n.to_string()),
        (1..=max).prop_map(|step| format!("*/{step}")),
        (min..=max, min..=max).prop_map(|(a, b)| format!("{}-{}", a.min(b), a.max(b))),
        (min..=max, min..=max).prop_map(|(a, b)| format!("{a},{b}")),
    ]
}

fn cron() -> impl Strategy<Value = CronExpression> {
    let fields = (
        cron_field(0, 59),
        cron_field(0, 23),
        cron_field(1, 31),
        cron_field(1, 12),
        cron_field(0, 6),
    )
        .prop_map(|(minute, hour, day, month, weekday)| {
            format!("{minute} {hour} {day} {month} {weekday}")
        });
    let written = select(vec![
        "0 0 29 2 *",
        "30 2 * * MON-FRI",
        "0  3 * * *",
        "0 3 * jan,jul sun",
    ])
    .prop_map(str::to_owned);
    prop_oneof![3 => fields, 1 => written]
        .prop_filter_map("an expression with a next moment", |text| {
            CronExpression::parse(&text).ok()
        })
}

/// Zones the system's database and the one built in both have: UTC, ones
/// with a daylight saving time, ones off the hour, and a fixed offset by its
/// name.
const ZONES: &[&str] = &[
    "UTC",
    "Europe/Berlin",
    "America/New_York",
    "America/Argentina/Buenos_Aires",
    "Asia/Kolkata",
    "Australia/Lord_Howe",
    "Pacific/Chatham",
    "Etc/GMT+5",
];

fn schedule() -> impl Strategy<Value = Schedule> {
    let cadence = prop_oneof![
        cron().prop_map(Cadence::Cron),
        duration(MIN_INTERVAL, MAX_DURATION)
            .prop_map(|every| Cadence::Every(Interval::new(every).expect("drawn in its range"))),
    ];
    // The reader looks a zone up in the system's database (decision 72).
    let zone = select(ZONES)
        .prop_map(|name| schedule_zone(name, jiff::tz::db()).expect("a zone every database has"));
    (
        cadence,
        weighted(0.5, zone),
        weighted(0.5, duration(SignedDuration::ZERO, MAX_DURATION)),
        any::<bool>(),
        any::<bool>(),
    )
        .prop_map(|(cadence, zone, jitter, catch_up, on_ac_power)| {
            let mut schedule = Schedule::new(cadence)
                .with_catch_up(catch_up)
                .with_on_ac_power(on_ac_power);
            if let Some(zone) = zone {
                schedule = schedule.with_zone(zone);
            }
            if let Some(jitter) = jitter {
                schedule = schedule.with_jitter(jitter).expect("drawn in its range");
            }
            schedule
        })
}

fn retry() -> impl Strategy<Value = RetryPolicy> {
    let most = i64::from(RetryPolicy::MAX_ATTEMPTS);
    let attempts = prop_oneof![Just(1), Just(most), 1..=most];
    let backoff = duration(RetryPolicy::MIN_BACKOFF, RetryPolicy::MAX_BACKOFF);
    (attempts, backoff).prop_map(|(attempts, backoff)| {
        RetryPolicy::new(attempts, backoff).expect("drawn in its range")
    })
}

fn keep_count() -> impl Strategy<Value = KeepCount> {
    prop_oneof![Just(-1), Just(0), Just(i64::from(i32::MAX)), 1..=50_i64]
        .prop_map(|count| KeepCount::new(count).expect("drawn in its range"))
}

/// A span of calendar time with any of its units, read from `jiff`'s
/// compact form or from ISO 8601.
fn keep_within() -> impl Strategy<Value = KeepWithin> {
    (
        (0..=3_i64, 0..=24_i64, 0..=10_i64, 0..=400_i64),
        (0..=48_i64, 0..=120_i64, 0..=120_i64),
        (0..=999_i64, 0..=999_i64, 0..=999_i64),
        any::<bool>(),
    )
        .prop_map(
            |((years, months, weeks, days), (hours, minutes, seconds), (ms, us, ns), iso)| {
                let span = Span::new()
                    .years(years)
                    .months(months)
                    .weeks(weeks)
                    .days(days)
                    .hours(hours)
                    .minutes(minutes)
                    .seconds(seconds)
                    .milliseconds(ms)
                    .microseconds(us)
                    .nanoseconds(ns);
                let text = if iso {
                    span.to_string()
                } else {
                    format!("{span:#}")
                };
                KeepWithin::parse(&text).expect("a span that isn't negative")
            },
        )
}

fn retention() -> impl Strategy<Value = RetentionPolicy> {
    let counts = prop::array::uniform9(weighted(0.2, keep_count()));
    let withins = prop::array::uniform9(weighted(0.2, keep_within()));
    (counts, withins).prop_map(|(counts, withins)| {
        let [
            keep_last,
            keep_minutely,
            keep_hourly,
            keep_daily,
            keep_weekly,
            keep_monthly,
            keep_quarter_yearly,
            keep_half_yearly,
            keep_yearly,
        ] = counts;
        let [
            keep_within,
            keep_within_minutely,
            keep_within_hourly,
            keep_within_daily,
            keep_within_weekly,
            keep_within_monthly,
            keep_within_quarter_yearly,
            keep_within_half_yearly,
            keep_within_yearly,
        ] = withins;
        RetentionPolicy {
            keep_last,
            keep_minutely,
            keep_hourly,
            keep_daily,
            keep_weekly,
            keep_monthly,
            keep_quarter_yearly,
            keep_half_yearly,
            keep_yearly,
            keep_within,
            keep_within_minutely,
            keep_within_hourly,
            keep_within_daily,
            keep_within_weekly,
            keep_within_monthly,
            keep_within_quarter_yearly,
            keep_within_half_yearly,
            keep_within_yearly,
        }
    })
}

fn filter() -> impl Strategy<Value = SourceFilter> {
    let texts = || vec(text(), 0..=2);
    let glob_files = || {
        vec(
            absolute().prop_filter_map("a path without NUL", |path| GlobFile::parse(&path).ok()),
            0..=2,
        )
    };
    (
        (texts(), texts(), glob_files(), glob_files()),
        (any::<bool>(), any::<bool>(), any::<bool>()),
        (texts(), texts(), texts()),
    )
        .prop_map(
            |(
                (globs, iglobs, glob_files, iglob_files),
                (git_ignore, no_require_git, one_file_system),
                (custom_ignorefiles, exclude_if_present, exclude_if_xattr),
            )| SourceFilter {
                globs,
                iglobs,
                glob_files,
                iglob_files,
                git_ignore,
                no_require_git,
                custom_ignorefiles,
                exclude_if_present,
                exclude_if_xattr,
                one_file_system,
            },
        )
}

/// A plan, but for its ID and the repository it backs up into, which is
/// one of those drawn beside it.
#[derive(Clone, Debug)]
struct DrawnPlan {
    repository: Index,
    host: Option<Hostname>,
    label: Option<String>,
    sources: Vec<Source>,
    tags: Vec<UserTag>,
    schedule: Option<Schedule>,
    overlap: Overlap,
    retry: Option<RetryPolicy>,
    filter: SourceFilter,
    retention: RetentionPolicy,
    hooks: Hooks,
}

impl DrawnPlan {
    fn plan(self, id: PlanId, repositories: &[RepositoryAlias]) -> Plan {
        Plan {
            id,
            repository: self.repository.get(repositories).clone(),
            host: self.host,
            label: self.label,
            sources: self.sources,
            tags: self.tags,
            schedule: self.schedule,
            overlap: self.overlap,
            retry: self.retry,
            filter: self.filter,
            retention: self.retention,
            hooks: self.hooks,
        }
    }
}

fn plan() -> impl Strategy<Value = DrawnPlan> {
    let host = plain(&[]).prop_filter_map("a host name", |text| Hostname::parse(&text).ok());
    let label = prop_oneof![
        1 => Just(None),
        1 => Just(Some(String::new())),
        2 => text().prop_map(Some),
    ];
    let source = absolute().prop_filter_map("a path without NUL", |path| Source::parse(&path).ok());
    let tag = text().prop_filter_map("a tag that isn't reserved", |tag| UserTag::new(tag).ok());
    let overlap = select(vec![Overlap::Skip, Overlap::Queue]);
    (
        (any::<Index>(), weighted(0.3, host), label),
        (vec(source, 1..=3), vec(tag, 0..=3)),
        (weighted(0.6, schedule()), overlap, weighted(0.4, retry())),
        (filter(), retention(), hooks()),
    )
        .prop_map(
            |(
                (repository, host, label),
                (sources, tags),
                (schedule, overlap, retry),
                (filter, retention, hooks),
            )| DrawnPlan {
                repository,
                host,
                label,
                sources,
                tags,
                schedule,
                overlap,
                retry,
                filter,
                retention,
                hooks,
            },
        )
}

/// Where a location's address starts: a local path, by itself or with its
/// scheme, a REST server and two OpenDAL services. None of them names a
/// path on this machine by a relative path, whatever follows.
const PLACES: &[&str] = &[
    "/srv/backups/",
    "local:/srv/backups/",
    "rest:https://demo@nas.example:8000/",
    "opendal:s3:",
    "opendal:sftp:",
];

/// One location of a repository, but for its address, which is made
/// different from every other one drawn beside it.
#[derive(Clone, Debug)]
struct DrawnLocation {
    place: &'static str,
    rest: String,
    hot: Option<(&'static str, String)>,
    options: [BTreeMap<String, String>; 3],
    when: Option<LocationRule>,
}

impl DrawnLocation {
    /// The location, as the `nth` of the repository `at` among those drawn.
    fn location(self, at: usize, nth: usize) -> Location {
        let [options, options_hot, options_cold] = self.options;
        Location {
            repository: format!("{}{at}-{nth}/{}", self.place, self.rest),
            repo_hot: self
                .hot
                .map(|(place, rest)| format!("{place}{at}-{nth}-hot/{rest}")),
            options,
            options_hot,
            options_cold,
            when: self.when,
        }
    }
}

/// A list of what `item` draws, at most `most` of them, as often as not one
/// or two.
fn up_to<S: Strategy>(item: impl Fn() -> S, most: usize) -> impl Strategy<Value = Vec<S::Value>> {
    prop_oneof![4 => vec(item(), 1..=2), 1 => vec(item(), most)]
}

fn network_name() -> impl Strategy<Value = NetworkName> {
    text().prop_filter_map("a network's name", |text| NetworkName::parse(&text).ok())
}

fn alternative() -> impl Strategy<Value = Alternative> {
    let names =
        (any::<bool>(), up_to(network_name, MAX_NETWORK_NAMES)).prop_map(|(only, names)| {
            if only {
                NameFilter::Only(names)
            } else {
                NameFilter::Except(names)
            }
        });
    prop_oneof![
        Just((NetworkKind::Wired, None)),
        weighted(0.6, names).prop_map(|names| (NetworkKind::WiFi, names)),
    ]
    .prop_map(|(kind, names)| Alternative::new(kind, names).expect("names only on Wi-Fi"))
}

fn location_rule() -> impl Strategy<Value = LocationRule> {
    up_to(alternative, MAX_RULE_ALTERNATIVES)
        .prop_map(|alternatives| LocationRule::new(alternatives).expect("drawn in its range"))
}

/// The options a backend reads as a file or a program on this machine
/// (`RepositorySettings::relative_path`), OpenDAL's in either case.
const PATH_OPTIONS: &[&str] = &[
    "http-cacert",
    "http-tls-client-cert",
    "post-create-command",
    "post-delete-command",
    "key",
    "KEY",
    "root",
    "atomic_write_dir",
    "credential_path",
    "google_application_credentials",
    "oidc_token_file",
];

/// A location's options: any name with any value, or one of
/// [`PATH_OPTIONS`] with an absolute path, which no backend reads as
/// relative.
fn options() -> impl Strategy<Value = BTreeMap<String, String>> {
    let option = prop_oneof![
        3 => (text(), text()),
        1 => (select(PATH_OPTIONS).prop_map(str::to_owned), absolute()),
    ];
    vec(option, 0..=2).prop_map(|options| options.into_iter().collect())
}

fn location() -> impl Strategy<Value = DrawnLocation> {
    (
        select(PLACES),
        text(),
        weighted(0.3, (select(PLACES), text())),
        [options(), options(), options()],
        weighted(0.3, location_rule()),
    )
        .prop_map(|(place, rest, hot, options, when)| DrawnLocation {
            place,
            rest,
            hot,
            options,
            when,
        })
}

/// A repository, but for its alias and its locations' addresses.
#[derive(Clone, Debug)]
struct DrawnRepository {
    first: DrawnLocation,
    further: Vec<DrawnLocation>,
    password: SecretRef,
    id: RepositoryId,
    recorded: bool,
    maintenance: Maintenance,
    group_by: GroupBy,
    hooks: Hooks,
}

impl DrawnRepository {
    /// The repository, as the one `at` among those drawn. One with further
    /// locations records its ID, which the reader requires of it.
    fn repository(self, at: usize) -> RepositoryConfig {
        let first = self.first.location(at, 0);
        let further: Vec<_> = self
            .further
            .into_iter()
            .enumerate()
            .map(|(n, location)| location.location(at, n + 1))
            .collect();
        let id = (self.recorded || !further.is_empty()).then_some(self.id);
        RepositoryConfig {
            locations: Locations {
                first_when: first.when.clone(),
                further,
            },
            settings: first.with_password(self.password),
            id,
            maintenance: self.maintenance,
            group_by: self.group_by,
            hooks: self.hooks,
        }
    }
}

fn repository() -> impl Strategy<Value = DrawnRepository> {
    let further = prop_oneof![
        3 => Just(Vec::new()),
        2 => vec(location(), 1..MAX_LOCATIONS),
    ];
    let id = "[0-9a-fA-F]{64}".prop_map(|text| RepositoryId::parse(&text).expect("64 hex digits"));
    let maintenance = select(vec![
        Maintenance::None,
        Maintenance::Local,
        Maintenance::Hub,
    ]);
    let group_by = any::<[bool; 4]>().prop_map(|[host, label, paths, tags]| GroupBy {
        host,
        label,
        paths,
        tags,
    });
    (
        (location(), further, password()),
        (id, any::<bool>(), maintenance, group_by, hooks()),
    )
        .prop_map(
            |((first, further, password), (id, recorded, maintenance, group_by, hooks))| {
                DrawnRepository {
                    first,
                    further,
                    password,
                    id,
                    recorded,
                    maintenance,
                    group_by,
                    hooks,
                }
            },
        )
}

fn channel() -> impl Strategy<Value = Channel> {
    let username = plain(&[':']).prop_filter_map("a user name", |text| Username::parse(&text).ok());
    let auth = prop_oneof![
        Just(None),
        named_secret().prop_map(|token| Some(Auth::Bearer(token))),
        (username, named_secret())
            .prop_map(|(username, password)| Some(Auth::Basic { username, password })),
    ];
    (select(ChannelKind::ALL.to_vec()), named_secret(), auth).prop_map(|(kind, url, auth)| {
        let auth = auth.filter(|_| kind.takes_auth());
        Channel::new(kind, url, auth).expect("no secret as text, and auth only where it is taken")
    })
}

/// A rule, but for the channel and the plans it names, which are among
/// those drawn beside it.
#[derive(Clone, Debug)]
struct DrawnRule {
    events: BTreeSet<EventKind>,
    channel: Index,
    plans: Vec<Index>,
    template: Option<Template>,
}

impl DrawnRule {
    fn rule(self, channels: &[ChannelName], plans: &[Plan]) -> Rule {
        let plans = if plans.is_empty() {
            BTreeSet::new()
        } else {
            self.plans
                .iter()
                .map(|at| at.get(plans).id.clone())
                .collect()
        };
        Rule::new(
            self.events,
            self.channel.get(channels).clone(),
            plans,
            self.template,
        )
        .expect("at least one event")
    }
}

fn template() -> impl Strategy<Value = Template> {
    let placeholders = [
        "{event}",
        "{plan}",
        "{repository}",
        "{operation}",
        "{code}",
        "{time}",
        "{attempt}",
        "{attempts}",
        "{{",
        "}}",
    ];
    let piece = prop_oneof![
        2 => text().prop_map(|text| text.replace(['{', '}'], "")),
        1 => select(placeholders.to_vec()).prop_map(str::to_owned),
    ];
    vec(piece, 1..=5).prop_filter_map("a template", |pieces| {
        Template::parse(&pieces.concat()).ok()
    })
}

fn rule() -> impl Strategy<Value = DrawnRule> {
    (
        btree_set(select(EventKind::ALL.to_vec()), 1..=3),
        any::<Index>(),
        vec(any::<Index>(), 0..=2),
        weighted(0.5, template()),
    )
        .prop_map(|(events, channel, plans, template)| DrawnRule {
            events,
            channel,
            plans,
            template,
        })
}

/// The keys of a configuration's tables: up to three aliases, four plan
/// IDs, three channel names and three rule names, each list in order.
#[derive(Clone, Debug)]
struct Names {
    aliases: Vec<String>,
    ids: Vec<String>,
    channels: Vec<String>,
    rules: Vec<String>,
}

fn names() -> impl Strategy<Value = Names> {
    (slugs(3), slugs(4), slugs(3), slugs(3)).prop_map(|(aliases, ids, channels, rules)| Names {
        aliases,
        ids,
        channels,
        rules,
    })
}

/// A configuration, but for the keys of its tables: a table missing from a
/// list leaves its key unused, so a configuration may lack any of them.
#[derive(Clone, Debug)]
struct Drawn {
    agent: AgentSettings,
    hooks: Hooks,
    repositories: Vec<Option<DrawnRepository>>,
    plans: Vec<Option<DrawnPlan>>,
    rotation: Index,
    channels: Vec<Option<Channel>>,
    rules: Vec<Option<DrawnRule>>,
    serve: ServeSettings,
}

impl Drawn {
    /// The configuration with the keys of `names`, each with the table
    /// drawn at its place: plans only where there is a repository to back
    /// up into, and rules only where there is a channel to send to. The
    /// plans are in the order of their IDs unless `reordered`.
    fn configuration(self, names: &Names, reordered: bool) -> AgentConfig {
        let repositories: BTreeMap<_, _> = names
            .aliases
            .iter()
            .map(|alias| RepositoryAlias::parse(alias).expect("a slug"))
            .zip(self.repositories)
            .enumerate()
            .filter_map(|(at, (alias, drawn))| Some((alias, drawn?.repository(at))))
            .collect();
        let aliases: Vec<_> = repositories.keys().cloned().collect();
        let mut plans: Vec<_> = if aliases.is_empty() {
            Vec::new()
        } else {
            names
                .ids
                .iter()
                .map(|id| PlanId::parse(id).expect("a slug"))
                .zip(self.plans)
                .filter_map(|(id, drawn)| Some(drawn?.plan(id, &aliases)))
                .collect()
        };
        if reordered && !plans.is_empty() {
            let by = self.rotation.index(plans.len());
            plans.rotate_left(by);
        }
        let channels: BTreeMap<_, _> = names
            .channels
            .iter()
            .map(|name| ChannelName::parse(name).expect("a slug"))
            .zip(self.channels)
            .filter_map(|(name, channel)| Some((name, channel?)))
            .collect();
        let channel_names: Vec<_> = channels.keys().cloned().collect();
        let rules: BTreeMap<_, _> = if channel_names.is_empty() {
            BTreeMap::new()
        } else {
            names
                .rules
                .iter()
                .map(|name| RuleName::parse(name).expect("a slug"))
                .zip(self.rules)
                .filter_map(|(name, drawn)| Some((name, drawn?.rule(&channel_names, &plans))))
                .collect()
        };
        AgentConfig::new(self.agent, self.hooks, repositories, plans)
            .and_then(|config| config.with_notifications(Notifications::new(channels, rules)?))
            .map(|config| config.with_serve(self.serve))
            .expect("drawn by the configuration's rules")
    }

    /// These tables, but `before`'s where `same` says.
    fn keeping(self, before: &Self, same: &Same) -> Self {
        Self {
            agent: if same.agent {
                before.agent.clone()
            } else {
                self.agent
            },
            hooks: if same.hooks {
                before.hooks.clone()
            } else {
                self.hooks
            },
            repositories: mix(&before.repositories, self.repositories, &same.repositories),
            plans: mix(&before.plans, self.plans, &same.plans),
            rotation: self.rotation,
            channels: mix(&before.channels, self.channels, &same.channels),
            rules: mix(&before.rules, self.rules, &same.rules),
            serve: if same.serve {
                before.serve.clone()
            } else {
                self.serve
            },
        }
    }
}

/// `after`'s tables, but `before`'s at each place `same` says, as many as
/// the longer list has.
fn mix<T: Clone>(before: &[Option<T>], after: Vec<Option<T>>, same: &[bool]) -> Vec<Option<T>> {
    let length = before.len().max(after.len());
    let mut after = after.into_iter();
    (0..length)
        .map(|at| {
            let drawn = after.next().flatten();
            if same.get(at) == Some(&true) {
                before.get(at).cloned().flatten()
            } else {
                drawn
            }
        })
        .collect()
}

/// Which tables a configuration keeps from the one before it: `[agent]`,
/// `[hooks]`, and each repository, plan, channel and rule by its place.
#[derive(Clone, Debug)]
struct Same {
    agent: bool,
    hooks: bool,
    serve: bool,
    repositories: Vec<bool>,
    plans: Vec<bool>,
    channels: Vec<bool>,
    rules: Vec<bool>,
}

fn same() -> impl Strategy<Value = Same> {
    let places = || vec(any::<bool>(), 4);
    (
        (any::<bool>(), any::<bool>(), any::<bool>()),
        [places(), places(), places(), places()],
    )
        .prop_map(
            |((agent, hooks, serve), [repositories, plans, channels, rules])| Same {
                agent,
                hooks,
                serve,
                repositories,
                plans,
                channels,
                rules,
            },
        )
}

/// What `serve` starts with: each key left out or set, the address and the
/// names from a few of each kind the reader takes.
fn serve() -> impl Strategy<Value = ServeSettings> {
    let address = prop_oneof![
        Just("127.0.0.1:9898"),
        Just("0.0.0.0:8080"),
        Just("[::]:9898"),
        Just("[fd00::1]:443"),
    ]
    .prop_map(|text| ServeSettings::parse_listen(text).expect("an address"));
    let host = prop_oneof![
        Just("nas.example.com".to_owned()),
        Just("192.0.2.10".to_owned()),
        Just("[fd00::1]".to_owned()),
    ];
    (weighted(0.5, address), vec(host, 0..=2), any::<bool>()).prop_map(
        |(listen, allowed_hosts, hub)| ServeSettings {
            listen,
            allowed_hosts,
            hub,
        },
    )
}

fn drawn() -> impl Strategy<Value = Drawn> {
    (
        // Boxed, as the lists' strategies are: unboxed, the whole drawing
        // overflowed the test thread's stack once `[agent]` had seven keys.
        (agent().boxed(), hooks(), serve()),
        (
            vec(weighted(0.8, repository().boxed()), 0..=3),
            vec(weighted(0.8, plan().boxed()), 0..=4),
            any::<Index>(),
        ),
        (
            vec(weighted(0.8, channel().boxed()), 0..=3),
            vec(weighted(0.8, rule().boxed()), 0..=3),
        ),
    )
        .prop_map(
            |((agent, hooks, serve), (repositories, plans, rotation), (channels, rules))| Drawn {
                agent,
                hooks,
                repositories,
                plans,
                rotation,
                channels,
                rules,
                serve,
            },
        )
}

/// A configuration the domain allows and the reader takes, its plans in an
/// order other than their IDs'.
fn configuration() -> impl Strategy<Value = AgentConfig> {
    (names(), drawn()).prop_map(|(names, drawn)| drawn.configuration(&names, true))
}

/// Two configurations whose tables have the same keys, as many of them as
/// each has: a repository, a plan, a channel or a rule may be in both, as
/// it was or with other settings, or in one of them only, wherever it is
/// among the others. The plans of each are in the order of their IDs, so
/// the plans in both are in the same order in each.
fn two_configurations() -> impl Strategy<Value = (AgentConfig, AgentConfig)> {
    (names(), drawn(), drawn(), same()).prop_map(|(names, before, after, same)| {
        let after = after.keeping(&before, &same);
        (
            before.configuration(&names, false),
            after.configuration(&names, false),
        )
    })
}

/// `config` with `change` made to its parts, checked again by the domain.
fn changed(
    config: &AgentConfig,
    change: impl FnOnce(
        &mut BTreeMap<RepositoryAlias, RepositoryConfig>,
        &mut BTreeMap<ChannelName, Channel>,
    ),
) -> AgentConfig {
    let mut repositories = config.repositories().clone();
    let mut channels = config.notifications().channels().clone();
    change(&mut repositories, &mut channels);
    let rules = config.notifications().rules().clone();
    AgentConfig::new(
        config.agent().clone(),
        config.hooks().clone(),
        repositories,
        config.plans().to_vec(),
    )
    .and_then(|config| config.with_notifications(Notifications::new(channels, rules)?))
    .expect("only a secret changed")
}

/// The key `render` refuses `config` at, if it refuses it as unwritable.
fn refused_at(config: &AgentConfig) -> Result<Option<String>, String> {
    match render(file(), config) {
        Err(ConfigError::Invalid {
            key,
            problem: Problem::Unwritable,
            at: None,
            ..
        }) => Ok(key),
        other => Err(format!("{other:?}")),
    }
}

proptest! {
    #![proptest_config(config())]

    #[test]
    fn a_configuration_written_is_read_back_as_it_was(config in configuration()) {
        let text = render(file(), &config).expect("a configuration the file can say");
        let read = parse(file(), &text, &Schemas::production());
        prop_assert!(read.is_ok(), "{:?}, reading:\n{}", read.as_ref().err(), text);
        let read = read.expect("checked");
        prop_assert!(!read.migrated);
        prop_assert_eq!(&read.config, &config, "read back otherwise from:\n{}", text);
        let again = render(file(), &read.config).expect("written again");
        prop_assert_eq!(again, text, "the same configuration gives the same text");
    }

    #[test]
    fn a_file_written_and_edited_in_place_reads_back_as_the_new_configuration(
        (before, after) in two_configurations(),
    ) {
        let schemas = Schemas::production();
        let text = render(file(), &before).expect("a configuration the file can say");
        let rendered = render(file(), &after).expect("a configuration the file can say");
        let edited = saved(file(), &text, &after, &rendered, &schemas);
        prop_assert!(
            edited.is_some(),
            "not edited in place, from:\n{}\nto:\n{}",
            text,
            rendered
        );
        let edited = edited.expect("checked");
        let read = parse(file(), &edited, &schemas).expect("an edit is kept only if it reads back");
        prop_assert_eq!(&read.config, &after, "edited as:\n{}", edited);
    }

    #[test]
    fn a_secret_the_file_cannot_say_is_refused_at_its_key(
        config in configuration(),
        path in absolute(),
    ) {
        let whole = SecretRef::File { path, read: FileRead::Whole };
        for alias in config.repositories().keys() {
            let unwritable = changed(&config, |repositories, _| {
                if let Some(repository) = repositories.get_mut(alias) {
                    repository.settings.password = whole.clone();
                }
            });
            let key = format!("repositories.{}.password", alias.as_str());
            prop_assert_eq!(refused_at(&unwritable), Ok(Some(key)));
        }
        for (name, channel) in config.notifications().channels() {
            let (kind, url, auth) = (channel.kind(), channel.url(), channel.auth());
            let mut unwritable = vec![(
                "url",
                Channel::new(kind, whole.clone(), auth.cloned()),
            )];
            match auth {
                None => {}
                Some(Auth::Bearer(_)) => unwritable.push((
                    "token",
                    Channel::new(kind, url.clone(), Some(Auth::Bearer(whole.clone()))),
                )),
                Some(Auth::Basic { username, .. }) => unwritable.push((
                    "password",
                    Channel::new(
                        kind,
                        url.clone(),
                        Some(Auth::Basic {
                            username: username.clone(),
                            password: whole.clone(),
                        }),
                    ),
                )),
            }
            for (key, channel) in unwritable {
                let channel = channel.expect("a whole file is a secret a channel may have");
                let unwritable = changed(&config, |_, channels| {
                    channels.insert(name.clone(), channel);
                });
                let key = format!("notifications.channels.{}.{key}", name.as_str());
                prop_assert_eq!(refused_at(&unwritable), Ok(Some(key)));
            }
        }
    }
}
