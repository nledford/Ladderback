//! The codes, arguments and details the catalog renders (§8.4) for what an
//! import or an export of a rustic profile refused or noted (§6.2).

use api::{ErrorCode, MsgArg, WarningCode};
use app::interop::{ExportError, ImportError};
use app::ports::{ConfigError, ExportRefusal, ProfileError, ProfileNotice};

use super::{glob_never_matches, location_kind, text};

/// An import that saved nothing, as the code, arguments and detail the
/// catalog renders (§8.4).
pub(crate) struct Refusal {
    pub(crate) code: ErrorCode,
    pub(crate) args: Vec<(&'static str, MsgArg)>,
    pub(crate) detail: Option<String>,
}

/// Something an import did that the person should know, as the code and
/// arguments the catalog renders.
pub(crate) struct Notice {
    pub(crate) code: WarningCode,
    pub(crate) args: Vec<(&'static str, MsgArg)>,
}

/// The `line` argument of a message about a place in a file, left out when
/// no place is known.
fn line_arg(at: Option<app::ports::Position>) -> Option<(&'static str, MsgArg)> {
    at.map(|at| ("line", text(at.line)))
}

/// What came of a refused import. `config_file` is the configuration's own
/// path, which a refusal about the merged configuration (rather than about a
/// file the store read) names.
pub(crate) fn import_refusal(error: ImportError, config_file: &std::path::Path) -> Refusal {
    match error {
        ImportError::Profile(error) => profile_refusal(error),
        ImportError::AliasTaken(alias) => Refusal {
            code: ErrorCode::RepositoryAliasTaken,
            args: vec![("alias", text(alias.as_str()))],
            detail: None,
        },
        ImportError::AlreadyConfigured(alias) => Refusal {
            code: ErrorCode::RepositoryAlreadyConfigured,
            args: vec![("alias", text(alias.as_str()))],
            detail: None,
        },
        ImportError::PlanTaken(plan) => Refusal {
            code: ErrorCode::PlanIdTaken,
            args: vec![("plan", text(plan.as_str()))],
            detail: None,
        },
        ImportError::Config(error) => profile_config_refusal(error, config_file),
        ImportError::CommandsUnconfirmed(_) => Refusal {
            code: ErrorCode::ProfileCommandsUnconfirmed,
            args: Vec::new(),
            detail: None,
        },
    }
}

/// What came of a refused export: the code, arguments and detail the catalog
/// renders. `config_file` is as for [`import_refusal`].
pub(crate) fn export_refusal(error: ExportError, config_file: &std::path::Path) -> Refusal {
    match error {
        ExportError::PlanUnknown(plan) => plan_unknown(plan.as_str()),
        ExportError::Render(ExportRefusal::InlineSecret) => Refusal {
            code: ErrorCode::InlineSecret,
            args: Vec::new(),
            detail: None,
        },
        ExportError::Render(ExportRefusal::Reference { key }) => Refusal {
            code: ErrorCode::ProfileExportReference,
            args: vec![("key", text(key))],
            detail: None,
        },
        ExportError::Render(ExportRefusal::UrlPassword { key }) => Refusal {
            code: ErrorCode::ProfileExportUrlPassword,
            args: vec![("key", text(key))],
            detail: None,
        },
        ExportError::Render(ExportRefusal::TagComma { tag }) => Refusal {
            code: ErrorCode::ProfileExportTagComma,
            args: vec![("tag", text(tag))],
            detail: None,
        },
        ExportError::Config(error) => profile_config_refusal(error, config_file),
    }
}

/// A plan the configuration does not have, or a name no plan can have.
pub(crate) fn plan_unknown(plan: &str) -> Refusal {
    Refusal {
        code: ErrorCode::PlanUnknown,
        args: vec![("plan", text(plan))],
        detail: None,
    }
}

/// A name for a repository that is not an alias: the one a person asked for,
/// or what could be made of a profile's file name.
pub(crate) fn repository_alias_invalid() -> Refusal {
    Refusal {
        code: ErrorCode::RepositoryAliasInvalid,
        args: Vec::new(),
        detail: None,
    }
}

/// A command that has no folder to keep the configuration in: the platform
/// names no home folder and no configuration folder is set.
pub(crate) fn config_folder_unknown() -> Refusal {
    Refusal {
        code: ErrorCode::ConfigFolderUnknown,
        args: Vec::new(),
        detail: None,
    }
}

/// A command whose async runtime did not start. The detail is the operating
/// system's own message, which the command line shows as it came.
pub(crate) fn runtime_unavailable(error: &std::io::Error) -> Refusal {
    Refusal {
        code: ErrorCode::RuntimeUnavailable,
        args: Vec::new(),
        detail: Some(error.to_string()),
    }
}

/// What the reader said was wrong with a profile.
fn profile_refusal(error: ProfileError) -> Refusal {
    match error {
        ProfileError::Unreadable { file } => Refusal {
            code: ErrorCode::ProfileUnreadable,
            args: vec![("file", text(file))],
            detail: None,
        },
        ProfileError::TooLarge { file, limit } => Refusal {
            code: ErrorCode::ProfileTooLarge,
            args: vec![("file", text(file)), ("limit", MsgArg::Bytes(limit))],
            detail: None,
        },
        ProfileError::Invalid {
            file,
            key,
            at,
            problem,
            detail,
        } => {
            let mut args = vec![("file", text(file))];
            args.extend(key.map(|key| ("key", text(key))));
            args.extend(line_arg(at));
            Refusal {
                code: ErrorCode::ProfileInvalid,
                args,
                detail: Some(problem_detail(&format!("{problem:?}"), &detail)),
            }
        }
        ProfileError::Unsupported { file, key, at } => {
            let mut args = vec![("file", text(file)), ("key", text(key))];
            args.extend(line_arg(at));
            Refusal {
                code: ErrorCode::ProfileUnsupported,
                args,
                detail: None,
            }
        }
    }
}

/// What the store said was wrong with the configuration an import would have
/// written: the answer a person gets for the file they know.
fn profile_config_refusal(error: ConfigError, config_file: &std::path::Path) -> Refusal {
    fn refusal(
        code: ErrorCode,
        args: Vec<(&'static str, MsgArg)>,
        detail: Option<String>,
    ) -> Refusal {
        Refusal { code, args, detail }
    }
    match error {
        ConfigError::Unavailable { file, detail } => refusal(
            ErrorCode::ConfigUnavailable,
            vec![("file", text(file))],
            Some(detail),
        ),
        ConfigError::TooLarge { file, limit } => refusal(
            ErrorCode::ConfigTooLarge,
            vec![("file", text(file)), ("limit", MsgArg::Bytes(limit))],
            None,
        ),
        ConfigError::SchemaMissing { file } => refusal(
            ErrorCode::ConfigSchemaMissing,
            vec![("file", text(file))],
            None,
        ),
        ConfigError::SchemaNewer {
            file,
            found,
            supported,
        } => refusal(
            ErrorCode::ConfigSchemaNewer,
            vec![
                ("file", text(file)),
                ("found", text(found)),
                ("supported", text(supported)),
            ],
            None,
        ),
        ConfigError::Invalid {
            file,
            key,
            at,
            problem,
            detail,
        } => {
            // The use case's own check of the merged configuration names no
            // file: it is the configuration the import would have written.
            let file = if file.is_empty() {
                config_file.display().to_string()
            } else {
                file
            };
            let mut args = vec![("file", text(file))];
            args.extend(line_arg(at));
            let what = problem_detail(&format!("{problem:?}"), &detail);
            refusal(
                ErrorCode::ConfigInvalid,
                args,
                Some(match key {
                    Some(key) => format!("{key}: {what}"),
                    None => what,
                }),
            )
        }
    }
}

/// What is wrong, as the problem's name and the parser's words when it had
/// any.
fn problem_detail(problem: &str, detail: &str) -> String {
    if detail.is_empty() {
        problem.to_owned()
    } else {
        format!("{problem}: {detail}")
    }
}

/// A notice of an import or export, as the warning the catalog renders.
pub(crate) fn profile_notice(notice: ProfileNotice) -> Notice {
    let (code, args) = match notice {
        ProfileNotice::KeyIgnored { key } => {
            (WarningCode::ProfileKeyIgnored, vec![("key", text(key))])
        }
        ProfileNotice::Prune => (WarningCode::ProfilePrune, Vec::new()),
        ProfileNotice::EnvIgnored => (WarningCode::ProfileEnvIgnored, Vec::new()),
        ProfileNotice::HooksPerPlan => (WarningCode::ProfileHooksPerPlan, Vec::new()),
        ProfileNotice::UsedProfileMissing { name } => {
            (WarningCode::ProfileUsedMissing, vec![("name", text(name))])
        }
        ProfileNotice::UsedProfileNotRead { name } => {
            (WarningCode::ProfileUsedNotRead, vec![("name", text(name))])
        }
        ProfileNotice::InlineSecret { field } => {
            (WarningCode::InlineSecret, vec![("field", text(field))])
        }
        ProfileNotice::LocationSecret { field, rclone } => (
            WarningCode::LocationSecret,
            vec![("field", text(field)), ("kind", location_kind(rclone))],
        ),
        ProfileNotice::GlobNeverMatches {
            plan,
            glob,
            absolute,
        } => glob_never_matches(&plan, &glob, absolute.as_deref()),
        ProfileNotice::TagDropped { tag } => {
            (WarningCode::ProfileTagDropped, vec![("tag", text(tag))])
        }
        ProfileNotice::OptionLeftOut { option, variable } => (
            WarningCode::ProfileOptionLeftOut,
            vec![("option", text(option)), ("variable", text(variable))],
        ),
        ProfileNotice::RetentionEmpty => (WarningCode::ProfileRetentionEmpty, Vec::new()),
        ProfileNotice::RepositorySettingKept {
            repository,
            setting,
        } => (
            WarningCode::ProfileSettingKept,
            vec![("repository", text(repository)), ("setting", text(setting))],
        ),
        ProfileNotice::KeyringCommandMissing { program } => (
            WarningCode::ProfileKeyringCommandMissing,
            vec![("program", text(program))],
        ),
        ProfileNotice::LocationsNotExported { further } => (
            WarningCode::ProfileLocationsNotExported,
            vec![(
                "count",
                MsgArg::Count(u64::try_from(further).unwrap_or(u64::MAX)),
            )],
        ),
        ProfileNotice::CommandLeftOut { key } => {
            (WarningCode::ProfileCommandLeftOut, vec![("key", text(key))])
        }
        ProfileNotice::CommandImported { key, program } => (
            WarningCode::ProfileCommandImported,
            vec![("key", text(key)), ("program", text(program))],
        ),
        ProfileNotice::GlobalHooksLeftOut => (WarningCode::ProfileGlobalHooksLeftOut, Vec::new()),
    };
    Notice { code, args }
}
