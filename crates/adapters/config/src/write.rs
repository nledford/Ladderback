//! From the domain's types to the text of a file. What is left to its
//! default isn't written, so the file holds what the user chose and a
//! default that changes later reaches everyone who never set it.
//!
//! The layout is a person's: the keys of a table in the order of the struct
//! that has them, a table for each repository and plan, and the small ones
//! (schedules, options, hooks) on one line. The serializer of `toml` can't
//! give that: it moves a list that holds tables behind the other keys of its
//! table, so the file is laid out here, with `toml` writing each value.

use app::ports::{ConfigError, Problem};
use domain::notifications::{Auth, Channel, Notifications, Rule};
use domain::planning::{
    AgentConfig, AgentSettings, Cadence, GlobFile, GroupBy, HookEntry, Hooks, KeepCount,
    KeepWithin, Location, Maintenance, Overlap, Plan, RepositoryConfig, RetentionPolicy,
    RetryPolicy, Schedule, format_duration,
};
use domain::secrets::{FileRead, SecretRef};

use crate::tables::{
    AgentTable, ChannelTable, FileTable, HookEntryTable, HookValue, HooksTable, LocationTable,
    NotificationsTable, PlanTable, Repositories, RepositoryTable, RetentionTable, RetryTable,
    RuleTable, ScheduleTable,
};

/// `value` unless it is empty, which isn't written.
fn some_if_any<T: Clone>(values: &[T]) -> Option<Vec<T>> {
    (!values.is_empty()).then(|| values.to_vec())
}

/// The glob files as the file writes them, unless there are none.
fn glob_files(files: &[GlobFile]) -> Option<Vec<String>> {
    (!files.is_empty()).then(|| files.iter().map(|file| file.as_str().to_owned()).collect())
}

/// The text of `tables`: `schema`, then `[agent]` and `[hooks]`, a table for
/// each repository with a `[[…locations]]` table for each further location, a table for each plan with its `[plans.retention]`, and
/// a table for each notification channel and rule.
/// `Err` holds what `toml` said when it couldn't write a value.
pub(crate) fn text(tables: &FileTable) -> Result<String, String> {
    let toml::Value::Table(root) = inline_value(tables)? else {
        return Err("the file is not a table".to_owned());
    };
    let mut out = String::new();
    for (key, value) in &root {
        match (key.as_str(), value) {
            ("agent" | "hooks", toml::Value::Table(table)) => {
                section(&mut out, key, false, table, &[])?;
            }
            ("repositories", toml::Value::Table(all)) => {
                for (alias, value) in all {
                    if let toml::Value::Table(table) = value {
                        section(
                            &mut out,
                            &format!("repositories.{alias}"),
                            false,
                            table,
                            &["locations"],
                        )?;
                    }
                }
            }
            ("plans", toml::Value::Array(all)) => {
                for plan in all {
                    if let toml::Value::Table(table) = plan {
                        section(&mut out, "plans", true, table, &["retention"])?;
                    }
                }
            }
            ("notifications", toml::Value::Table(all)) => {
                for group in ["channels", "rules"] {
                    let Some(toml::Value::Table(named)) = all.get(group) else {
                        continue;
                    };
                    for (name, value) in named {
                        if let toml::Value::Table(table) = value {
                            let path = format!("notifications.{group}.{name}");
                            section(&mut out, &path, false, table, &[])?;
                        }
                    }
                }
            }
            _ => line(&mut out, key, value)?,
        }
    }
    Ok(out)
}

/// What `value` serializes to, as a `toml::Value` that keeps the order of its
/// keys.
fn inline_value<T: serde::Serialize>(value: &T) -> Result<toml::Value, String> {
    toml::Value::try_from(value).map_err(|error| error.to_string())
}

/// A value as `toml` writes it inline: a table is `{ a = 1 }`.
fn inline(value: &toml::Value) -> Result<String, String> {
    use serde::Serialize as _;
    let mut written = String::new();
    value
        .serialize(toml::ser::ValueSerializer::new(&mut written))
        .map_err(|error| error.to_string())?;
    Ok(written)
}

/// `key = value` on one line. Every key the writer meets is one of the format's
/// own, which are bare, and the aliases that head a table are slugs.
fn line(out: &mut String, key: &str, value: &toml::Value) -> Result<(), String> {
    out.push_str(key);
    out.push_str(" = ");
    out.push_str(&inline(value)?);
    out.push('\n');
    Ok(())
}

/// A table under `[path]`, or `[[path]]` for the next item of an array of
/// tables, with its keys on lines of their own, and then the `tables` of it
/// that get a header too, as `[path.name]`, or `[[path.name]]` for each item
/// of an array of tables.
fn section(
    out: &mut String,
    path: &str,
    array_item: bool,
    table: &toml::Table,
    tables: &[&str],
) -> Result<(), String> {
    if !out.is_empty() {
        out.push('\n');
    }
    out.push_str(&if array_item {
        format!("[[{path}]]\n")
    } else {
        format!("[{path}]\n")
    });
    for (key, value) in table {
        if !tables.contains(&key.as_str()) {
            line(out, key, value)?;
        }
    }
    for name in tables {
        match table.get(*name) {
            Some(toml::Value::Table(sub)) => {
                section(out, &format!("{path}.{name}"), false, sub, &[])?;
            }
            Some(toml::Value::Array(items)) => {
                for item in items {
                    if let toml::Value::Table(sub) = item {
                        section(out, &format!("{path}.{name}"), true, sub, &[])?;
                    }
                }
            }
            _ => {}
        }
    }
    Ok(())
}

/// The tables of `config` as a file in version `schema`, or why it can't be
/// written: `Err` holds the key of the setting the format can't say.
pub(crate) fn file(config: &AgentConfig, schema: i64) -> Result<FileTable, String> {
    let mut repositories = Vec::new();
    for (alias, repository) in config.repositories() {
        repositories.push((
            alias.as_str().to_owned(),
            repository_table(repository)
                .map_err(|key| format!("repositories.{}.{key}", alias.as_str()))?,
        ));
    }
    Ok(FileTable {
        schema: Some(schema),
        agent: agent(config.agent()),
        hooks: hooks(config.hooks()),
        repositories: (!repositories.is_empty()).then_some(Repositories(repositories)),
        plans: (!config.plans().is_empty()).then(|| config.plans().iter().map(plan).collect()),
        notifications: notifications(config.notifications())?,
    })
}

/// The `[notifications]` tables, unless there are none, or the key of the
/// setting that can't be written.
fn notifications(notifications: &Notifications) -> Result<Option<NotificationsTable>, String> {
    if notifications.is_empty() {
        return Ok(None);
    }
    let mut channels = std::collections::BTreeMap::new();
    for (name, channel) in notifications.channels() {
        let table = channel_table(channel)
            .map_err(|key| format!("notifications.channels.{}.{key}", name.as_str()))?;
        channels.insert(name.as_str().to_owned(), table);
    }
    let rules = notifications
        .rules()
        .iter()
        .map(|(name, rule)| (name.as_str().to_owned(), rule_table(rule)))
        .collect::<std::collections::BTreeMap<_, _>>();
    Ok(Some(NotificationsTable {
        channels: (!channels.is_empty()).then_some(channels),
        rules: (!rules.is_empty()).then_some(rules),
    }))
}

/// The four keys a channel's secret can be named by, `-command`,
/// `-keyring`, `-file` and `-env`, with the one it is set; `None` for a
/// secret no such key can name: one written as text, or a whole file.
fn secret_keys(secret: &SecretRef) -> Option<[Option<String>; 4]> {
    Some(match secret {
        SecretRef::Command(command) => [Some(words(command)), None, None, None],
        SecretRef::Keyring(entry) => [None, Some(entry.to_string()), None, None],
        SecretRef::File {
            path,
            read: FileRead::FirstLine,
        } => [None, None, Some(path.clone()), None],
        SecretRef::Env(name) => [None, None, None, Some(name.clone())],
        SecretRef::Inline(_)
        | SecretRef::File {
            read: FileRead::Whole,
            ..
        } => return None,
    })
}

/// The channel's table, or the key of the setting that can't be written.
fn channel_table(channel: &Channel) -> Result<ChannelTable, &'static str> {
    let [url_command, url_keyring, url_file, url_env] = secret_keys(channel.url()).ok_or("url")?;
    let mut table = ChannelTable {
        kind: Some(channel.kind().name().to_owned()),
        url_command,
        url_keyring,
        url_file,
        url_env,
        ..ChannelTable::default()
    };
    match channel.auth() {
        None => {}
        Some(Auth::Bearer(token)) => {
            let [command, keyring, file, env] = secret_keys(token).ok_or("token")?;
            table.token_command = command;
            table.token_keyring = keyring;
            table.token_file = file;
            table.token_env = env;
        }
        Some(Auth::Basic { username, password }) => {
            let [command, keyring, file, env] = secret_keys(password).ok_or("password")?;
            table.username = Some(username.as_str().to_owned());
            table.password_command = command;
            table.password_keyring = keyring;
            table.password_file = file;
            table.password_env = env;
        }
    }
    Ok(table)
}

fn rule_table(rule: &Rule) -> RuleTable {
    RuleTable {
        events: Some(
            rule.events()
                .iter()
                .map(|event| event.name().to_owned())
                .collect(),
        ),
        channel: Some(rule.channel().as_str().to_owned()),
        plans: (!rule.plans().is_empty()).then(|| {
            rule.plans()
                .iter()
                .map(|plan| plan.as_str().to_owned())
                .collect()
        }),
        template: rule.template().map(|template| template.as_str().to_owned()),
    }
}

/// The error for a setting the format can't say.
pub(crate) fn unwritable(file: &str, key: String) -> ConfigError {
    ConfigError::Invalid {
        file: file.to_owned(),
        key: Some(key),
        at: None,
        problem: Problem::Unwritable,
        detail: String::new(),
    }
}

fn agent(settings: &AgentSettings) -> Option<AgentTable> {
    if settings == &AgentSettings::default() {
        return None;
    }
    Some(AgentTable {
        hostname: settings
            .hostname
            .as_ref()
            .map(|name| name.as_str().to_owned()),
        concurrency: (settings.concurrency != AgentSettings::default().concurrency)
            .then(|| i64::from(settings.concurrency.get())),
        nice: settings.nice.map(|nice| i64::from(nice.get())),
        allow_hub_restore: settings.allow_hub_restore.then_some(true),
        locale: settings
            .locale
            .as_ref()
            .map(|locale| locale.as_str().to_owned()),
    })
}

fn hooks(hooks: &Hooks) -> Option<HooksTable> {
    if hooks.is_empty() {
        return None;
    }
    let list =
        |entries: &[HookEntry]| (!entries.is_empty()).then(|| entries.iter().map(hook).collect());
    Some(HooksTable {
        run_before: list(&hooks.run_before),
        run_after: list(&hooks.run_after),
        run_failed: list(&hooks.run_failed),
        run_finally: list(&hooks.run_finally),
    })
}

/// A command line as the one string of shell words it reads back from.
fn words(command: &domain::secrets::CommandLine) -> String {
    shell_words::join(
        std::iter::once(command.program()).chain(command.args().iter().map(String::as_str)),
    )
}

/// A hook is a string unless it says what to do when it fails.
fn hook(entry: &HookEntry) -> HookValue {
    if entry.on_failure == domain::planning::OnFailure::default() {
        return HookValue::Text(words(&entry.command));
    }
    HookValue::Table(HookEntryTable {
        command: Some(entry.command.program().to_owned()),
        args: some_if_any(entry.command.args()),
        on_failure: Some(entry.on_failure.as_str().to_owned()),
    })
}

/// The repository's table, or the key of the setting that can't be written.
fn repository_table(repository: &RepositoryConfig) -> Result<RepositoryTable, &'static str> {
    let settings = &repository.settings;
    let mut table = RepositoryTable {
        repository: Some(settings.repository.clone()),
        repository_id: repository.id.as_ref().map(|id| id.as_str().to_owned()),
        repo_hot: settings.repo_hot.clone(),
        maintenance: (repository.maintenance != Maintenance::default())
            .then(|| repository.maintenance.as_str().to_owned()),
        group_by: (repository.group_by != GroupBy::default())
            .then(|| repository.group_by.to_string()),
        options: (!settings.options.is_empty()).then(|| settings.options.clone()),
        options_hot: (!settings.options_hot.is_empty()).then(|| settings.options_hot.clone()),
        options_cold: (!settings.options_cold.is_empty()).then(|| settings.options_cold.clone()),
        hooks: hooks(&repository.hooks),
        locations: (!repository.locations.further.is_empty()).then(|| {
            repository
                .locations
                .further
                .iter()
                .map(location_table)
                .collect()
        }),
        ..RepositoryTable::default()
    };
    match &settings.password {
        SecretRef::Inline(value) => table.password = Some(value.expose().to_owned()),
        SecretRef::Command(command) => table.password_command = Some(words(command)),
        SecretRef::Keyring(entry) => table.password_keyring = Some(entry.to_string()),
        SecretRef::File {
            path,
            read: FileRead::FirstLine,
        } => table.password_file = Some(path.clone()),
        // `password-file` reads one line: a whole file is what `${file:…}` is for.
        SecretRef::File {
            read: FileRead::Whole,
            ..
        } => return Err("password"),
        SecretRef::Env(name) => table.password_env = Some(name.clone()),
    }
    Ok(table)
}

/// A location after the first, as its `[[repositories.<alias>.locations]]`
/// table writes it (decision 80).
fn location_table(location: &Location) -> LocationTable {
    LocationTable {
        repository: Some(location.repository.clone()),
        repo_hot: location.repo_hot.clone(),
        options: (!location.options.is_empty()).then(|| location.options.clone()),
        options_hot: (!location.options_hot.is_empty()).then(|| location.options_hot.clone()),
        options_cold: (!location.options_cold.is_empty()).then(|| location.options_cold.clone()),
    }
}

fn plan(plan: &Plan) -> PlanTable {
    let filter = &plan.filter;
    PlanTable {
        id: Some(plan.id.as_str().to_owned()),
        repository: Some(plan.repository.as_str().to_owned()),
        host: plan.host.as_ref().map(|host| host.as_str().to_owned()),
        label: plan.label.clone(),
        sources: Some(plan.sources.iter().map(|s| s.as_str().to_owned()).collect()),
        tags: (!plan.tags.is_empty()).then(|| {
            plan.tags
                .iter()
                .map(|tag| tag.as_str().to_owned())
                .collect()
        }),
        globs: some_if_any(&filter.globs),
        iglobs: some_if_any(&filter.iglobs),
        glob_files: glob_files(&filter.glob_files),
        iglob_files: glob_files(&filter.iglob_files),
        git_ignore: filter.git_ignore.then_some(true),
        no_require_git: filter.no_require_git.then_some(true),
        custom_ignorefiles: some_if_any(&filter.custom_ignorefiles),
        exclude_if_present: some_if_any(&filter.exclude_if_present),
        exclude_if_xattr: some_if_any(&filter.exclude_if_xattr),
        one_file_system: filter.one_file_system.then_some(true),
        schedule: plan.schedule.as_ref().map(schedule),
        overlap: (plan.overlap != Overlap::Skip).then(|| plan.overlap.as_str().to_owned()),
        retry: plan.retry.as_ref().map(retry),
        hooks: hooks(&plan.hooks),
        retention: retention(&plan.retention),
    }
}

fn schedule(schedule: &Schedule) -> ScheduleTable {
    ScheduleTable {
        cron: schedule.cron().map(|cron| cron.as_str().to_owned()),
        every: match schedule.cadence() {
            Cadence::Every(every) => Some(format_duration(every.get())),
            Cadence::Cron(_) => None,
        },
        timezone: schedule.zone().map(|zone| zone.name().to_owned()),
        jitter: schedule.jitter().map(format_duration),
        catch_up: (!schedule.catch_up()).then_some(false),
        on_ac_power: schedule.on_ac_power().then_some(true),
    }
}

fn retry(retry: &RetryPolicy) -> RetryTable {
    RetryTable {
        attempts: Some(i64::from(retry.attempts())),
        backoff: Some(format_duration(retry.backoff())),
    }
}

fn retention(policy: &RetentionPolicy) -> Option<RetentionTable> {
    if policy.is_empty() {
        return None;
    }
    let count = |count: Option<KeepCount>| count.map(|count| i64::from(count.get()));
    let within = |within: &Option<KeepWithin>| within.as_ref().map(ToString::to_string);
    Some(RetentionTable {
        keep_last: count(policy.keep_last),
        keep_minutely: count(policy.keep_minutely),
        keep_hourly: count(policy.keep_hourly),
        keep_daily: count(policy.keep_daily),
        keep_weekly: count(policy.keep_weekly),
        keep_monthly: count(policy.keep_monthly),
        keep_quarter_yearly: count(policy.keep_quarter_yearly),
        keep_half_yearly: count(policy.keep_half_yearly),
        keep_yearly: count(policy.keep_yearly),
        keep_within: within(&policy.keep_within),
        keep_within_minutely: within(&policy.keep_within_minutely),
        keep_within_hourly: within(&policy.keep_within_hourly),
        keep_within_daily: within(&policy.keep_within_daily),
        keep_within_weekly: within(&policy.keep_within_weekly),
        keep_within_monthly: within(&policy.keep_within_monthly),
        keep_within_quarter_yearly: within(&policy.keep_within_quarter_yearly),
        keep_within_half_yearly: within(&policy.keep_within_half_yearly),
        keep_within_yearly: within(&policy.keep_within_yearly),
    })
}
