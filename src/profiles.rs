//! `ladderback import-profile` and `ladderback export-profile` (technical
//! design §6.2): wires the interop use cases to the config file, the agent's
//! ID and rustic's profile format, and reports what they did.
//!
//! The import's notices and refusals go to stderr as the catalog's messages
//! for their codes; what the import did goes to stdout. With `--json` it
//! prints one document on stdout instead, notices in it, made of codes and
//! arguments and never of sentences (§8.4). A refusal exits with 1 and
//! leaves the configuration as it was.
//!
//! The export prints the profile on stdout, and only when it was made.
//!
//! Both write through `output`, never `print!`: a command whose output
//! nobody reads any more stops quietly (see [`finish`]).

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;

use app::interop::{ExportProfile, ImportProfile, Imported};
use app::ports::ConfigStore;
use config_adapter::FileConfigStore;
use domain::{PlanId, RepositoryAlias};
use interop::{FileProfileReader, KeyringCommand, RusticProfileWriter};
use l10n::{Localizer, PosixEnv, msg};
use platform::{SystemClock, agent_id, dirs};

use crate::composition::{
    Notice, Refusal, config_folder_unknown, export_refusal, import_refusal, plan_unknown,
    profile_notice, repository_alias_invalid, runtime_unavailable,
};
use crate::output::{Report, to_stderr, to_stdout, write_json};
use crate::{finish, say, text};

/// Where stock rustic looks for the profiles a profile uses, when it is not
/// told otherwise (rustic's own order): its config folder, `/etc/rustic`,
/// then the working folder.
fn search_dirs(rustic_config: Option<PathBuf>, working: Option<PathBuf>) -> Vec<PathBuf> {
    rustic_config
        .into_iter()
        .chain(std::iter::once(PathBuf::from("/etc/rustic")))
        .chain(working)
        .collect()
}

/// The variables of rustic's environment, the only ones the importer is
/// told of. A variable whose name or value is not text is not rustic's to
/// read either.
fn rustic_variables(
    environment: impl IntoIterator<Item = (OsString, OsString)>,
) -> BTreeMap<String, String> {
    environment
        .into_iter()
        .filter_map(|(name, value)| Some((name.into_string().ok()?, value.into_string().ok()?)))
        .filter(|(name, _)| name.starts_with("RUSTIC_"))
        .collect()
}

/// Prints a refusal on stderr, as the catalog says it.
fn refuse(localizer: &Localizer, refusal: &Refusal) -> io::Result<()> {
    to_stderr(&format!(
        "ladderback: {}\n",
        localizer.error(refusal.code, &refusal.args, refusal.detail.as_deref())
    ))
}

/// Prints a notice on stderr, as the catalog says it.
fn warn(localizer: &Localizer, notice: &Notice) -> io::Result<()> {
    to_stderr(&format!(
        "ladderback: {}\n",
        localizer.warning(notice.code, &notice.args, None)
    ))
}

/// What `import-profile --json` prints when the import saved the profile.
fn import_document(imported: Imported) -> api::ProfileImportDto {
    api::ProfileImportDto {
        repository: imported.repository.as_str().to_owned(),
        plans: imported
            .plans
            .iter()
            .map(|plan| plan.as_str().to_owned())
            .collect(),
        notices: imported
            .notices
            .into_iter()
            .map(|notice| {
                let Notice { code, args } = profile_notice(notice);
                api::WarningDto::new(code, args)
            })
            .collect(),
    }
}

/// What `import-profile --json` prints when the import was refused.
fn failure_document(refusal: Refusal) -> api::FailureDto {
    api::FailureDto {
        error: api::ErrorDto::new(refusal.code, refusal.args, refusal.detail),
    }
}

/// What is wrong with the name an import would save its repository under.
enum AliasProblem {
    /// The name asked for with `--repo-name` is not an alias.
    BadName(String),
    /// None was asked for, and the profile's file name has nothing an alias
    /// can be made of.
    NoName,
}

/// The alias an import saves the repository under: the one asked for, or
/// the profile's file name as a slug.
fn repository_alias(file: &Path, repo_name: Option<&str>) -> Result<RepositoryAlias, AliasProblem> {
    if let Some(name) = repo_name {
        return RepositoryAlias::parse(name).map_err(|_| AliasProblem::BadName(name.to_owned()));
    }
    file.file_stem()
        .and_then(|stem| stem.to_str())
        .and_then(RepositoryAlias::slugify)
        .ok_or(AliasProblem::NoName)
}

/// Why an import saved nothing.
enum NotImported {
    /// There is no folder to keep the configuration in.
    NoConfigDir,
    /// There is no name to save the repository under.
    Alias(AliasProblem),
    /// The async runtime did not start.
    Runtime(io::Error),
    /// The import was refused, and the configuration is as it was.
    Refused(Refusal),
}

impl NotImported {
    /// Why, as the code, arguments and detail a document carries (§8.4).
    fn refusal(self) -> Refusal {
        match self {
            Self::NoConfigDir => config_folder_unknown(),
            Self::Alias(_) => repository_alias_invalid(),
            Self::Runtime(error) => runtime_unavailable(&error),
            Self::Refused(refusal) => refusal,
        }
    }

    /// Says why on stderr, as the catalog says it. `file` is the profile.
    fn say(&self, localizer: &Localizer, file: &Path) -> io::Result<()> {
        match self {
            Self::NoConfigDir => say(localizer, msg::CLI_NO_CONFIG_DIR, &[]),
            Self::Alias(AliasProblem::BadName(name)) => say(
                localizer,
                msg::CLI_PROFILE_BAD_REPOSITORY_NAME,
                &[("name", text(name))],
            ),
            Self::Alias(AliasProblem::NoName) => say(
                localizer,
                msg::CLI_PROFILE_NO_REPOSITORY_NAME,
                &[("file", text(file.display()))],
            ),
            Self::Runtime(error) => say(
                localizer,
                msg::CLI_CANNOT_START_RUNTIME,
                &[("detail", text(error))],
            ),
            Self::Refused(refusal) => refuse(localizer, refusal),
        }
    }
}

/// Does the import: what it added, or why it saved nothing.
fn import(file: &Path, repo_name: Option<&str>) -> Result<Imported, NotImported> {
    let config_dir = dirs::config_dir().ok_or(NotImported::NoConfigDir)?;
    let config_file = config_dir.join("config.toml");
    let alias = repository_alias(file, repo_name).map_err(NotImported::Alias)?;
    let runtime = tokio::runtime::Runtime::new().map_err(NotImported::Runtime)?;
    let config: Arc<dyn ConfigStore> = Arc::new(FileConfigStore::new(
        config_file.clone(),
        Arc::new(SystemClock),
    ));
    let reader = FileProfileReader::new(
        search_dirs(dirs::rustic_config_dir(), std::env::current_dir().ok()),
        rustic_variables(std::env::vars_os()),
    );
    let use_case = ImportProfile::new(Arc::new(reader), config, Arc::new(engine::RusticSources));
    runtime
        .block_on(use_case.import(file, alias))
        .map_err(|error| NotImported::Refused(import_refusal(error, &config_file)))
}

/// Says what the import did, in words: its notices on stderr, then what it
/// added on stdout; or why it saved nothing, on stderr.
fn tell_import(
    localizer: &Localizer,
    file: &Path,
    outcome: Result<Imported, NotImported>,
) -> io::Result<ExitCode> {
    let imported = match outcome {
        Ok(imported) => imported,
        Err(not_imported) => {
            not_imported.say(localizer, file)?;
            return Ok(ExitCode::FAILURE);
        }
    };
    for notice in imported.notices {
        warn(localizer, &profile_notice(notice))?;
    }
    let mut told = localizer
        .message(
            msg::CLI_PROFILE_IMPORTED,
            &[
                ("repository", text(imported.repository.as_str())),
                ("file", text(file.display())),
            ],
        )
        .to_string();
    told.push('\n');
    for plan in imported.plans {
        let line = localizer.message(
            msg::CLI_PROFILE_ADDED_PLAN,
            &[("plan", text(plan.as_str()))],
        );
        told.push_str(line.text());
        told.push('\n');
    }
    to_stdout(&told)?;
    Ok(ExitCode::SUCCESS)
}

/// Prints what the import did, or why it saved nothing, as one JSON document
/// on stdout, and nothing on stderr. A document that cannot be written has
/// nowhere to go: that is said in words, on stderr, by [`finish`].
fn document_import(outcome: Result<Imported, NotImported>) -> io::Result<ExitCode> {
    match outcome {
        Ok(imported) => {
            print_json(&import_document(imported))?;
            Ok(ExitCode::SUCCESS)
        }
        Err(not_imported) => {
            print_json(&failure_document(not_imported.refusal()))?;
            Ok(ExitCode::FAILURE)
        }
    }
}

/// Prints `document` as the one line `--json` prints on stdout.
fn print_json(document: &impl serde::Serialize) -> io::Result<()> {
    write_json(&mut io::stdout().lock(), document)
}

/// Imports the rustic profile in `file`. The repository is saved under
/// `repo_name`, or under the profile's file name made into a slug.
pub(crate) fn import_profile(file: PathBuf, repo_name: Option<String>, report: Report) -> ExitCode {
    let localizer = Localizer::new(&PosixEnv::from_process().preferences());
    let outcome = import(&file, repo_name.as_deref());
    let told = match report {
        Report::Text => tell_import(&localizer, &file, outcome),
        Report::Json => document_import(outcome),
    };
    finish(&localizer, told)
}

/// Exports the plan `plan` as a profile on stdout, with its notices on
/// stderr. A refusal prints nothing on stdout and exits with 1.
pub(crate) fn export_profile(plan: String) -> ExitCode {
    let localizer = Localizer::new(&PosixEnv::from_process().preferences());
    finish(&localizer, export(&localizer, &plan))
}

/// Does the export, and says what came of it.
fn export(localizer: &Localizer, plan: &str) -> io::Result<ExitCode> {
    let Some(config_dir) = dirs::config_dir() else {
        say(localizer, msg::CLI_NO_CONFIG_DIR, &[])?;
        return Ok(ExitCode::FAILURE);
    };
    let Some(data_dir) = dirs::data_dir() else {
        say(localizer, msg::CLI_NO_DATA_DIR, &[])?;
        return Ok(ExitCode::FAILURE);
    };
    let config_file = config_dir.join("config.toml");
    // A name that is not a plan ID is not a plan of the configuration either.
    let Ok(plan_id) = PlanId::parse(plan) else {
        refuse(localizer, &plan_unknown(plan))?;
        return Ok(ExitCode::FAILURE);
    };
    let agent = match agent_id::load_or_create(&data_dir) {
        Ok(agent) => agent,
        Err(error) => {
            say(
                localizer,
                msg::CLI_PROFILE_NO_AGENT_ID,
                &[("path", text(data_dir.display())), ("detail", text(error))],
            )?;
            return Ok(ExitCode::FAILURE);
        }
    };
    let runtime = match tokio::runtime::Runtime::new() {
        Ok(runtime) => runtime,
        Err(error) => {
            say(
                localizer,
                msg::CLI_CANNOT_START_RUNTIME,
                &[("detail", text(error))],
            )?;
            return Ok(ExitCode::FAILURE);
        }
    };
    let config: Arc<dyn ConfigStore> = Arc::new(FileConfigStore::new(
        config_file.clone(),
        Arc::new(SystemClock),
    ));
    // The PATH the profile is exported under: where its keyring command is
    // looked for.
    let path = std::env::var_os("PATH");
    let writer = RusticProfileWriter::new(KeyringCommand::of_this_platform(), path.as_deref());
    let use_case = ExportProfile::new(Arc::new(writer), config, agent);
    match runtime.block_on(use_case.export(&plan_id)) {
        Ok(exported) => {
            for notice in exported.notices {
                warn(localizer, &profile_notice(notice))?;
            }
            to_stdout(&exported.text)?;
            Ok(ExitCode::SUCCESS)
        }
        Err(error) => {
            refuse(localizer, &export_refusal(error, &config_file))?;
            Ok(ExitCode::FAILURE)
        }
    }
}

#[cfg(test)]
mod tests {
    use app::interop::ImportError;
    use app::ports::ProfileNotice;

    use super::*;

    fn os(name: &str) -> OsString {
        OsString::from(name)
    }

    #[test]
    fn a_used_profile_is_looked_for_in_rustics_folder_then_etc_then_here() {
        assert_eq!(
            search_dirs(
                Some(PathBuf::from("/home/demo/.config/rustic")),
                Some(PathBuf::from("/work"))
            ),
            [
                PathBuf::from("/home/demo/.config/rustic"),
                PathBuf::from("/etc/rustic"),
                PathBuf::from("/work"),
            ]
        );
    }

    #[test]
    fn a_missing_folder_is_left_out_of_the_search() {
        assert_eq!(search_dirs(None, None), [PathBuf::from("/etc/rustic")]);
    }

    #[test]
    fn only_rustics_variables_reach_the_importer() {
        let found = rustic_variables([
            (os("RUSTIC_PASSWORD"), os("hunter2")),
            (os("RUSTIC_REPO_OPT_ENDPOINT"), os("https://s3.example")),
            (os("HOME"), os("/home/demo")),
            (os("AWS_SECRET_ACCESS_KEY"), os("abc")),
            (os("rustic_password"), os("lowercase is not rustic's")),
        ]);
        assert_eq!(
            found.keys().map(String::as_str).collect::<Vec<_>>(),
            ["RUSTIC_PASSWORD", "RUSTIC_REPO_OPT_ENDPOINT"]
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_variable_that_is_not_text_is_left_out() {
        use std::os::unix::ffi::OsStringExt;
        let found = rustic_variables([
            (os("RUSTIC_PASSWORD"), OsString::from_vec(vec![0xff, 0xfe])),
            (OsString::from_vec(vec![b'R', 0xff]), os("x")),
            (os("RUSTIC_REPOSITORY"), os("/srv/repo")),
        ]);
        assert_eq!(
            found.keys().map(String::as_str).collect::<Vec<_>>(),
            ["RUSTIC_REPOSITORY"]
        );
    }

    #[test]
    fn an_import_is_reported_as_its_repository_its_plans_and_its_notices() {
        let imported = Imported {
            repository: RepositoryAlias::parse("nas").unwrap(),
            plans: vec![
                PlanId::parse("docs").unwrap(),
                PlanId::parse("home").unwrap(),
            ],
            notices: vec![
                ProfileNotice::KeyIgnored {
                    key: "copy".to_owned(),
                },
                ProfileNotice::Prune,
            ],
        };
        assert_eq!(
            serde_json::to_value(import_document(imported)).unwrap(),
            serde_json::json!({
                "repository": "nas",
                "plans": ["docs", "home"],
                "notices": [
                    {
                        "code": "profile-key-ignored",
                        "args": {"key": {"type": "text", "value": "copy"}},
                    },
                    {"code": "profile-prune"},
                ],
            })
        );
    }

    #[test]
    fn a_refusal_is_reported_as_the_typed_error_under_error() {
        let refusal = import_refusal(
            ImportError::PlanTaken(PlanId::parse("home").unwrap()),
            Path::new("/config/config.toml"),
        );
        assert_eq!(
            serde_json::to_value(failure_document(refusal)).unwrap(),
            serde_json::json!({
                "error": {
                    "code": "plan-id-taken",
                    "args": {"plan": {"type": "text", "value": "home"}},
                },
            })
        );
    }

    #[test]
    fn a_refusal_that_has_words_of_its_own_carries_them_as_the_detail() {
        let refusal = Refusal {
            code: api::ErrorCode::ProfileInvalid,
            args: vec![("file", text("nas.toml")), ("line", text(7))],
            detail: Some("unknown field".to_owned()),
        };
        assert_eq!(
            serde_json::to_value(failure_document(refusal)).unwrap(),
            serde_json::json!({
                "error": {
                    "code": "profile-invalid",
                    "args": {
                        "file": {"type": "text", "value": "nas.toml"},
                        "line": {"type": "text", "value": "7"},
                    },
                    "detail": "unknown field",
                },
            })
        );
    }

    /// An import that saved nothing, as the document `--json` prints.
    fn failure_of(not_imported: NotImported) -> serde_json::Value {
        serde_json::to_value(failure_document(not_imported.refusal())).unwrap()
    }

    #[test]
    fn an_import_with_no_folder_for_the_configuration_is_a_code_alone() {
        assert_eq!(
            failure_of(NotImported::NoConfigDir),
            serde_json::json!({"error": {"code": "config-folder-unknown"}})
        );
    }

    #[test]
    fn an_import_whose_runtime_did_not_start_carries_the_operating_systems_words() {
        assert_eq!(
            failure_of(NotImported::Runtime(io::Error::other(
                "Too many open files"
            ))),
            serde_json::json!({
                "error": {
                    "code": "runtime-unavailable",
                    "detail": "Too many open files",
                },
            })
        );
    }

    #[test]
    fn an_import_with_no_name_for_the_repository_is_a_code_alone_whichever_the_problem() {
        for problem in [
            AliasProblem::BadName("Not_A_Name".to_owned()),
            AliasProblem::NoName,
        ] {
            assert_eq!(
                failure_of(NotImported::Alias(problem)),
                serde_json::json!({"error": {"code": "repository-alias-invalid"}})
            );
        }
    }

    #[test]
    fn a_refusal_of_the_import_is_the_refusal_itself() {
        assert_eq!(
            failure_of(NotImported::Refused(plan_unknown("home"))),
            serde_json::json!({
                "error": {
                    "code": "plan-unknown",
                    "args": {"plan": {"type": "text", "value": "home"}},
                },
            })
        );
    }
}
