//! `ladderback import-profile` and `ladderback export-profile` (technical
//! design §6.2): wires the interop use cases to the config file, the agent's
//! ID and rustic's profile format, and reports what they did.
//!
//! The import's notices and refusals go to stderr as the catalog's messages
//! for their codes; what the import did goes to stdout. A refusal exits with
//! 1 and leaves the configuration as it was.
//!
//! Red commit: the export runs its use case, which is `todo!()`.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;

use app::interop::{ExportProfile, ImportProfile};
use app::ports::ConfigStore;
use config_adapter::FileConfigStore;
use domain::{PlanId, RepositoryAlias};
use interop::{FileProfileReader, RusticProfileWriter};
use l10n::{Localizer, PosixEnv, msg};
use platform::{SystemClock, agent_id, dirs};

use crate::composition::{Notice, Refusal, import_refusal, profile_notice};
use crate::{say, text};

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
fn refuse(localizer: &Localizer, refusal: &Refusal) {
    eprintln!(
        "ladderback: {}",
        localizer.error(refusal.code, &refusal.args, refusal.detail.as_deref())
    );
}

/// Prints a notice on stderr, as the catalog says it.
fn warn(localizer: &Localizer, notice: &Notice) {
    eprintln!(
        "ladderback: {}",
        localizer.warning(notice.code, &notice.args, None)
    );
}

/// The alias an import saves the repository under: the one asked for, or
/// the profile's file name as a slug. `None` after saying what is wrong.
fn repository_alias(
    localizer: &Localizer,
    file: &Path,
    repo_name: Option<&str>,
) -> Option<RepositoryAlias> {
    if let Some(name) = repo_name {
        return match RepositoryAlias::parse(name) {
            Ok(alias) => Some(alias),
            Err(_) => {
                say(
                    localizer,
                    msg::CLI_PROFILE_BAD_REPOSITORY_NAME,
                    &[("name", text(name))],
                );
                None
            }
        };
    }
    let alias = file
        .file_stem()
        .and_then(|stem| stem.to_str())
        .and_then(RepositoryAlias::slugify);
    if alias.is_none() {
        say(
            localizer,
            msg::CLI_PROFILE_NO_REPOSITORY_NAME,
            &[("file", text(file.display()))],
        );
    }
    alias
}

/// Imports the rustic profile in `file`. The repository is saved under
/// `repo_name`, or under the profile's file name made into a slug.
pub(crate) fn import_profile(file: PathBuf, repo_name: Option<String>) -> ExitCode {
    let localizer = Localizer::new(&PosixEnv::from_process().preferences());
    let Some(config_dir) = dirs::config_dir() else {
        say(&localizer, msg::CLI_NO_CONFIG_DIR, &[]);
        return ExitCode::FAILURE;
    };
    let config_file = config_dir.join("config.toml");
    let Some(alias) = repository_alias(&localizer, &file, repo_name.as_deref()) else {
        return ExitCode::FAILURE;
    };
    let runtime = match tokio::runtime::Runtime::new() {
        Ok(runtime) => runtime,
        Err(error) => {
            say(
                &localizer,
                msg::CLI_CANNOT_START_RUNTIME,
                &[("detail", text(error))],
            );
            return ExitCode::FAILURE;
        }
    };
    let config: Arc<dyn ConfigStore> = Arc::new(FileConfigStore::new(
        config_file.clone(),
        Arc::new(SystemClock),
    ));
    let reader = FileProfileReader::new(
        search_dirs(dirs::rustic_config_dir(), std::env::current_dir().ok()),
        rustic_variables(std::env::vars_os()),
    );
    let use_case = ImportProfile::new(Arc::new(reader), config);
    match runtime.block_on(use_case.import(&file, alias)) {
        Ok(imported) => {
            for notice in imported.notices {
                warn(&localizer, &profile_notice(notice));
            }
            println!(
                "{}",
                localizer.message(
                    msg::CLI_PROFILE_IMPORTED,
                    &[
                        ("repository", text(imported.repository.as_str())),
                        ("file", text(file.display())),
                    ],
                )
            );
            for plan in imported.plans {
                println!(
                    "{}",
                    localizer.message(
                        msg::CLI_PROFILE_ADDED_PLAN,
                        &[("plan", text(plan.as_str()))],
                    )
                );
            }
            ExitCode::SUCCESS
        }
        Err(error) => {
            refuse(&localizer, &import_refusal(error, &config_file));
            ExitCode::FAILURE
        }
    }
}

/// Exports the plan `plan` as a profile on stdout, with its notices on
/// stderr.
#[allow(clippy::todo, reason = "red test: interop export")]
pub(crate) fn export_profile(plan: String) -> ExitCode {
    let Some(config_dir) = dirs::config_dir() else {
        todo!("say there is no config folder")
    };
    let Some(data_dir) = dirs::data_dir() else {
        todo!("say there is no data folder")
    };
    let Ok(agent) = agent_id::load_or_create(&data_dir) else {
        todo!("say the agent's ID could not be read")
    };
    let Ok(plan) = PlanId::parse(&plan) else {
        todo!("refuse a plan ID that is not a slug")
    };
    let config: Arc<dyn ConfigStore> = Arc::new(FileConfigStore::new(
        config_dir.join("config.toml"),
        Arc::new(SystemClock),
    ));
    let Ok(runtime) = tokio::runtime::Runtime::new() else {
        todo!("say the runtime could not start")
    };
    let use_case = ExportProfile::new(Arc::new(RusticProfileWriter::new()), config, agent);
    let outcome = runtime.block_on(use_case.export(&plan));
    let _ = outcome;
    todo!("print the profile, the notices and the refusal through the catalog")
}

#[cfg(test)]
mod tests {
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
}
