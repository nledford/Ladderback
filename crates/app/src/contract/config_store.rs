use std::collections::BTreeMap;

use domain::execution::plan_tags::UserTag;
use domain::planning::{
    AgentConfig, AgentSettings, Concurrency, CronExpression, GroupBy, HookEntry, Hooks, Hostname,
    KeepCount, KeepWithin, Locale, Maintenance, Nice, OnFailure, Plan, RepositoryConfig,
    RetentionPolicy, RetryPolicy, Schedule, Source, SourceFilter,
};
use domain::{PlanId, RepositoryAlias, SignedDuration};

use crate::ports::ConfigStore;

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
    let settings = AgentSettings {
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
                .with_jitter(SignedDuration::from_millis(90_250))
                .ok()?
                .with_catch_up(false)
                .with_on_ac_power(true),
        ),
        retry: Some(RetryPolicy::new(3, SignedDuration::from_millis(600_500)).ok()?),
        filter: SourceFilter {
            globs: strings(&["*.tmp", "!keep.tmp", "# not a comment", "say \"hi\""]),
            iglobs: strings(&["*.LOG"]),
            glob_files: strings(&["/home/demo/.excludes"]),
            iglob_files: strings(&["/home/demo/.iexcludes"]),
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
            cloud,
            vec![source("/home/demo/photos")?, source("/home/demo/video")?],
        )
    };

    AgentConfig::new(
        settings,
        agent_hooks,
        repositories,
        vec![home, work, photos],
    )
    .ok()
}

/// One repository and one plan, with nothing else set: no part of it
/// matches anything in [`rich`] except an alias.
fn minimal() -> Option<AgentConfig> {
    let nas = alias("nas")?;
    let repository = RepositoryConfig {
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
