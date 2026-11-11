//! One profile's text into a [`Part`]: the syntax, then the keys of every
//! table against what rustic 0.11.4 accepts there, then the values, each
//! checked by the type that knows what it may be. Every refusal names the file
//! and the key and, when it can, the place; none of them holds a value of the
//! file, which may hold a secret.

use std::ops::Range;

use app::ports::{Position, ProfileError, ProfileProblem};
use domain::execution::plan_tags::{AGENT_TAG_PREFIX, PLAN_TAG_PREFIX};
use domain::planning::{
    GlobFile, GroupBy, HookEntry, Hooks, Hostname, InvalidValue, KeepCount, KeepWithin, OnFailure,
    RetentionPolicy, Source,
};
use domain::secrets::CommandLine;
use serde::Deserialize;
use toml::de::{DeTable, Deserializer};

use crate::keys;
use crate::locate::{
    Doc, Path, items_at, key_span, keys as keys_of, path_of, plain, position, scrub, table_at,
    texts_at, value_span,
};
use crate::part::{
    Entry, Forget, Global, Located, Part, Repository, Settings, Use, sort_and_dedup,
};
use crate::tables::{
    BackupTable, CommandTable, CommandValue, FileTable, ForgetTable, GlobalTable, HooksTable,
    RepositoryTable,
};
use crate::{MAX_ENTRIES, MAX_LEFT_OUT_KEYS, MAX_PROFILES};

type Read<T> = Result<T, ProfileError>;

/// Whether an error is at the key or at the value.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Spot {
    Key,
    Value,
}

/// Where an error is: which file, and which text the places refer to.
struct Reader<'a, 'i> {
    file: &'a str,
    text: &'a str,
    doc: &'a Doc<'i>,
}

impl Reader<'_, '_> {
    fn at(&self, path: &Path, spot: Spot) -> Option<Position> {
        let span: Option<Range<usize>> = match spot {
            Spot::Key => key_span(self.doc, path),
            Spot::Value => value_span(self.doc, path),
        };
        span.map(|span| position(self.text, span.start))
    }

    fn invalid(
        &self,
        path: &Path,
        spot: Spot,
        problem: ProfileProblem,
        detail: String,
    ) -> ProfileError {
        ProfileError::Invalid {
            file: self.file.to_owned(),
            key: path.text(),
            at: self.at(path, spot),
            problem,
            detail,
        }
    }

    fn refuse(&self, path: &Path, problem: ProfileProblem) -> ProfileError {
        self.invalid(path, Spot::Value, problem, String::new())
    }

    /// A key rustic accepts in the profile and Ladderback can't honour.
    fn unsupported(&self, path: &Path, spot: Spot) -> ProfileError {
        ProfileError::Unsupported {
            file: self.file.to_owned(),
            key: path.text().unwrap_or_default(),
            at: self.at(path, spot),
        }
    }

    /// A value the domain refuses, at the value.
    fn value<T>(&self, path: &Path, result: Result<T, InvalidValue>) -> Read<T> {
        result.map_err(|_| self.refuse(path, ProfileProblem::BadValue))
    }

    /// A key that has to be there, which is missing from the table at
    /// `table`: the error is at the table.
    fn missing(&self, table: &Path, key: &str) -> ProfileError {
        let mut error = self.refuse(table, ProfileProblem::Missing);
        if let ProfileError::Invalid { key: at, .. } = &mut error {
            *at = table.key(key).text();
        }
        error
    }

    fn unknown(&self, path: &Path) -> ProfileError {
        self.invalid(path, Spot::Key, ProfileProblem::UnknownKey, String::new())
    }

    /// Notes that `key`, written at `path`, is left out, once however often
    /// it is met. A profile that leaves out more different keys than
    /// [`MAX_LEFT_OUT_KEYS`] is refused at the first one over.
    fn note(&self, path: &Path, left_out: &mut Vec<String>, key: String) -> Read<()> {
        if left_out.contains(&key) {
            return Ok(());
        }
        if left_out.len() >= MAX_LEFT_OUT_KEYS {
            return Err(self.invalid(path, Spot::Key, ProfileProblem::TooManyKeys, String::new()));
        }
        left_out.push(key);
        Ok(())
    }
}

/// The text of a profile, as the [`Part`] it holds.
pub(crate) fn parse(file: &str, text: &str) -> Read<Part> {
    let doc = DeTable::parse(text).map_err(|error| ProfileError::Invalid {
        file: file.to_owned(),
        key: None,
        at: error.span().map(|span| position(text, span.start)),
        problem: ProfileProblem::Syntax,
        detail: scrub(error.message()),
    })?;
    let reader = Reader {
        file,
        text,
        doc: &doc,
    };
    let mut left_out = Vec::new();
    check(&reader, &mut left_out)?;
    let table = FileTable::deserialize(Deserializer::from(doc.clone()))
        .map_err(|error| wrong_type(&reader, &error))?;
    build(&reader, table, left_out)
}

/// A table that doesn't have the shape the key takes.
fn wrong_type(reader: &Reader<'_, '_>, error: &toml::de::Error) -> ProfileError {
    let found = error.span().and_then(|span| path_of(reader.doc, &span));
    let (path, spot) = found.map_or((Path::default(), Spot::Value), |(path, is_key)| {
        (path, if is_key { Spot::Key } else { Spot::Value })
    });
    reader.invalid(
        &path,
        spot,
        ProfileProblem::WrongType,
        scrub(error.message()),
    )
}

// --- Keys -------------------------------------------------------------------

/// Every key of the profile against what rustic accepts in its table: a key
/// in no list is refused where it is written, a key that is only left out is
/// noted in `left_out`. A value that is not a table where one is wanted is
/// left for the reader of the values to refuse.
fn check(reader: &Reader<'_, '_>, left_out: &mut Vec<String>) -> Read<()> {
    let root = Path::default();
    let Some(top) = table_at(reader.doc, &root) else {
        return Ok(());
    };
    for (key, _) in keys_of(top) {
        let path = root.key(key);
        if !keys::TOP_LEVEL.contains(&key) {
            return Err(reader.unknown(&path));
        }
        match key {
            "global" => check_global(reader, &path, left_out)?,
            "repository" => check_repository(reader, &path, left_out)?,
            "backup" => check_backup(reader, &path, left_out)?,
            "forget" => check_forget(reader, &path, left_out)?,
            other => check_left_out_section(reader, &path, other, left_out)?,
        }
    }
    Ok(())
}

/// A section for another purpose: one notice, when it holds anything.
fn check_left_out_section(
    reader: &Reader<'_, '_>,
    path: &Path,
    name: &str,
    left_out: &mut Vec<String>,
) -> Read<()> {
    let Some(table) = table_at(reader.doc, path) else {
        return Err(reader.invalid(
            path,
            Spot::Value,
            ProfileProblem::WrongType,
            "expected a table".to_owned(),
        ));
    };
    if name == "snapshot-filter" {
        strict_keys(reader, path, keys::SNAPSHOT_FILTER)?;
    }
    if keys_of(table).next().is_some() {
        reader.note(path, left_out, name.to_owned())?;
    }
    Ok(())
}

/// Refuses the first key of the table at `path` that is not one of `known`.
fn strict_keys(reader: &Reader<'_, '_>, path: &Path, known: &[&str]) -> Read<()> {
    let Some(table) = table_at(reader.doc, path) else {
        return Ok(());
    };
    for (key, _) in keys_of(table) {
        if !known.contains(&key) {
            return Err(reader.unknown(&path.key(key)));
        }
    }
    Ok(())
}

/// The keys of the table at `path`, in file order.
fn keys_at<'a>(reader: &'a Reader<'_, '_>, path: &Path) -> impl Iterator<Item = &'a str> + use<'a> {
    table_at(reader.doc, path)
        .into_iter()
        .flat_map(|table| keys_of(table).map(|(key, _)| key))
}

fn check_hooks(reader: &Reader<'_, '_>, path: &Path) -> Read<()> {
    strict_keys(reader, path, keys::HOOKS)
}

fn check_global(reader: &Reader<'_, '_>, path: &Path, left_out: &mut Vec<String>) -> Read<()> {
    strict_keys(reader, path, keys::GLOBAL)?;
    check_hooks(reader, &path.key("hooks"))?;
    let names = path.key("use-profiles");
    if items_at(reader.doc, &names).map_or(0, <[_]>::len) > MAX_PROFILES {
        return Err(reader.invalid(
            &names,
            Spot::Key,
            ProfileProblem::UsedTooMany,
            String::new(),
        ));
    }
    for key in keys_at(reader, path) {
        if !keys::GLOBAL_MAPPED.contains(&key) {
            reader.note(&path.key(key), left_out, format!("global.{}", plain(key)))?;
        }
    }
    Ok(())
}

/// `[repository]` is read leniently by rustic: a key it doesn't know is
/// ignored, so here it is only left out.
fn check_repository(reader: &Reader<'_, '_>, path: &Path, left_out: &mut Vec<String>) -> Read<()> {
    check_hooks(reader, &path.key("hooks"))?;
    for key in keys_at(reader, path) {
        if !keys::REPOSITORY_MAPPED.contains(&key) {
            reader.note(
                &path.key(key),
                left_out,
                format!("repository.{}", plain(key)),
            )?;
        }
    }
    Ok(())
}

fn check_backup(reader: &Reader<'_, '_>, path: &Path, left_out: &mut Vec<String>) -> Read<()> {
    strict_keys(reader, path, &keys::backup_known())?;
    for key in [keys::NAME, keys::SOURCES] {
        if keys_at(reader, path).any(|found| found == key) {
            return Err(reader.invalid(
                &path.key(key),
                Spot::Key,
                ProfileProblem::NotHere,
                String::new(),
            ));
        }
    }
    check_backup_keys(reader, path, "backup", left_out)?;

    let entries = path.key(keys::SNAPSHOTS);
    let count = items_at(reader.doc, &entries).map_or(0, <[_]>::len);
    if count > MAX_ENTRIES {
        return Err(reader.invalid(
            &entries,
            Spot::Key,
            ProfileProblem::TooManyEntries,
            String::new(),
        ));
    }
    for index in 0..count {
        let entry = entries.index(index);
        strict_keys(reader, &entry, &keys::backup_known())?;
        if keys_at(reader, &entry).any(|found| found == keys::SNAPSHOTS) {
            return Err(reader.invalid(
                &entry.key(keys::SNAPSHOTS),
                Spot::Key,
                ProfileProblem::NotHere,
                String::new(),
            ));
        }
        check_backup_keys(reader, &entry, "backup.snapshots", left_out)?;
    }
    Ok(())
}

/// The keys of `[backup]` or of an entry that are neither refused nor given
/// to the import, noted as `prefix.key`. `name` and `sources` are an entry's
/// own, and are not settings.
fn check_backup_keys(
    reader: &Reader<'_, '_>,
    path: &Path,
    prefix: &str,
    left_out: &mut Vec<String>,
) -> Read<()> {
    check_hooks(reader, &path.key("hooks"))?;
    for key in keys_at(reader, path) {
        if key == keys::STDIN_COMMAND {
            return Err(reader.unsupported(&path.key(key), Spot::Key));
        }
        let own = [keys::NAME, keys::SOURCES, keys::SNAPSHOTS].contains(&key);
        if !own && !keys::backup_mapped(key) {
            reader.note(&path.key(key), left_out, format!("{prefix}.{}", plain(key)))?;
        }
    }
    Ok(())
}

fn check_forget(reader: &Reader<'_, '_>, path: &Path, left_out: &mut Vec<String>) -> Read<()> {
    strict_keys(reader, path, &keys::forget_known())?;
    for key in keys_at(reader, path) {
        if !keys::forget_mapped(key) {
            reader.note(&path.key(key), left_out, format!("forget.{}", plain(key)))?;
        }
    }
    Ok(())
}

// --- Values -----------------------------------------------------------------

fn build(reader: &Reader<'_, '_>, table: FileTable, left_out: Vec<String>) -> Read<Part> {
    let root = Path::default();
    let (global, uses) = global(
        reader,
        &root.key("global"),
        table.global.unwrap_or_default(),
    )?;
    let repository = repository(
        reader,
        &root.key("repository"),
        table.repository.unwrap_or_default(),
    )?;
    let (backup, mut entries) = backup(
        reader,
        &root.key("backup"),
        table.backup.unwrap_or_default(),
    )?;
    let forget = forget(
        reader,
        &root.key("forget"),
        table.forget.unwrap_or_default(),
    )?;
    sort_and_dedup(&mut entries);
    let filter = root.key("snapshot-filter");
    let snapshot_filter = keys_at(reader, &filter)
        .next()
        .filter(|_| !is_own_filter(reader, &filter))
        .map(|key| {
            let here = filter.key(key);
            Located {
                file: reader.file.to_owned(),
                key: here.text().unwrap_or_default(),
                at: reader.at(&here, Spot::Key),
            }
        });
    Ok(Part {
        global,
        repository,
        backup,
        entries,
        forget,
        snapshot_filter,
        left_out,
        uses,
    })
}

/// Whether the `[snapshot-filter]` at `path` is the one an export makes: one
/// `filter-tags` entry with the two tags of a plan and its agent, which
/// selects what Ladderback's forget reaches anyway, the plan's own
/// snapshots. Anything else narrows what rustic's forget reaches.
fn is_own_filter(reader: &Reader<'_, '_>, path: &Path) -> bool {
    if keys_at(reader, path).count() != 1 {
        return false;
    }
    let Some(tags) = texts_at(reader.doc, &path.key("filter-tags")) else {
        return false;
    };
    let [entry] = tags.as_slice() else {
        return false;
    };
    let mut tags = entry.split(',');
    matches!(
        (tags.next(), tags.next(), tags.next()),
        (Some(plan), Some(agent), None)
            if plan.starts_with(PLAN_TAG_PREFIX) && agent.starts_with(AGENT_TAG_PREFIX)
    )
}

fn global(reader: &Reader<'_, '_>, path: &Path, table: GlobalTable) -> Read<(Global, Vec<Use>)> {
    // The text of a used profile is escaped so that it stays text. With this
    // on, rustic reads its `${VAR}` as a variable, so the plans would not be
    // what rustic ran.
    if table.profile_substitute_env == Some(true) {
        return Err(reader.unsupported(&path.key("profile-substitute-env"), Spot::Key));
    }
    let group_by = group_by(reader, &path.key("group-by"), table.group_by)?;
    let hooks = hooks(reader, &path.key("hooks"), table.hooks.unwrap_or_default())?;
    let names_path = path.key("use-profiles");
    let uses = table
        .use_profiles
        .unwrap_or_default()
        .into_iter()
        .enumerate()
        .map(|(index, name)| {
            let here = names_path.index(index);
            Use {
                name,
                key: here.text().unwrap_or_default(),
                at: reader.at(&here, Spot::Value),
            }
        })
        .collect();
    let global = Global {
        group_by,
        hooks,
        env: table.env.is_some_and(|env| !env.is_empty()),
    };
    Ok((global, uses))
}

fn group_by(reader: &Reader<'_, '_>, path: &Path, text: Option<String>) -> Read<Option<GroupBy>> {
    text.map(|text| reader.value(path, GroupBy::parse(&text)))
        .transpose()
}

fn hooks(reader: &Reader<'_, '_>, path: &Path, table: HooksTable) -> Read<Hooks> {
    let list = |key: &str, entries: Option<Vec<CommandValue>>| -> Read<Vec<HookEntry>> {
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

fn hook(reader: &Reader<'_, '_>, path: &Path, entry: CommandValue) -> Read<HookEntry> {
    match entry {
        CommandValue::Text(text) => reader.value(path, HookEntry::parse(&text)),
        CommandValue::Table(CommandTable {
            command,
            args,
            on_failure,
        }) => {
            let command = command.ok_or_else(|| reader.missing(path, "command"))?;
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

/// A command as a program and its arguments, for `password-command`.
fn command_line(reader: &Reader<'_, '_>, path: &Path, value: CommandValue) -> Read<CommandLine> {
    let line = match value {
        CommandValue::Text(text) => CommandLine::parse(&text),
        CommandValue::Table(CommandTable { command, args, .. }) => {
            let command = command.ok_or_else(|| reader.missing(path, "command"))?;
            CommandLine::new(command, args.unwrap_or_default())
        }
    };
    line.map_err(|_| reader.refuse(path, ProfileProblem::BadValue))
}

fn repository(reader: &Reader<'_, '_>, path: &Path, table: RepositoryTable) -> Read<Repository> {
    let password_file = path.key("password-file");
    if table
        .password_file
        .as_deref()
        .is_some_and(|file| !file.starts_with('/'))
    {
        return Err(reader.unsupported(&password_file, Spot::Value));
    }
    let password_command = table
        .password_command
        .map(|command| command_line(reader, &path.key("password-command"), command))
        .transpose()?;
    Ok(Repository {
        repository: table.repository,
        repo_hot: table.repo_hot,
        options: table.options.unwrap_or_default(),
        options_hot: table.options_hot.unwrap_or_default(),
        options_cold: table.options_cold.unwrap_or_default(),
        password: table.password,
        password_file: table.password_file,
        password_command,
        hooks: hooks(reader, &path.key("hooks"), table.hooks.unwrap_or_default())?,
    })
}

/// `[backup]`, and the entries under it.
fn backup(
    reader: &Reader<'_, '_>,
    path: &Path,
    mut table: BackupTable,
) -> Read<(Settings, Vec<Entry>)> {
    let entries = std::mem::take(&mut table.snapshots).unwrap_or_default();
    let settings = settings(reader, path, table)?;
    let entries_path = path.key(keys::SNAPSHOTS);
    let entries = entries
        .into_iter()
        .enumerate()
        .map(|(index, entry)| entry_of(reader, &entries_path.index(index), entry))
        .collect::<Read<Vec<_>>>()?;
    Ok((settings, entries))
}

fn entry_of(reader: &Reader<'_, '_>, path: &Path, mut table: BackupTable) -> Read<Entry> {
    let name = table.name.take();
    let sources_path = path.key(keys::SOURCES);
    let sources = table
        .sources
        .take()
        .filter(|sources| !sources.is_empty())
        .ok_or_else(|| reader.missing(path, keys::SOURCES))?
        .iter()
        .enumerate()
        .map(|(index, text)| {
            let here = sources_path.index(index);
            Source::parse(text).map_err(|_| reader.unsupported(&here, Spot::Value))
        })
        .collect::<Read<Vec<_>>>()?;
    Ok(Entry {
        name,
        sources,
        settings: settings(reader, path, table)?,
    })
}

fn settings(reader: &Reader<'_, '_>, path: &Path, table: BackupTable) -> Read<Settings> {
    let host = table
        .host
        .map(|text| reader.value(&path.key("host"), Hostname::parse(&text)))
        .transpose()?;
    Ok(Settings {
        label: table.label,
        host,
        tags: table.tags.unwrap_or_default(),
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
        hooks: hooks(reader, &path.key("hooks"), table.hooks.unwrap_or_default())?,
    })
}

/// The files of glob patterns listed at `path`. rustic reads a relative one
/// from the folder it runs in, which a profile doesn't name: taking the
/// profile's own folder instead would read a file rustic doesn't, so a
/// relative one is refused at its index, as a relative source is.
fn glob_files(
    reader: &Reader<'_, '_>,
    path: &Path,
    files: Option<Vec<String>>,
) -> Read<Vec<GlobFile>> {
    files
        .unwrap_or_default()
        .iter()
        .enumerate()
        .map(|(index, text)| {
            GlobFile::parse(text).map_err(|_| reader.unsupported(&path.index(index), Spot::Value))
        })
        .collect()
}

fn forget(reader: &Reader<'_, '_>, path: &Path, table: ForgetTable) -> Read<Forget> {
    for (key, rule) in [
        ("keep-tags", &table.keep_tags),
        ("keep-ids", &table.keep_ids),
    ] {
        if rule.as_ref().is_some_and(|rule| !rule.is_empty()) {
            return Err(reader.unsupported(&path.key(key), Spot::Key));
        }
    }
    let count = |key: &str, count: Option<i64>| -> Read<Option<KeepCount>> {
        count
            .map(|count| reader.value(&path.key(key), KeepCount::new(count)))
            .transpose()
    };
    let within = |key: &str, text: Option<String>| -> Read<Option<KeepWithin>> {
        text.map(|text| reader.value(&path.key(key), KeepWithin::parse(&text)))
            .transpose()
    };
    let retention = RetentionPolicy {
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
    };
    Ok(Forget {
        group_by: group_by(reader, &path.key("group-by"), table.group_by)?,
        prune: table.prune.unwrap_or(false),
        retention,
    })
}
