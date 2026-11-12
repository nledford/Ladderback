//! The model every golden file says, whatever version of the format it is in.

use std::collections::BTreeMap;

use domain::execution::plan_tags::UserTag;
use domain::planning::{
    AgentConfig, AgentSettings, Concurrency, CronExpression, GlobFile, GroupBy, HookEntry, Hooks,
    Hostname, Interval, KeepCount, KeepWithin, Locale, Maintenance, Nice, OnFailure, Overlap, Plan,
    RepositoryConfig, RepositorySettings, RetentionPolicy, RetryPolicy, Schedule, Source,
    SourceFilter,
};
use domain::secrets::{CommandLine, FileRead, KeyringEntry, SecretRef, SecretValue};
use domain::{PlanId, RepositoryAlias, SignedDuration};

fn hook(text: &str) -> HookEntry {
    HookEntry::parse(text).expect("a hook")
}

fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|item| (*item).to_owned()).collect()
}

fn by_name(items: &[(&str, &str)]) -> BTreeMap<String, String> {
    items
        .iter()
        .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
        .collect()
}

fn alias(text: &str) -> RepositoryAlias {
    RepositoryAlias::parse(text).expect("an alias")
}

fn source(text: &str) -> Source {
    Source::parse(text).expect("a source")
}

fn settings(repository: &str, password: SecretRef) -> RepositorySettings {
    RepositorySettings {
        repository: repository.to_owned(),
        repo_hot: None,
        password,
        options: BTreeMap::new(),
        options_hot: BTreeMap::new(),
        options_cold: BTreeMap::new(),
    }
}

/// What `golden/v1.toml` says, and what every later version's file must say
/// of the same configuration.
pub fn expected() -> AgentConfig {
    let agent = AgentSettings {
        hostname: Some(Hostname::parse("demo-laptop").expect("a hostname")),
        concurrency: Concurrency::new(2).expect("a concurrency"),
        nice: Some(Nice::new(10).expect("a nice")),
        allow_hub_restore: true,
        locale: Some(Locale::parse("en-US").expect("a locale")),
    };
    let hooks = Hooks {
        run_before: vec![hook("echo 'agent starting'")],
        run_finally: vec![
            HookEntry::new("true", Vec::<String>::new(), OnFailure::Ignore).expect("a hook"),
        ],
        ..Hooks::default()
    };
    let repositories = BTreeMap::from([
        (
            alias("nas"),
            RepositoryConfig {
                id: None,
                settings: RepositorySettings {
                    options: by_name(&[(
                        "sftp.command",
                        "ssh -o 'StrictHostKeyChecking yes' backup@nas.lan -s sftp",
                    )]),
                    ..settings(
                        "sftp://backup@nas.lan/backups/demo",
                        SecretRef::Command(
                            CommandLine::parse("security find-generic-password -s demo-nas -w")
                                .expect("a command"),
                        ),
                    )
                },
                maintenance: Maintenance::Hub,
                group_by: GroupBy::default(),
                hooks: Hooks {
                    run_before: vec![hook("echo 'repository ready'")],
                    ..Hooks::default()
                },
            },
        ),
        (
            alias("cloud"),
            RepositoryConfig {
                id: None,
                settings: RepositorySettings {
                    repo_hot: Some("/srv/demo-hot".to_owned()),
                    options: by_name(&[
                        ("endpoint", "https://${DEMO_HOST}/path"),
                        ("key", "${file:/home/demo/.s3-key}"),
                    ]),
                    options_hot: by_name(&[("region", "us-east-1")]),
                    options_cold: by_name(&[("storage-class", "GLACIER")]),
                    ..settings(
                        "s3:s3.example.lan/demo-bucket",
                        SecretRef::Keyring(
                            KeyringEntry::parse("ladderback/demo-cloud").expect("an entry"),
                        ),
                    )
                },
                maintenance: Maintenance::Local,
                group_by: GroupBy {
                    host: true,
                    label: false,
                    paths: false,
                    tags: true,
                },
                hooks: Hooks::default(),
            },
        ),
        (
            alias("usb"),
            RepositoryConfig {
                id: None,
                settings: settings(
                    "/media/demo/usb backups",
                    SecretRef::File {
                        path: "/home/demo/.usb-password".to_owned(),
                        read: FileRead::FirstLine,
                    },
                ),
                maintenance: Maintenance::None,
                group_by: GroupBy::default(),
                hooks: Hooks::default(),
            },
        ),
        (
            alias("vault"),
            RepositoryConfig {
                id: None,
                settings: settings(
                    "/srv/demo-vault",
                    SecretRef::Env("DEMO_VAULT_PASSWORD".to_owned()),
                ),
                maintenance: Maintenance::None,
                group_by: GroupBy {
                    host: false,
                    label: false,
                    paths: false,
                    tags: false,
                },
                hooks: Hooks::default(),
            },
        ),
        (
            alias("scratch"),
            RepositoryConfig {
                id: None,
                settings: settings(
                    "/srv/demo-scratch",
                    SecretRef::Inline(SecretValue::new("demo \"inline\" #password")),
                ),
                maintenance: Maintenance::None,
                group_by: GroupBy::default(),
                hooks: Hooks::default(),
            },
        ),
    ]);

    let count = |n| Some(KeepCount::new(n).expect("a count"));
    let within = |text: &str| Some(KeepWithin::parse(text).expect("a duration"));
    let home = Plan {
        id: PlanId::parse("home").expect("an ID"),
        repository: alias("nas"),
        host: Some(Hostname::parse("demo-laptop").expect("a hostname")),
        label: Some("Demo \"home\" backup #1 \u{2013} caf\u{e9}".to_owned()),
        sources: vec![
            source("/home/demo"),
            source("/srv/demo data/it's here"),
            source("/home/demo/\u{e9}/\u{65e5}\u{672c}"),
        ],
        tags: ["demo", "laptop", "Home Dir"]
            .into_iter()
            .map(|tag| UserTag::new(tag).expect("a tag"))
            .collect(),
        schedule: Some(
            Schedule::new(CronExpression::parse("0 3 * * 1-5").expect("a cron"))
                .with_zone(super::zone("Europe/Berlin"))
                .with_jitter(SignedDuration::from_secs(90))
                .expect("a jitter")
                .with_catch_up(false)
                .with_on_ac_power(true),
        ),
        overlap: Overlap::Queue,
        retry: Some(RetryPolicy::new(3, SignedDuration::from_secs(600)).expect("a retry")),
        filter: SourceFilter {
            globs: strings(&["*.tmp", "!keep.tmp"]),
            iglobs: strings(&["*.LOG"]),
            glob_files: vec![GlobFile::parse("/home/demo/.excludes").unwrap()],
            iglob_files: vec![GlobFile::parse("/home/demo/.iexcludes").unwrap()],
            git_ignore: true,
            no_require_git: true,
            custom_ignorefiles: strings(&[".backupignore"]),
            exclude_if_present: strings(&[".nobackup", "CACHEDIR.TAG"]),
            exclude_if_xattr: strings(&["com.example.skip"]),
            one_file_system: true,
        },
        retention: RetentionPolicy {
            keep_last: count(1),
            keep_hourly: count(3),
            keep_daily: count(7),
            keep_weekly: count(4),
            keep_monthly: count(12),
            keep_quarter_yearly: count(0),
            keep_yearly: count(-1),
            keep_within: within("36 hours"),
            keep_within_weekly: within("1 month"),
            keep_within_yearly: within("2 years"),
            ..RetentionPolicy::default()
        },
        hooks: Hooks {
            run_before: vec![hook("echo 'plan starting' --now")],
            run_after: vec![
                HookEntry::new(
                    "/opt/demo tools/notify",
                    ["done", "two words"],
                    OnFailure::Warn,
                )
                .expect("a hook"),
                hook("echo second"),
            ],
            ..Hooks::default()
        },
    };
    let work = Plan::new(
        PlanId::parse("work").expect("an ID"),
        alias("cloud"),
        vec![source("/home/demo/work")],
    );
    let photos = Plan {
        label: Some(String::new()),
        schedule: Some(Schedule::new(
            CronExpression::parse("@daily").expect("a cron"),
        )),
        ..Plan::new(
            PlanId::parse("photos").expect("an ID"),
            alias("cloud"),
            vec![source("/home/demo/photos"), source("/home/demo/video")],
        )
    };
    let mail = Plan {
        schedule: Some(Schedule::new(
            Interval::new(SignedDuration::from_mins(90)).expect("an interval"),
        )),
        ..Plan::new(
            PlanId::parse("mail").expect("an ID"),
            alias("cloud"),
            vec![source("/home/demo/mail")],
        )
    };
    AgentConfig::new(agent, hooks, repositories, vec![home, work, photos, mail])
        .expect("the golden configuration agrees with itself")
}
