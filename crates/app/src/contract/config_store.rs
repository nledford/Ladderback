use std::collections::BTreeMap;

use domain::execution::plan_tags::UserTag;
use domain::planning::{
    AgentConfig, AgentSettings, Concurrency, CronExpression, GlobFile, GroupBy, HookEntry, Hooks,
    Hostname, Interval, KeepCount, KeepWithin, Locale, Maintenance, Nice, OnFailure, Overlap, Plan,
    RepositoryConfig, RepositorySettings, RetentionPolicy, RetryPolicy, Schedule, ScheduleZone,
    Source, SourceFilter,
};
use domain::secrets::{CommandLine, FileRead, KeyringEntry, SecretRef, SecretValue};
use domain::{PlanId, RepositoryAlias, RepositoryId, SignedDuration, TimeZoneDatabase};

use crate::ports::ConfigStore;

/// Settings for a repository at `repository`, with a password from the
/// environment and no options: the caller sets what it is about.
fn settings(repository: &str) -> RepositorySettings {
    RepositorySettings {
        repository: repository.to_owned(),
        repo_hot: None,
        password: SecretRef::Env("DEMO_PASSWORD".to_owned()),
        options: BTreeMap::new(),
        options_hot: BTreeMap::new(),
        options_cold: BTreeMap::new(),
    }
}

fn strings_by_name(items: &[(&str, &str)]) -> BTreeMap<String, String> {
    items
        .iter()
        .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
        .collect()
}

/// The schedule zone `name` names in the database built in: the store's
/// adapter looks the name up in the system's, and zones are equal by name.
fn zone(name: &str) -> Option<ScheduleZone> {
    ScheduleZone::new(name, TimeZoneDatabase::bundled().get(name).ok()?).ok()
}

/// Run against an empty store: the first check is that it loads the empty
/// configuration. The suite saves a handful of configurations over one
/// another, so a store behind a file should be a scratch one.
///
/// What the suite checks is the round trip: that every part of a
/// configuration, set to something no default would give, comes back as it
/// went in, down to the order of each list and the characters of each string.
/// How a store fails on a file it can't read is for its own tests.
pub async fn run(store: &dyn ConfigStore) {
    let configs = Configs::new();
    assert!(
        configs.is_some(),
        "the suite's own configurations are valid ones"
    );
    let Some(configs) = configs else { return };

    a_store_nothing_was_saved_to_loads_the_empty_configuration(store).await;
    a_saved_configuration_loads_as_it_was_saved(store, &configs.rich).await;
    loading_twice_gives_the_same_configuration(store, &configs.rich).await;
    saving_what_was_loaded_changes_nothing(store, &configs.rich).await;
    a_minimal_configuration_loads_as_it_was_saved(store, &configs.minimal).await;
    the_last_save_wins(store, &configs).await;
    recording_an_id_changes_that_entry_and_nothing_else(store, &configs.rich).await;
    an_id_the_configuration_says_not_to_record_changes_nothing(store, &configs.rich).await;
}

/// A configuration that sets every part of the model, and one that sets only
/// what the model requires.
struct Configs {
    rich: AgentConfig,
    minimal: AgentConfig,
}

impl Configs {
    fn new() -> Option<Self> {
        Some(Self {
            rich: rich()?,
            minimal: minimal()?,
        })
    }
}

fn alias(text: &str) -> Option<RepositoryAlias> {
    RepositoryAlias::parse(text).ok()
}

fn plan_id(text: &str) -> Option<PlanId> {
    PlanId::parse(text).ok()
}

fn source(text: &str) -> Option<Source> {
    Source::parse(text).ok()
}

fn hook(text: &str) -> Option<HookEntry> {
    HookEntry::parse(text).ok()
}

fn count(count: i64) -> Option<KeepCount> {
    KeepCount::new(count).ok()
}

fn within(text: &str) -> Option<KeepWithin> {
    KeepWithin::parse(text).ok()
}

fn tag(text: &str) -> Option<UserTag> {
    UserTag::new(text).ok()
}

fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|item| (*item).to_owned()).collect()
}

/// Every part set, to values that no default and no neighbouring part
/// shares, and with the characters a file format has to quote.
fn rich() -> Option<AgentConfig> {
    let agent_settings = AgentSettings {
        hostname: Some(Hostname::parse("demo-host").ok()?),
        concurrency: Concurrency::new(4).ok()?,
        nice: Some(Nice::new(10).ok()?),
        allow_hub_restore: true,
        locale: Some(Locale::parse("en-US").ok()?),
    };
    let agent_hooks = Hooks {
        run_before: vec![hook("echo 'agent starting'")?],
        run_after: vec![HookEntry::new("true", Vec::<String>::new(), OnFailure::Ignore).ok()?],
        run_failed: vec![hook("echo failed")?],
        run_finally: vec![hook("echo done")?],
    };

    let nas = alias("nas")?;
    let cloud = alias("cloud")?;
    let usb = alias("usb")?;
    let repositories = BTreeMap::from([
        (
            nas.clone(),
            RepositoryConfig {
                locations: Default::default(),
                id: Some(RepositoryId::parse(&"0123456789abcdef".repeat(4)).ok()?),
                settings: RepositorySettings {
                    password: SecretRef::Command(
                        CommandLine::new(
                            "security",
                            ["find-generic-password", "-s", "demo nas", "-w"],
                        )
                        .ok()?,
                    ),
                    options: strings_by_name(&[(
                        "sftp.command",
                        "ssh -o 'x y' backup@nas.lan -s sftp",
                    )]),
                    ..settings("sftp://backup@nas.lan/backups/demo")
                },
                maintenance: Maintenance::Hub,
                group_by: GroupBy {
                    host: true,
                    label: false,
                    paths: true,
                    tags: true,
                },
                hooks: Hooks {
                    run_before: vec![hook("echo 'repository ready'")?],
                    run_after: vec![hook("echo 'repository done'")?],
                    run_failed: vec![hook("echo 'repository failed'")?],
                    run_finally: vec![hook("echo 'repository finally'")?],
                },
            },
        ),
        (
            cloud.clone(),
            RepositoryConfig {
                locations: Default::default(),
                id: None,
                settings: RepositorySettings {
                    repo_hot: Some("/srv/demo-hot".to_owned()),
                    password: SecretRef::Keyring(
                        KeyringEntry::parse("ladderback/demo cloud").ok()?,
                    ),
                    options: strings_by_name(&[
                        ("endpoint", "https://${DEMO_HOST}/path"),
                        ("key", "${file:/home/demo/.s3 key}"),
                    ]),
                    options_hot: strings_by_name(&[("region", "us-east-1")]),
                    options_cold: strings_by_name(&[("storage-class", "GLACIER")]),
                    ..settings("s3:s3.example.lan/demo-bucket")
                },
                maintenance: Maintenance::Local,
                group_by: GroupBy::default(),
                hooks: Hooks {
                    run_failed: vec![hook("echo 'cloud failed'")?],
                    ..Hooks::default()
                },
            },
        ),
        (
            usb,
            RepositoryConfig {
                locations: Default::default(),
                id: None,
                settings: RepositorySettings {
                    password: SecretRef::File {
                        path: "/home/demo/.usb password".to_owned(),
                        read: FileRead::FirstLine,
                    },
                    ..settings("/media/demo/usb backups")
                },
                maintenance: Maintenance::None,
                group_by: GroupBy {
                    host: false,
                    label: false,
                    paths: false,
                    tags: false,
                },
                hooks: Hooks {
                    run_finally: vec![hook("echo 'usb finally'")?],
                    ..Hooks::default()
                },
            },
        ),
        (
            alias("vault")?,
            RepositoryConfig {
                locations: Default::default(),
                id: None,
                settings: RepositorySettings {
                    password: SecretRef::Env("DEMO_VAULT_PASSWORD".to_owned()),
                    ..settings("/srv/demo-vault")
                },
                maintenance: Maintenance::Local,
                group_by: GroupBy::default(),
                hooks: Hooks::default(),
            },
        ),
        (
            alias("scratch")?,
            RepositoryConfig {
                locations: Default::default(),
                id: None,
                settings: RepositorySettings {
                    password: SecretRef::Inline(SecretValue::new("demo \"inline\" #password")),
                    ..settings("/srv/demo-scratch")
                },
                maintenance: Maintenance::None,
                group_by: GroupBy::default(),
                hooks: Hooks::default(),
            },
        ),
    ]);

    let home = Plan {
        id: plan_id("home")?,
        repository: nas.clone(),
        host: Some(Hostname::parse("demo-laptop").ok()?),
        label: Some("Demo \"home\" backup #1 \u{2013} caf\u{e9}".to_owned()),
        sources: vec![
            source("/home/demo")?,
            source("/srv/demo data/it's here")?,
            source("/home/demo/\u{e9}/\u{65e5}\u{672c}")?,
        ],
        tags: vec![tag("demo")?, tag("laptop")?, tag("Home Dir")?],
        schedule: Some(
            Schedule::new(CronExpression::parse("0 3 * * 1-5").ok()?)
                .with_zone(zone("Europe/Berlin")?)
                .with_jitter(SignedDuration::from_millis(90_250))
                .ok()?
                .with_catch_up(false)
                .with_on_ac_power(true),
        ),
        overlap: Overlap::Queue,
        retry: Some(RetryPolicy::new(3, SignedDuration::from_millis(600_500)).ok()?),
        filter: SourceFilter {
            globs: strings(&["*.tmp", "!keep.tmp", "# not a comment", "say \"hi\""]),
            iglobs: strings(&["*.LOG"]),
            glob_files: vec![GlobFile::parse("/home/demo/.excludes").ok()?],
            iglob_files: vec![GlobFile::parse("/home/demo/.iexcludes").ok()?],
            git_ignore: true,
            no_require_git: true,
            custom_ignorefiles: strings(&[".resticignore", ".backupignore"]),
            exclude_if_present: strings(&[".nobackup", "CACHEDIR.TAG"]),
            exclude_if_xattr: strings(&["com.apple.metadata:com_apple_backup_excludeItem"]),
            one_file_system: true,
        },
        retention: RetentionPolicy {
            keep_last: count(1),
            keep_minutely: count(2),
            keep_hourly: count(3),
            keep_daily: count(7),
            keep_weekly: count(4),
            keep_monthly: count(12),
            keep_quarter_yearly: count(0),
            keep_half_yearly: count(8),
            keep_yearly: count(-1),
            keep_within: within("36 hours"),
            keep_within_minutely: within("90 minutes"),
            keep_within_hourly: within("2 days"),
            keep_within_daily: within("3 weeks"),
            keep_within_weekly: within("1 month"),
            keep_within_monthly: within("6 months"),
            keep_within_quarter_yearly: within("9 months"),
            keep_within_half_yearly: within("1 year"),
            keep_within_yearly: within("2 years"),
        },
        hooks: Hooks {
            run_before: vec![hook("echo 'plan starting' --now")?],
            run_after: vec![
                HookEntry::new(
                    "/opt/demo tools/notify",
                    [
                        "done",
                        "two words",
                        "it's",
                        "say \"hi\"",
                        "back\\slash",
                        "tab\there",
                    ],
                    OnFailure::Warn,
                )
                .ok()?,
                hook("echo 'second one'")?,
            ],
            run_failed: vec![
                hook("echo failed")?,
                HookEntry::new("true", Vec::<String>::new(), OnFailure::Ignore).ok()?,
            ],
            run_finally: vec![hook("echo finished")?],
        },
    };

    // Nothing but what a plan needs: its defaults must come back as defaults.
    let work = Plan::new(
        plan_id("work")?,
        cloud.clone(),
        vec![source("/home/demo/work")?],
    );

    // A schedule that says only when: its defaults must come back as defaults.
    // Its label is empty, which is not the same as having none.
    let photos = Plan {
        label: Some(String::new()),
        schedule: Some(Schedule::new(CronExpression::parse("@daily").ok()?)),
        ..Plan::new(
            plan_id("photos")?,
            cloud.clone(),
            vec![source("/home/demo/photos")?, source("/home/demo/video")?],
        )
    };

    // A schedule that runs every so often, in a zone that plays no part.
    let mail = Plan {
        schedule: Some(
            Schedule::new(Interval::new(SignedDuration::from_mins(90)).ok()?)
                .with_zone(zone("UTC")?),
        ),
        ..Plan::new(plan_id("mail")?, cloud, vec![source("/home/demo/mail")?])
    };

    AgentConfig::new(
        agent_settings,
        agent_hooks,
        repositories,
        vec![home, work, photos, mail],
    )
    .ok()
}

/// One repository and one plan, with nothing else set: no part of it
/// matches anything in [`rich`] except an alias.
fn minimal() -> Option<AgentConfig> {
    let nas = alias("nas")?;
    let repository = RepositoryConfig {
        locations: Default::default(),
        id: None,
        settings: settings("/srv/demo-repo"),
        maintenance: Maintenance::None,
        group_by: GroupBy::default(),
        hooks: Hooks::default(),
    };
    let docs = Plan::new(
        plan_id("docs")?,
        nas.clone(),
        vec![source("/home/demo/docs")?],
    );
    AgentConfig::new(
        AgentSettings::default(),
        Hooks::default(),
        BTreeMap::from([(nas, repository)]),
        vec![docs],
    )
    .ok()
}

async fn saved(store: &dyn ConfigStore, config: &AgentConfig, what: &str) {
    assert_eq!(store.save(config).await, Ok(()), "{what}: the save");
}

/// Loads, and checks the result part by part before checking it whole, so
/// that a failure says which part went wrong.
async fn loads_as(store: &dyn ConfigStore, expected: &AgentConfig, what: &str) {
    let loaded = store.load().await;
    assert!(loaded.is_ok(), "{what}: the load failed with {loaded:?}");
    let Ok(loaded) = loaded else { return };
    assert_eq!(
        loaded.agent(),
        expected.agent(),
        "{what}: the agent's settings"
    );
    assert_eq!(
        loaded.hooks(),
        expected.hooks(),
        "{what}: the agent's hooks"
    );
    assert_eq!(
        loaded.repositories(),
        expected.repositories(),
        "{what}: the repositories"
    );
    assert_eq!(loaded.plans(), expected.plans(), "{what}: the plans");
    assert_eq!(&loaded, expected, "{what}: the configuration");
}

async fn a_store_nothing_was_saved_to_loads_the_empty_configuration(store: &dyn ConfigStore) {
    assert_eq!(
        store.load().await,
        Ok(AgentConfig::default()),
        "a store nothing was saved to loads the empty configuration"
    );
}

async fn a_saved_configuration_loads_as_it_was_saved(
    store: &dyn ConfigStore,
    config: &AgentConfig,
) {
    let what = "a saved configuration loads as it was saved";
    saved(store, config, what).await;
    loads_as(store, config, what).await;
}

async fn loading_twice_gives_the_same_configuration(store: &dyn ConfigStore, config: &AgentConfig) {
    let what = "loading twice gives the same configuration";
    loads_as(store, config, what).await;
    loads_as(store, config, what).await;
}

async fn saving_what_was_loaded_changes_nothing(store: &dyn ConfigStore, config: &AgentConfig) {
    let what = "saving what was loaded changes nothing";
    let loaded = store.load().await;
    assert!(loaded.is_ok(), "{what}: the load failed with {loaded:?}");
    let Ok(loaded) = loaded else { return };
    saved(store, &loaded, what).await;
    loads_as(store, config, what).await;
}

async fn a_minimal_configuration_loads_as_it_was_saved(
    store: &dyn ConfigStore,
    config: &AgentConfig,
) {
    let what = "a configuration with nothing but what is required loads as it was saved";
    saved(store, config, what).await;
    loads_as(store, config, what).await;
}

async fn the_last_save_wins(store: &dyn ConfigStore, configs: &Configs) {
    let what = "the last save wins";
    saved(store, &configs.rich, what).await;
    saved(store, &configs.minimal, what).await;
    loads_as(
        store,
        &configs.minimal,
        "a later save replaces an earlier one",
    )
    .await;

    let empty = AgentConfig::default();
    saved(store, &empty, what).await;
    loads_as(store, &empty, "the empty configuration replaces another").await;

    saved(store, &configs.rich, what).await;
    loads_as(
        store,
        &configs.rich,
        "a configuration saved again replaces the empty one",
    )
    .await;
}

fn new_id(byte: &str) -> Option<RepositoryId> {
    RepositoryId::parse(&byte.repeat(32)).ok()
}

async fn recording_an_id_changes_that_entry_and_nothing_else(
    store: &dyn ConfigStore,
    config: &AgentConfig,
) {
    let what = "recording an ID changes that entry and nothing else";
    saved(store, config, what).await;
    let (Some(cloud), Some(id)) = (alias("cloud"), new_id("c1")) else {
        return;
    };
    let read = config.repository(&cloud).cloned();
    assert!(read.is_some(), "{what}: the suite's entry is there");
    let Some(read) = read else { return };
    assert_eq!(read.id, None, "{what}: the suite's entry records no ID");
    let expected = config.record_id(&cloud, &read, &id);
    assert!(
        expected.is_ok(),
        "{what}: the rule records it: {expected:?}"
    );
    let Ok(expected) = expected else { return };
    assert_eq!(store.record_id(&cloud, &read, &id).await, Ok(()), "{what}");
    loads_as(store, &expected, what).await;
}

async fn an_id_the_configuration_says_not_to_record_changes_nothing(
    store: &dyn ConfigStore,
    config: &AgentConfig,
) {
    use crate::ports::RecordError;
    use domain::planning::NotRecorded;

    let what = "an ID the configuration says not to record changes nothing";
    saved(store, config, what).await;
    let (Some(nas), Some(usb), Some(id)) = (alias("nas"), alias("usb"), new_id("c2")) else {
        return;
    };
    let (Some(recorded), Some(usb_entry)) = (
        config.repository(&nas).cloned(),
        config.repository(&usb).cloned(),
    ) else {
        return;
    };
    let mut edited = usb_entry.clone();
    edited.maintenance = Maintenance::Hub;
    let Some(nas_id) = recorded.id.clone() else {
        return;
    };
    let refusals = [
        (&nas, &recorded, &id, NotRecorded::Recorded),
        (&usb, &edited, &id, NotRecorded::Changed),
        (
            &usb,
            &usb_entry,
            &nas_id,
            NotRecorded::SameRepository { other: nas.clone() },
        ),
    ];
    for (alias, read, id, why) in refusals {
        assert_eq!(
            store.record_id(alias, read, id).await,
            Err(RecordError::NotRecorded(why.clone())),
            "{what}: {why:?}"
        );
        loads_as(store, config, what).await;
    }

    let empty = AgentConfig::default();
    saved(store, &empty, what).await;
    assert_eq!(
        store.record_id(&usb, &usb_entry, &id).await,
        Err(RecordError::NotRecorded(NotRecorded::Changed)),
        "{what}: an entry that isn't there"
    );
    loads_as(store, &empty, what).await;
}
