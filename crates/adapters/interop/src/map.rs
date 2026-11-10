//! A merged profile into Ladderback's types (technical design §6.2): the
//! repository, a plan for each snapshot entry, and the hooks of the agent,
//! with a notice for everything that was left out. What the profile can't
//! say in Ladderback's terms, or says in a way Ladderback could only guess
//! at, is refused.

use std::collections::BTreeMap;

use app::ports::{ImportedProfile, ProfileError, ProfileNotice, ProfileProblem};
use domain::execution::plan_tags::UserTag;
use domain::planning::{
    Hooks, Maintenance, Plan, RepositoryConfig, RepositorySettings, Source, SourceFilter,
};
use domain::secrets::{CommandLine, FileRead, SecretRef, SecretValue, is_secret_option};
use domain::{PlanId, RepositoryAlias};

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
    let plans = part
        .entries
        .iter()
        .map(|entry| plan(&part, entry, alias, file, &mut notices))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(ImportedProfile {
        repository,
        plans,
        hooks: part.global.hooks,
        notices,
    })
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
        (Some(location), _) => escape::text(location),
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
    let password = password(own, variables, file, notices)?;
    let settings = RepositorySettings {
        repository: location,
        repo_hot: own.repo_hot.as_deref().map(escape::text),
        password,
        options: options(&own.options, "options", variables, OPTIONS, notices),
        options_hot: options(
            &own.options_hot,
            "options-hot",
            variables,
            OPTIONS_HOT,
            notices,
        ),
        options_cold: options(
            &own.options_cold,
            "options-cold",
            variables,
            OPTIONS_COLD,
            notices,
        ),
    };
    Ok(RepositoryConfig {
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

/// Whether `name` is a name a `${…}` can hold.
fn is_variable_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic() || first == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
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
/// is written in the profile is noted: it is kept as text.
fn options(
    own: &BTreeMap<String, String>,
    table: &str,
    variables: &BTreeMap<String, String>,
    prefix: &str,
    notices: &mut Vec<ProfileNotice>,
) -> BTreeMap<String, String> {
    let mut options = BTreeMap::new();
    for (name, value) in own {
        if is_secret_option(name) {
            notices.push(ProfileNotice::InlineSecret {
                field: format!("repository.{table}.{}", plain(name)),
            });
        }
        options.insert(name.clone(), escape::text(value));
    }
    for variable in variables.keys() {
        let Some(rest) = variable.strip_prefix(prefix) else {
            continue;
        };
        let name = rest.to_ascii_lowercase().replace('_', "-");
        if name.is_empty() || !is_variable_name(variable) || variables[variable].is_empty() {
            continue;
        }
        options.entry(name).or_insert_with(|| reference(variable));
    }
    options
}

// --- The plans --------------------------------------------------------------

fn plan(
    part: &Part,
    entry: &Entry,
    alias: &RepositoryAlias,
    file: &str,
    notices: &mut Vec<ProfileNotice>,
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
    plan.tags = tags(&settings, notices);
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
/// A reserved tag is dropped, with a notice.
fn tags(settings: &Settings, notices: &mut Vec<ProfileNotice>) -> Vec<UserTag> {
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
                let notice = ProfileNotice::TagDropped { tag: reserved.tag };
                if !notices.contains(&notice) {
                    notices.push(notice);
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
