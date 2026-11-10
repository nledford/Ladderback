//! `ladderback import-profile` and `ladderback export-profile` (technical
//! design §6.2): wires the interop use cases to the config file, the agent's
//! ID and rustic's profile format, and reports what they did.
//!
//! Red commit: the commands run the use cases, which are `todo!()`.

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

use app::interop::{ExportProfile, ImportProfile};
use app::ports::ConfigStore;
use config_adapter::FileConfigStore;
use domain::{PlanId, RepositoryAlias};
use interop::{FileProfileReader, RusticProfileWriter};
use platform::{SystemClock, agent_id, dirs};

/// Imports the rustic profile in `file`. The repository is saved under
/// `repo_name`, or under the profile's file name made into a slug.
#[allow(clippy::todo, reason = "red test: interop import")]
pub(crate) fn import_profile(file: PathBuf, repo_name: Option<String>) -> ExitCode {
    let Some(config_dir) = dirs::config_dir() else {
        todo!("say there is no config folder")
    };
    let config: Arc<dyn ConfigStore> = Arc::new(FileConfigStore::new(
        config_dir.join("config.toml"),
        Arc::new(SystemClock),
    ));
    let named = repo_name.as_deref().map(RepositoryAlias::parse);
    let alias = match named {
        Some(Ok(alias)) => alias,
        Some(Err(_)) => todo!("refuse an alias that is not a slug"),
        None => match file
            .file_stem()
            .and_then(|stem| stem.to_str())
            .and_then(RepositoryAlias::slugify)
        {
            Some(alias) => alias,
            None => todo!("refuse a file name with no slug in it"),
        },
    };
    let Ok(runtime) = tokio::runtime::Runtime::new() else {
        todo!("say the runtime could not start")
    };
    let use_case = ImportProfile::new(Arc::new(FileProfileReader::new()), config);
    let outcome = runtime.block_on(use_case.import(&file, alias));
    let _ = outcome;
    todo!("print the summary, the notices and the refusal through the catalog")
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
