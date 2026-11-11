//! A merged profile into Ladderback's types (technical design §6.2): the
//! repository, a plan for each snapshot entry, and the hooks of the agent,
//! with a notice for everything that was left out. What the profile can't
//! say in Ladderback's terms, or says in a way Ladderback could only guess
//! at, is refused.

use std::collections::{BTreeMap, BTreeSet};

use app::ports::{ImportedProfile, ProfileError, ProfileNotice, ProfileProblem};
use domain::execution::plan_tags::UserTag;
use domain::planning::{
    Hooks, Maintenance, Plan, RepositoryConfig, RepositorySettings, Source, SourceFilter,
    check_options,
};
use domain::secrets::{
    CommandLine, FileRead, SecretRef, SecretValue, Template, is_secret_option,
    location_secret_written, rclone_location, url_password_written,
};
use domain::{PlanId, RepositoryAlias};

use crate::MAX_ITEMS;
use crate::escape;
use crate::locate::plain;
use crate::part::{Entry, Part, Repository, Settings};
use crate::resolve::Resolved;

/// The variables of rustic's environment the import reads, and how they are
/// named.
const PASSWORD: &str = "RUSTIC_PASSWORD";
const PASSWORD_FILE: &str = "RUSTIC_PASSWORD_FILE";
const PASSWORD_COMMAND: &str = "RUSTIC_PASSWORD_COMMAND";
const REPOSITORY: &str = "RUSTIC_REPOSITORY";
const OPTIONS: &str = "RUSTIC_REPO_OPT_";
const OPTIONS_HOT: &str = "RUSTIC_REPO_OPTHOT_";
const OPTIONS_COLD: &str = "RUSTIC_REPO_OPTCOLD_";

/// What `plan` names a plan by when nothing in its entry does.
const FALLBACK_PLAN: &str = "plan";

/// The profile `resolved` as an [`ImportedProfile`] whose plans back up into
/// `alias`. `variables` is rustic's environment; `file` the profile's own
/// path, for the refusals that are not at any key of a file.
pub(crate) fn map(
    resolved: Resolved,
    alias: &RepositoryAlias,
    variables: &BTreeMap<String, String>,
    file: &str,
) -> Result<ImportedProfile, ProfileError> {
    let Resolved { part, missing } = resolved;
    let mut notices: Vec<ProfileNotice> = missing
        .into_iter()
        .map(|name| ProfileNotice::UsedProfileMissing { name })
        .collect();
    notices.extend(
        part.left_out
            .iter()
            .map(|key| ProfileNotice::KeyIgnored { key: key.clone() }),
    );
    check_items(&part, file)?;
    // rustic's forget reaches only the snapshots the filter selects, so the
    // retention would delete more here than it does there.
    if let Some(filter) = &part.snapshot_filter
        && !part.forget.retention.is_empty()
    {
        return Err(ProfileError::Unsupported {
            file: filter.file.clone(),
            key: filter.key.clone(),
            at: filter.at,
        });
    }
    let repository = repository(&part, variables, file, &mut notices)?;
    if part.global.env {
        notices.push(ProfileNotice::EnvIgnored);
    }
    if part.forget.prune {
        notices.push(ProfileNotice::Prune);
    }
    if !part.backup.hooks.is_empty() && !part.entries.is_empty() {
        notices.push(ProfileNotice::HooksPerPlan);
    }
    let mut dropped = BTreeSet::new();
    let plans = part
        .entries
        .iter()
        .map(|entry| plan(&part, entry, alias, file, &mut notices, &mut dropped))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(ImportedProfile {
        repository,
        plans,
        hooks: part.global.hooks,
        notices,
    })
}

/// Refuses a profile whose plans would hold more than [`MAX_ITEMS`] items:
/// every plan gets a copy of what `[backup]` sets next to what its entry sets,
/// so the memory the plans take is the number of entries times what `[backup]`
/// holds, which a profile of a megabyte can make as large as it likes.
fn check_items(part: &Part, file: &str) -> Result<(), ProfileError> {
    let shared = items(&part.backup);
    let total = part.entries.iter().fold(0_usize, |total, entry| {
        total
            .saturating_add(shared)
            .saturating_add(entry.sources.len())
            .saturating_add(items(&entry.settings))
    });
    if total <= MAX_ITEMS {
        return Ok(());
    }
    Err(ProfileError::Invalid {
        file: file.to_owned(),
        key: Some("backup".to_owned()),
        at: None,
        problem: ProfileProblem::TooManyItems,
        detail: String::new(),
    })
}

/// How many items the lists and the hooks of `settings` hold.
fn items(settings: &Settings) -> usize {
    let hooks = &settings.hooks;
    [
        settings.tags.len(),
        settings.globs.len(),
        settings.iglobs.len(),
        settings.glob_files.len(),
        settings.iglob_files.len(),
        settings.custom_ignorefiles.len(),
        settings.exclude_if_present.len(),
        settings.exclude_if_xattr.len(),
        hooks.run_before.len(),
        hooks.run_after.len(),
        hooks.run_failed.len(),
        hooks.run_finally.len(),
    ]
    .into_iter()
    .fold(0_usize, usize::saturating_add)
}

// --- The repository ---------------------------------------------------------

fn repository(
    part: &Part,
    variables: &BTreeMap<String, String>,
    file: &str,
    notices: &mut Vec<ProfileNotice>,
) -> Result<RepositoryConfig, ProfileError> {
    let own = &part.repository;
    let location = match (&own.repository, variable(variables, REPOSITORY)) {
        (Some(location), _) => fits(escape::text(location), "repository.repository", file)?,
        (None, Some(_)) => reference(REPOSITORY),
        (None, None) => {
            return Err(ProfileError::Invalid {
                file: file.to_owned(),
                key: Some("repository.repository".to_owned()),
                at: None,
                problem: app::ports::ProfileProblem::Missing,
                detail: String::new(),
            });
        }
    };
    let repo_hot = own
        .repo_hot
        .as_deref()
        .map(|hot| fits(escape::text(hot), "repository.repo-hot", file))
        .transpose()?;
    // A location the profile wrote is escaped, so a `${…}` in it is text: a
    // password there is kept as text, whatever it looks like. One from
    // rustic's variable is a reference, which keeps nothing as text.
    for (key, written) in [
        ("repository.repository", Some(&location)),
        ("repository.repo-hot", repo_hot.as_ref()),
    ] {
        if let Some(written) = written.filter(|written| location_secret_written(written)) {
            notices.push(ProfileNotice::LocationSecret {
                field: key.to_owned(),
                rclone: rclone_location(written),
            });
        }
    }
    let password = password(own, variables, file, notices)?;
    let settings = RepositorySettings {
        repository: location,
        repo_hot,
        password,
        options: options(&own.options, "options", variables, OPTIONS, file, notices)?,
        options_hot: options(
            &own.options_hot,
            "options-hot",
            variables,
            OPTIONS_HOT,
            file,
            notices,
        )?,
        options_cold: options(
            &own.options_cold,
            "options-cold",
            variables,
            OPTIONS_COLD,
            file,
            notices,
        )?,
    };
    Ok(RepositoryConfig {
        id: None,
        settings,
        maintenance: if part.forget.prune {
            Maintenance::Local
        } else {
            Maintenance::None
        },
        group_by: part
            .forget
            .group_by
            .or(part.global.group_by)
            .unwrap_or_default(),
        hooks: own.hooks.clone(),
    })
}

/// `value`, an escaped value of the profile at `key`, if the config will
/// read it back: it is a template, which is no longer than a secret may be,
/// and escaping made it up to half as long again.
fn fits(value: String, key: &str, file: &str) -> Result<String, ProfileError> {
    match Template::parse(&value) {
        Ok(_) => Ok(value),
        Err(_) => Err(ProfileError::Invalid {
            file: file.to_owned(),
            key: Some(key.to_owned()),
            at: None,
            problem: ProfileProblem::BadValue,
            detail: String::new(),
        }),
    }
}

/// A variable of rustic's environment, unless it is empty.
fn variable<'a>(variables: &'a BTreeMap<String, String>, name: &str) -> Option<&'a str> {
    variables
        .get(name)
        .map(String::as_str)
        .filter(|value| !value.is_empty())
}

/// What stands for the variable `name` in a repository field. It is written
/// here, not by the profile, so it is the one `${` that is not escaped.
fn reference(name: &str) -> String {
    format!("${{{name}}}")
}

/// Whether `name`, a variable of the process that starts with `RUSTIC_`, is
/// one a `${…}` can hold: letters, digits and `_`.
fn is_variable_name(name: &str) -> bool {
    name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// The password, from the first of the sources rustic asks, in its order:
/// the profile's `password`, `password-file` and `password-command`, then
/// rustic's variables for them. What a profile names beats a variable, and
/// the variables are kept as references, never as the password.
fn password(
    own: &Repository,
    variables: &BTreeMap<String, String>,
    file: &str,
    notices: &mut Vec<ProfileNotice>,
) -> Result<SecretRef, ProfileError> {
    let refused = |key: &str| ProfileError::Unsupported {
        file: file.to_owned(),
        key: key.to_owned(),
        at: None,
    };
    let given = [
        ("repository.password", own.password.is_some()),
        ("repository.password-file", own.password_file.is_some()),
        (
            "repository.password-command",
            own.password_command.is_some(),
        ),
    ];
    let chosen = if let Some(text) = &own.password {
        notices.push(ProfileNotice::InlineSecret {
            field: "repository.password".to_owned(),
        });
        Some(SecretRef::Inline(SecretValue::new(text.clone())))
    } else if let Some(path) = &own.password_file {
        Some(SecretRef::File {
            path: path.clone(),
            read: FileRead::FirstLine,
        })
    } else {
        own.password_command.clone().map(SecretRef::Command)
    };
    let Some(chosen) = chosen else {
        return from_variables(variables, &refused);
    };
    let kept = match &chosen {
        SecretRef::Inline(_) => 0,
        SecretRef::File { .. } => 1,
        _ => 2,
    };
    for (index, (key, set)) in given.iter().enumerate() {
        if *set && index != kept {
            notices.push(ProfileNotice::KeyIgnored {
                key: (*key).to_owned(),
            });
        }
    }
    Ok(chosen)
}

fn from_variables(
    variables: &BTreeMap<String, String>,
    refused: &dyn Fn(&str) -> ProfileError,
) -> Result<SecretRef, ProfileError> {
    if variable(variables, PASSWORD).is_some() {
        return Ok(SecretRef::Env(PASSWORD.to_owned()));
    }
    if let Some(path) = variable(variables, PASSWORD_FILE) {
        return if path.starts_with('/') {
            Ok(SecretRef::File {
                path: path.to_owned(),
                read: FileRead::FirstLine,
            })
        } else {
            Err(refused(PASSWORD_FILE))
        };
    }
    if let Some(text) = variable(variables, PASSWORD_COMMAND) {
        return CommandLine::parse(text)
            .map(SecretRef::Command)
            .map_err(|_| refused(PASSWORD_COMMAND));
    }
    Err(refused("repository.password"))
}

/// The options of one table, with a reference for each of rustic's variables
/// whose option the profile leaves out. An option that holds a credential and
/// is written in the profile is noted: it is kept as text. So is one whose
/// value is a URL with a password in it.
fn options(
    own: &BTreeMap<String, String>,
    table: &str,
    variables: &BTreeMap<String, String>,
    prefix: &str,
    file: &str,
    notices: &mut Vec<ProfileNotice>,
) -> Result<BTreeMap<String, String>, ProfileError> {
    let mut options = BTreeMap::new();
    for (name, value) in own {
        let key = format!("repository.{table}.{}", plain(name));
        let value = fits(escape::text(value), &key, file)?;
        if is_secret_option(name) || url_password_written(&value) {
            notices.push(ProfileNotice::InlineSecret { field: key });
        }
        options.insert(name.clone(), value);
    }
    for (variable, value) in variables {
        let Some(rest) = variable.strip_prefix(prefix) else {
            continue;
        };
        let name = rest.to_ascii_lowercase().replace('_', "-");
        if name.is_empty() || !is_variable_name(variable) || value.is_empty() {
            continue;
        }
        options.entry(name).or_insert_with(|| reference(variable));
    }
    // The profiles it uses and rustic's variables may add options to the
    // ones the profile writes: the table the config gets is counted whole.
    check_options(&options).map_err(|_| ProfileError::Invalid {
        file: file.to_owned(),
        key: Some(format!("repository.{table}")),
        at: None,
        problem: ProfileProblem::BadValue,
        detail: String::new(),
    })?;
    Ok(options)
}

// --- The plans --------------------------------------------------------------

fn plan(
    part: &Part,
    entry: &Entry,
    alias: &RepositoryAlias,
    file: &str,
    notices: &mut Vec<ProfileNotice>,
    dropped: &mut BTreeSet<String>,
) -> Result<Plan, ProfileError> {
    let mut settings = entry.settings.clone();
    let entry_hooks = std::mem::take(&mut settings.hooks);
    let mut backup = part.backup.clone();
    let backup_hooks = std::mem::take(&mut backup.hooks);
    settings.merge(backup);

    let mut plan = Plan::new(
        id(entry, &settings, file)?,
        alias.clone(),
        entry.sources.clone(),
    );
    plan.host = settings.host.clone();
    plan.label = settings.label.clone();
    plan.tags = tags(&settings, notices, dropped);
    plan.filter = filter(settings);
    plan.retention = part.forget.retention.clone();
    plan.hooks = hooks(backup_hooks, entry_hooks);
    Ok(plan)
}

/// The ID a plan gets: its name, else its label, else the folder it backs up,
/// else `plan`, each made into a slug. `plan` always is one, so the error is
/// for a case that can't happen.
fn id(entry: &Entry, settings: &Settings, file: &str) -> Result<PlanId, ProfileError> {
    let folder = entry
        .sources
        .first()
        .and_then(|source: &Source| source.as_str().rsplit('/').find(|part| !part.is_empty()));
    [
        entry.name.as_deref(),
        settings.label.as_deref(),
        folder,
        Some(FALLBACK_PLAN),
    ]
    .into_iter()
    .flatten()
    .find_map(PlanId::slugify)
    .ok_or_else(|| ProfileError::Invalid {
        file: file.to_owned(),
        key: Some("backup.snapshots".to_owned()),
        at: None,
        problem: ProfileProblem::BadValue,
        detail: String::new(),
    })
}

/// What a tag written as `a,b` is: each of the two, the empty ones left out.
/// A reserved tag is dropped, with a notice the first time it is.
fn tags(
    settings: &Settings,
    notices: &mut Vec<ProfileNotice>,
    dropped: &mut BTreeSet<String>,
) -> Vec<UserTag> {
    let mut kept = Vec::new();
    for tag in settings
        .tags
        .iter()
        .flat_map(|tags| tags.split(','))
        .filter(|tag| !tag.is_empty())
    {
        match UserTag::new(tag) {
            Ok(tag) => kept.push(tag),
            Err(reserved) => {
                // The notice is shown as it is, and the tag is text of the
                // profile.
                let tag = plain(&reserved.tag);
                if dropped.insert(tag.clone()) {
                    notices.push(ProfileNotice::TagDropped { tag });
                }
            }
        }
    }
    kept
}

fn filter(settings: Settings) -> SourceFilter {
    SourceFilter {
        globs: settings.globs,
        iglobs: settings.iglobs,
        glob_files: settings.glob_files,
        iglob_files: settings.iglob_files,
        git_ignore: settings.git_ignore,
        no_require_git: settings.no_require_git,
        custom_ignorefiles: settings.custom_ignorefiles,
        exclude_if_present: settings.exclude_if_present,
        exclude_if_xattr: settings.exclude_if_xattr,
        one_file_system: settings.one_file_system,
    }
}

/// The hooks of a plan, in the order rustic runs them: the backup section's
/// are the outer ones, so they run first before and last after.
fn hooks(backup: Hooks, entry: Hooks) -> Hooks {
    let around =
        |outer: Vec<_>, inner: Vec<_>| -> Vec<_> { outer.into_iter().chain(inner).collect() };
    Hooks {
        run_before: around(backup.run_before, entry.run_before),
        run_after: around(entry.run_after, backup.run_after),
        run_failed: around(entry.run_failed, backup.run_failed),
        run_finally: around(entry.run_finally, backup.run_finally),
    }
}
