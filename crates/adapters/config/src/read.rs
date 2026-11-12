//! From the tables of a file to the domain's types, each value checked by the
//! type that knows what it may be. Every refusal names the key and, when the
//! text is the file's own, the place; none of them holds a value of the file.

use std::collections::BTreeMap;

use app::ports::{CadenceProblem, ConfigError, PasswordProblem, Problem};
use domain::execution::plan_tags::UserTag;
use domain::planning::{
    AgentConfig, AgentSettings, Cadence, Concurrency, CronExpression, GlobFile, GroupBy, HookEntry,
    Hooks, Hostname, Interval, InvalidValue, KeepCount, KeepWithin, Locale, Maintenance, Nice,
    OnFailure, Overlap, Plan, RepositoryConfig, RepositoryField, RepositorySettings,
    RetentionPolicy, RetryPolicy, Schedule, Source, SourceFilter, ValueKind, Violation,
    check_location, check_option_value, check_options, parse_duration,
};
use domain::secrets::{CommandLine, FileRead, KeyringEntry, SecretRef, SecretValue};
use domain::{InvalidId, PlanId, RepositoryAlias, RepositoryId};

use crate::locate::{Doc, Path, key_span, position, value_span};
use crate::tables::{
    AgentTable, FileTable, HookEntryTable, HookValue, HooksTable, PlanTable, RepositoryTable,
    RetentionTable, RetryTable, ScheduleTable,
};
use crate::zone::schedule_zone;

/// Where an error is: which file, and which text the places refer to.
pub(crate) struct Reader<'a, 'i> {
    pub file: &'a str,
    pub text: &'a str,
    pub doc: &'a Doc<'i>,
    /// Whether `text` is what the user wrote. A migrated file's is not, and a
    /// line of it would point at something else.
    pub located: bool,
}

/// Whether an error is at the key or at the value.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Spot {
    Key,
    Value,
}

type Read<T> = Result<T, ConfigError>;

impl Reader<'_, '_> {
    pub(crate) fn error(
        &self,
        path: &Path,
        spot: Spot,
        problem: Problem,
        detail: String,
    ) -> ConfigError {
        let span = match spot {
            Spot::Key => key_span(self.doc, path),
            Spot::Value => value_span(self.doc, path),
        };
        ConfigError::Invalid {
            file: self.file.to_owned(),
            key: path.text(),
            at: span
                .filter(|_| self.located)
                .map(|span| position(self.text, span.start)),
            problem,
            detail,
        }
    }

    fn refuse(&self, path: &Path, problem: Problem) -> ConfigError {
        self.error(path, Spot::Value, problem, String::new())
    }

    /// A value the model refuses, at the value.
    fn value<T>(&self, path: &Path, result: Result<T, InvalidValue>) -> Read<T> {
        result.map_err(|invalid| self.refuse(path, Problem::Value(invalid.kind)))
    }

    fn id<T>(&self, path: &Path, result: Result<T, InvalidId>) -> Read<T> {
        result.map_err(|invalid| self.refuse(path, Problem::Id(invalid.kind)))
    }

    /// A key that has to be there, which is missing from the table at `table`.
    fn required<T>(&self, table: &Path, key: &str, found: Option<T>) -> Read<T> {
        found.ok_or_else(|| {
            let mut error = self.refuse(table, Problem::MissingKey);
            if let ConfigError::Invalid { key: at, .. } = &mut error {
                *at = table.key(key).text();
            }
            error
        })
    }
}

/// The configuration the tables hold.
pub(crate) fn config(reader: &Reader<'_, '_>, file: FileTable) -> Read<AgentConfig> {
    let root = Path::default();
    let agent = agent(reader, &root.key("agent"), file.agent.unwrap_or_default())?;
    let hooks = hooks(reader, &root.key("hooks"), file.hooks.unwrap_or_default())?;
    let mut repositories = BTreeMap::new();
    for (name, table) in file.repositories.map(|named| named.0).unwrap_or_default() {
        let path = root.key("repositories").key(&name);
        let alias = RepositoryAlias::parse(&name).map_err(|invalid| {
            reader.error(&path, Spot::Key, Problem::Id(invalid.kind), String::new())
        })?;
        repositories.insert(alias, repository(reader, &path, table)?);
    }
    let mut plans = Vec::new();
    for (index, table) in file.plans.unwrap_or_default().into_iter().enumerate() {
        plans.push(plan(reader, &root.key("plans").index(index), table)?);
    }
    AgentConfig::new(agent, hooks, repositories, plans)
        .map_err(|violation| violated(reader, &root, violation))
}

/// A violation, at the key that the rule is about: a plan's, or the second
/// of two entries that are one repository, with both of them named.
fn violated(reader: &Reader<'_, '_>, root: &Path, violation: Violation) -> ConfigError {
    let (index, key) = match &violation {
        Violation::DuplicatePlan { index, .. } => (*index, "id"),
        Violation::UnknownRepository { index, .. } => (*index, "repository"),
        Violation::NoSources { index, .. } => (*index, "sources"),
        Violation::SameRepository { first, second } => {
            let entry = |alias: &RepositoryAlias| root.key("repositories").key(alias.as_str());
            let detail = format!(
                "{} and {} are one repository: keep one entry and point its plans at it",
                entry(first).text().unwrap_or_default(),
                entry(second).text().unwrap_or_default(),
            );
            let path = entry(second);
            return reader.error(&path, Spot::Key, Problem::Violation(violation), detail);
        }
    };
    let path = root.key("plans").index(index).key(key);
    reader.refuse(&path, Problem::Violation(violation))
}

fn agent(reader: &Reader<'_, '_>, path: &Path, table: AgentTable) -> Read<AgentSettings> {
    let hostname = table
        .hostname
        .map(|text| reader.value(&path.key("hostname"), Hostname::parse(&text)))
        .transpose()?;
    let concurrency = match table.concurrency {
        Some(count) => reader.value(&path.key("concurrency"), Concurrency::new(count))?,
        None => Concurrency::default(),
    };
    let nice = table
        .nice
        .map(|nice| reader.value(&path.key("nice"), Nice::new(nice)))
        .transpose()?;
    let locale = table
        .locale
        .map(|text| reader.value(&path.key("locale"), Locale::parse(&text)))
        .transpose()?;
    Ok(AgentSettings {
        hostname,
        concurrency,
        nice,
        allow_hub_restore: table.allow_hub_restore.unwrap_or(false),
        locale,
    })
}

fn hooks(reader: &Reader<'_, '_>, path: &Path, table: HooksTable) -> Read<Hooks> {
    let list = |key: &str, entries: Option<Vec<HookValue>>| -> Read<Vec<HookEntry>> {
        let path = path.key(key);
        entries
            .unwrap_or_default()
            .into_iter()
            .enumerate()
            .map(|(index, entry)| hook(reader, &path.index(index), entry))
            .collect()
    };
    Ok(Hooks {
        run_before: list("run-before", table.run_before)?,
        run_after: list("run-after", table.run_after)?,
        run_failed: list("run-failed", table.run_failed)?,
        run_finally: list("run-finally", table.run_finally)?,
    })
}

fn hook(reader: &Reader<'_, '_>, path: &Path, entry: HookValue) -> Read<HookEntry> {
    match entry {
        HookValue::Text(text) => reader.value(path, HookEntry::parse(&text)),
        HookValue::Table(HookEntryTable {
            command,
            args,
            on_failure,
        }) => {
            let command = reader.required(path, "command", command)?;
            let on_failure = match on_failure {
                Some(text) => reader.value(&path.key("on-failure"), OnFailure::parse(&text))?,
                None => OnFailure::default(),
            };
            reader.value(
                &path.key("command"),
                HookEntry::new(&command, args.unwrap_or_default(), on_failure),
            )
        }
    }
}

fn repository(
    reader: &Reader<'_, '_>,
    path: &Path,
    table: RepositoryTable,
) -> Read<RepositoryConfig> {
    let RepositoryTable {
        repository,
        repository_id,
        repo_hot,
        password,
        password_command,
        password_keyring,
        password_file,
        password_env,
        maintenance,
        group_by,
        options,
        options_hot,
        options_cold,
        hooks: hooks_table,
    } = table;
    let repository = reader.required(path, "repository", repository)?;
    reader.value(&path.key("repository"), check_location(&repository))?;
    if let Some(hot) = &repo_hot {
        reader.value(&path.key("repo-hot"), check_location(hot))?;
    }
    for (key, table) in [
        ("options", &options),
        ("options-hot", &options_hot),
        ("options-cold", &options_cold),
    ] {
        if let Some(table) = table {
            option_table(reader, &path.key(key), table)?;
        }
    }
    let id = repository_id
        .map(|text| reader.id(&path.key("repository-id"), RepositoryId::parse(&text)))
        .transpose()?;
    let password = password_ref(
        reader,
        path,
        [
            password,
            password_command,
            password_keyring,
            password_file,
            password_env,
        ],
    )?;
    let maintenance = match maintenance {
        Some(text) => reader.value(&path.key("maintenance"), Maintenance::parse(&text))?,
        None => Maintenance::default(),
    };
    let group_by = match group_by {
        Some(text) => reader.value(&path.key("group-by"), GroupBy::parse(&text))?,
        None => GroupBy::default(),
    };
    let settings = RepositorySettings {
        repository,
        repo_hot,
        password,
        options: options.unwrap_or_default(),
        options_hot: options_hot.unwrap_or_default(),
        options_cold: options_cold.unwrap_or_default(),
    };
    if let Some(field) = settings.relative_path() {
        let (path, problem) = match field {
            RepositoryField::Repository => (path.key("repository"), Problem::Value(LOCAL_PATH)),
            RepositoryField::RepoHot => (path.key("repo-hot"), Problem::Value(LOCAL_PATH)),
            RepositoryField::PasswordCommand => (
                path.key("password-command"),
                Problem::Password(PasswordProblem::RelativeProgram),
            ),
            RepositoryField::Option(name) => {
                (path.key("options").key(&name), Problem::Value(LOCAL_PATH))
            }
            RepositoryField::OptionHot(name) => (
                path.key("options-hot").key(&name),
                Problem::Value(LOCAL_PATH),
            ),
            RepositoryField::OptionCold(name) => (
                path.key("options-cold").key(&name),
                Problem::Value(LOCAL_PATH),
            ),
        };
        return Err(reader.refuse(&path, problem));
    }
    Ok(RepositoryConfig {
        id,
        settings,
        maintenance,
        group_by,
        hooks: hooks(reader, &path.key("hooks"), hooks_table.unwrap_or_default())?,
    })
}

/// What a relative path a repository's settings name is refused as.
const LOCAL_PATH: ValueKind = ValueKind::LocalPath;

/// A table of options a job can take, refused at the table when it holds too
/// many and at the value when one is too long (`check_options`,
/// `check_option_value`).
fn option_table(
    reader: &Reader<'_, '_>,
    path: &Path,
    table: &BTreeMap<String, String>,
) -> Read<()> {
    reader.value(path, check_options(table))?;
    for (name, value) in table {
        reader.value(&path.key(name), check_option_value(value))?;
    }
    Ok(())
}

/// The five ways to give a password, in the order of the keys:
/// `password`, `password-command`, `password-keyring`, `password-file`,
/// `password-env`. Exactly one is there.
fn password_ref(
    reader: &Reader<'_, '_>,
    path: &Path,
    given: [Option<String>; 5],
) -> Read<SecretRef> {
    let password = |problem| reader.refuse(path, Problem::Password(problem));
    let mut present = given
        .into_iter()
        .enumerate()
        .filter_map(|(at, text)| text.map(|text| (at, text)));
    let Some((at, text)) = present.next() else {
        return Err(password(PasswordProblem::Missing));
    };
    if present.next().is_some() {
        return Err(password(PasswordProblem::Several));
    }
    let at_key = |key: &str, problem| reader.refuse(&path.key(key), Problem::Password(problem));
    match at {
        0 => Ok(SecretRef::Inline(SecretValue::new(text))),
        1 => CommandLine::parse(&text)
            .map(SecretRef::Command)
            .map_err(|_| at_key("password-command", PasswordProblem::BadCommand)),
        2 => KeyringEntry::parse(&text)
            .map(SecretRef::Keyring)
            .map_err(|_| at_key("password-keyring", PasswordProblem::NotAKeyringEntry)),
        3 if text.starts_with('/') => Ok(SecretRef::File {
            path: text,
            read: FileRead::FirstLine,
        }),
        3 => Err(at_key("password-file", PasswordProblem::RelativeFile)),
        _ => Ok(SecretRef::Env(text)),
    }
}

fn plan(reader: &Reader<'_, '_>, path: &Path, table: PlanTable) -> Read<Plan> {
    let id = reader.required(path, "id", table.id)?;
    let id = reader.id(&path.key("id"), PlanId::parse(&id))?;
    let repository = reader.required(path, "repository", table.repository)?;
    let repository = reader.id(&path.key("repository"), RepositoryAlias::parse(&repository))?;
    let host = table
        .host
        .map(|text| reader.value(&path.key("host"), Hostname::parse(&text)))
        .transpose()?;
    let sources_path = path.key("sources");
    let sources = reader
        .required(path, "sources", table.sources)?
        .iter()
        .enumerate()
        .map(|(index, text)| reader.value(&sources_path.index(index), Source::parse(text)))
        .collect::<Read<Vec<_>>>()?;
    let tags_path = path.key("tags");
    let tags = table
        .tags
        .unwrap_or_default()
        .into_iter()
        .enumerate()
        .map(|(index, tag)| {
            UserTag::new(tag)
                .map_err(|_| reader.refuse(&tags_path.index(index), Problem::ReservedTag))
        })
        .collect::<Read<Vec<_>>>()?;
    let schedule = table
        .schedule
        .map(|schedule| schedule_of(reader, &path.key("schedule"), schedule))
        .transpose()?;
    let overlap = table
        .overlap
        .map(|text| reader.value(&path.key("overlap"), Overlap::parse(&text)))
        .transpose()?
        .unwrap_or_default();
    let retry = table
        .retry
        .map(|retry| retry_of(reader, &path.key("retry"), retry))
        .transpose()?;
    let filter = SourceFilter {
        globs: table.globs.unwrap_or_default(),
        iglobs: table.iglobs.unwrap_or_default(),
        glob_files: glob_files(reader, &path.key("glob-files"), table.glob_files)?,
        iglob_files: glob_files(reader, &path.key("iglob-files"), table.iglob_files)?,
        git_ignore: table.git_ignore.unwrap_or(false),
        no_require_git: table.no_require_git.unwrap_or(false),
        custom_ignorefiles: table.custom_ignorefiles.unwrap_or_default(),
        exclude_if_present: table.exclude_if_present.unwrap_or_default(),
        exclude_if_xattr: table.exclude_if_xattr.unwrap_or_default(),
        one_file_system: table.one_file_system.unwrap_or(false),
    };
    let hooks = hooks(reader, &path.key("hooks"), table.hooks.unwrap_or_default())?;
    let retention = retention(
        reader,
        &path.key("retention"),
        table.retention.unwrap_or_default(),
    )?;
    Ok(Plan {
        id,
        repository,
        host,
        label: table.label,
        sources,
        tags,
        schedule,
        overlap,
        retry,
        filter,
        retention,
        hooks,
    })
}

/// The files of glob patterns listed at `path`, each refused at its own index.
fn glob_files(
    reader: &Reader<'_, '_>,
    path: &Path,
    files: Option<Vec<String>>,
) -> Read<Vec<GlobFile>> {
    files
        .unwrap_or_default()
        .iter()
        .enumerate()
        .map(|(index, text)| reader.value(&path.index(index), GlobFile::parse(text)))
        .collect()
}

fn schedule_of(reader: &Reader<'_, '_>, path: &Path, table: ScheduleTable) -> Read<Schedule> {
    let cadence = match (table.cron, table.every) {
        (Some(cron), None) => {
            Cadence::Cron(reader.value(&path.key("cron"), CronExpression::parse(&cron))?)
        }
        (None, Some(every)) => {
            Cadence::Every(reader.value(&path.key("every"), Interval::parse(&every))?)
        }
        (None, None) => return Err(reader.refuse(path, Problem::Cadence(CadenceProblem::Missing))),
        (Some(_), Some(_)) => {
            return Err(reader.refuse(path, Problem::Cadence(CadenceProblem::Several)));
        }
    };
    let mut schedule = Schedule::new(cadence)
        .with_catch_up(table.catch_up.unwrap_or(true))
        .with_on_ac_power(table.on_ac_power.unwrap_or(false));
    if let Some(name) = table.timezone {
        // The system's copy of the time zone database, or the one built in
        // where the system has none.
        let zone = schedule_zone(&name, jiff::tz::db());
        schedule = schedule.with_zone(reader.value(&path.key("timezone"), zone)?);
    }
    if let Some(text) = table.jitter {
        let jitter_path = path.key("jitter");
        let jitter = reader.value(&jitter_path, parse_duration(&text))?;
        schedule = reader.value(&jitter_path, schedule.with_jitter(jitter))?;
    }
    Ok(schedule)
}

fn retry_of(reader: &Reader<'_, '_>, path: &Path, table: RetryTable) -> Read<RetryPolicy> {
    let attempts = reader.required(path, "attempts", table.attempts)?;
    let backoff = reader.required(path, "backoff", table.backoff)?;
    let backoff = reader.value(&path.key("backoff"), parse_duration(&backoff))?;
    RetryPolicy::new(attempts, backoff).map_err(|invalid| {
        // Only the attempts can be wrong here: the backoff was a duration, and
        // its limits are the policy's.
        let key = match invalid.kind {
            domain::planning::ValueKind::Attempts => "attempts",
            _ => "backoff",
        };
        reader.refuse(&path.key(key), Problem::Value(invalid.kind))
    })
}

fn retention(reader: &Reader<'_, '_>, path: &Path, table: RetentionTable) -> Read<RetentionPolicy> {
    let count = |key: &str, count: Option<i64>| -> Read<Option<KeepCount>> {
        count
            .map(|count| reader.value(&path.key(key), KeepCount::new(count)))
            .transpose()
    };
    let within = |key: &str, text: Option<String>| -> Read<Option<KeepWithin>> {
        text.map(|text| reader.value(&path.key(key), KeepWithin::parse(&text)))
            .transpose()
    };
    Ok(RetentionPolicy {
        keep_last: count("keep-last", table.keep_last)?,
        keep_minutely: count("keep-minutely", table.keep_minutely)?,
        keep_hourly: count("keep-hourly", table.keep_hourly)?,
        keep_daily: count("keep-daily", table.keep_daily)?,
        keep_weekly: count("keep-weekly", table.keep_weekly)?,
        keep_monthly: count("keep-monthly", table.keep_monthly)?,
        keep_quarter_yearly: count("keep-quarter-yearly", table.keep_quarter_yearly)?,
        keep_half_yearly: count("keep-half-yearly", table.keep_half_yearly)?,
        keep_yearly: count("keep-yearly", table.keep_yearly)?,
        keep_within: within("keep-within", table.keep_within)?,
        keep_within_minutely: within("keep-within-minutely", table.keep_within_minutely)?,
        keep_within_hourly: within("keep-within-hourly", table.keep_within_hourly)?,
        keep_within_daily: within("keep-within-daily", table.keep_within_daily)?,
        keep_within_weekly: within("keep-within-weekly", table.keep_within_weekly)?,
        keep_within_monthly: within("keep-within-monthly", table.keep_within_monthly)?,
        keep_within_quarter_yearly: within(
            "keep-within-quarter-yearly",
            table.keep_within_quarter_yearly,
        )?,
        keep_within_half_yearly: within("keep-within-half-yearly", table.keep_within_half_yearly)?,
        keep_within_yearly: within("keep-within-yearly", table.keep_within_yearly)?,
    })
}
