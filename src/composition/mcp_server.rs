//! The MCP server's wiring (technical design §4.6): `ladderback mcp
//! --connect` drives the daemon at the address it was given through the
//! connect backend, with the API token of its token file when there is one;
//! `--headless` runs a session of its own on Ladderback's folders, and
//! `--scratch` one in a scratch folder it seeds and deletes, both through
//! the in-process backend. Each names this program's build.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use api::MsgArg;
use app::ports::{ConfigStore as _, Random};
use async_trait::async_trait;
use config_adapter::FileConfigStore;
use domain::planning::{
    AgentConfig, AgentSettings, Hooks, Maintenance, Plan, RepositoryConfig, RepositorySettings,
    Source,
};
use domain::secrets::{FileRead, SecretRef};
use domain::{PlanId, RepositoryAlias};
use l10n::msg;
use platform::SystemClock;
use platform::data_dir_lock::DataDirLock;
use platform::scratch::{self, Scratch};

use super::runs::Runs;
use super::{Daemon, LOG_CLOSE_BOUND, OperationLog, Refused, close_within, daemon_at, text};
use crate::shutdown::WORKERS_BOUND;

/// What a headless or scratch session holds until it ends: its workers and
/// runs, which it stops as `serve` stops; the operation log it closes; the
/// data folder's lock it lets go of once the runtime is gone (§9); and its
/// scratch folder, deleted last.
pub(crate) struct Held {
    pub(crate) log: OperationLog,
    pub(crate) lock: DataDirLock,
    pub(crate) scratch: Option<Scratch>,
    workers: engine::Workers,
    runs: Runs,
}

impl Held {
    /// Stops the session as `serve` stops at its signal (decision 92): no
    /// run starts a worker, runs a hook or records an end from here on; the
    /// workers are asked to stop and waited for; then the operation log ends
    /// what is still running as `daemon-shut-down` and closes. Operations
    /// still queued stay queued for the next `serve`, as `serve` leaves
    /// them (decision 73). Call it once the session has ended, inside the
    /// runtime.
    pub(crate) async fn stop(&self) {
        let workers_stopped = self.workers.stop();
        self.runs.freeze();
        if tokio::time::timeout(WORKERS_BOUND, workers_stopped)
            .await
            .is_err()
        {
            log::warn!(
                "stopping: the workers did not stop within {} s; the next start ends what they ran",
                WORKERS_BOUND.as_secs()
            );
        }
        close_within(
            self.log.close_at_orderly_stop(),
            &self.log.database,
            LOG_CLOSE_BOUND,
        )
        .await;
    }

    /// Lets go of the data folder, then deletes the scratch folder, if the
    /// session has one. Call it once [`Held::stop`] has returned and the
    /// runtime is gone. A folder that can't be deleted is said in the log;
    /// the next scratch session sweeps it.
    pub(crate) fn release(self) {
        drop(self.lock);
        if let Some(scratch) = self.scratch {
            let path = scratch.path().to_owned();
            if let Err(error) = scratch.remove() {
                log::warn!(
                    "the scratch folder {} couldn't be deleted, so the next scratch session deletes it: {error}",
                    path.display()
                );
            }
        }
    }
}

/// What a server says its refusals through: the locale of the environment
/// the session runs in, found as for the command line's own messages, since
/// an MCP client asks for none (§4.6, §8.4).
fn localizer() -> l10n::Localizer {
    l10n::Localizer::new(&l10n::PosixEnv::from_process().preferences())
}

/// The server for a session that drives the daemon at `daemon`, sending the
/// API token of `token_file`, or else of `mcp-token` in the configuration
/// folder `config_dir`, when either holds one.
pub(crate) fn connect(
    daemon: ::mcp::DaemonUrl,
    token_file: Option<&Path>,
    config_dir: Option<&Path>,
) -> Result<::mcp::Server, Refused> {
    let token = api_token(token_file, config_dir)?;
    let backend = ::mcp::Connect::new(daemon).map_err(|error| Refused {
        message: msg::CLI_MCP_CANNOT_START,
        args: vec![("detail", text(error))],
    })?;
    let backend = match token {
        Some(token) => backend
            .with_token(token)
            .map_err(|::mcp::TokenOverPlainHttp| Refused {
                message: msg::CLI_MCP_TOKEN_OVER_PLAIN_HTTP,
                args: Vec::new(),
            })?,
        None => backend,
    };
    // One client serves both: the proposal tools reach the daemon as the
    // other tools do, with the same token, which approves nothing.
    let backend = Arc::new(backend);
    Ok(::mcp::Server::new(
        Arc::clone(&backend) as Arc<dyn ::mcp::Backend>,
        crate::build_info::running(),
        localizer(),
    )
    .proposing(backend, Arc::new(FileTables)))
}

/// A proposed plan's table, given as TOML text, read with the
/// configuration's own parser (§4.6); the daemon checks the plan.
struct FileTables;

impl ::mcp::PlanTables for FileTables {
    fn read(&self, text: &str) -> Option<api::PlanDto> {
        config_adapter::plan_table(text).ok().map(api::PlanDto)
    }
}

/// The API token connect mode sends: the one in `token_file`, which must be
/// there, or else in `mcp-token` in `config_dir`, which need not be: without
/// it, no token is sent (§4.6).
///
/// # Errors
///
/// The file `--token-file` names isn't there; or the file read can't be
/// read, others can read it, or it holds no token.
fn api_token(
    token_file: Option<&Path>,
    config_dir: Option<&Path>,
) -> Result<Option<::mcp::ApiToken>, Refused> {
    let path: PathBuf = match (token_file, config_dir) {
        (Some(path), _) => path.to_owned(),
        (None, Some(folder)) => folder.join(platform::token_file::DEFAULT_NAME),
        (None, None) => return Ok(None),
    };
    let unreadable = |detail: String| Refused {
        message: msg::CLI_MCP_TOKEN_FILE_UNREADABLE,
        args: vec![("file", text(path.display())), ("detail", text(detail))],
    };
    let held = match platform::token_file::read(&path) {
        Ok(Some(held)) => held,
        Ok(None) if token_file.is_some() => {
            return Err(Refused {
                message: msg::CLI_MCP_TOKEN_FILE_MISSING,
                args: vec![("file", text(path.display()))],
            });
        }
        Ok(None) => return Ok(None),
        Err(error) => return Err(unreadable(error.detail)),
    };
    ::mcp::ApiToken::parse(&held)
        .map(Some)
        .ok_or_else(|| unreadable("it holds no API token".to_owned()))
}

/// The server for a headless session on Ladderback's folders, which it
/// holds as `serve` does: one a daemon or another session holds is refused.
/// It starts with `serve`'s recovery, and plans and runs nothing.
pub(crate) async fn headless() -> Result<(::mcp::Server, Held), Refused> {
    let daemon = daemon_at(
        platform::dirs::config_dir(),
        platform::dirs::data_dir(),
        platform::dirs::cache_dir(),
    )
    .await?;
    Ok(in_process(daemon, None))
}

/// The plan, and the repository it backs up to, a scratch session is
/// seeded with.
const SCRATCH_NAME: &str = "scratch";

/// The files a scratch session's plan backs up.
const SCRATCH_FILES: usize = 3;

/// The server for a session in a new scratch folder in the temporary
/// folder: a plan over a few generated files, backing up to a repository of
/// its own whose password is a file in the folder. The scratch folders
/// crashed sessions left are swept first.
pub(crate) async fn scratch(random: &dyn Random) -> Result<(::mcp::Server, Held), Refused> {
    let parent = std::env::temp_dir();
    let swept = scratch::sweep(&parent);
    for path in &swept.removed {
        log::info!(
            "deleted the scratch folder {}, which a session that crashed left",
            path.display()
        );
    }
    for (path, error) in &swept.failed {
        log::warn!(
            "the scratch folder {}, which a session that crashed left, couldn't be deleted: {error}",
            path.display()
        );
    }
    let unavailable = |detail: String| scratch_unavailable(&parent, detail);
    let name = random.token().map_err(|error| unavailable(error.0))?;
    let name = hex(name.get(..8).unwrap_or_default());
    let folder = Scratch::create(&parent, &name).map_err(|error| unavailable(error.to_string()))?;
    match seeded(&folder, random).await {
        Ok(daemon) => Ok(in_process(daemon, Some(folder))),
        Err(unseeded) => {
            // Nothing of a session that never started is left.
            let _ = folder.remove();
            Err(match unseeded {
                Unseeded::Refused(refused) => refused,
                Unseeded::Failed(detail) => unavailable(detail),
            })
        }
    }
}

/// `cli-mcp-scratch-unavailable`: no scratch folder could be made in
/// `parent`, for the reason `detail` gives.
fn scratch_unavailable(parent: &Path, detail: String) -> Refused {
    Refused {
        message: msg::CLI_MCP_SCRATCH_UNAVAILABLE,
        args: vec![
            ("path", text(parent.display())),
            ("detail", MsgArg::Text(detail)),
        ],
    }
}

/// Why a scratch session couldn't start: the session's own refusal, as
/// `serve`'s start would say it, or what went wrong in seeding its folder.
enum Unseeded {
    Refused(Refused),
    Failed(String),
}

impl From<String> for Unseeded {
    fn from(detail: String) -> Self {
        Self::Failed(detail)
    }
}

impl From<std::io::Error> for Unseeded {
    fn from(error: std::io::Error) -> Self {
        Self::Failed(error.to_string())
    }
}

/// A session over `folder`, once it holds the generated files, the
/// password, the configuration and the repository.
async fn seeded(folder: &Scratch, random: &dyn Random) -> Result<Daemon, Unseeded> {
    let root = folder.path();
    let files = root.join("files");
    let config = root.join("config");
    let repository = root.join("repository");
    std::fs::create_dir(&files)?;
    std::fs::create_dir(&config)?;
    for n in 1..=SCRATCH_FILES {
        let lines: String = (1..=n.saturating_mul(10))
            .map(|line| format!("Scratch file {n}, line {line}.\n"))
            .collect();
        std::fs::write(files.join(format!("file-{n}.txt")), lines)?;
    }
    let password = random.token().map_err(|error| error.0)?;
    let password = folder.write_private("password", format!("{}\n", hex(&password)).as_bytes())?;
    FileConfigStore::new(config.join("config.toml"), Arc::new(SystemClock))
        .save(&scratch_config(&files, &repository, &password)?)
        .await
        .map_err(|error| error.to_string())?;
    // Checked before the session starts, so a failure leaves no log open.
    let settings = api::RepositorySettingsDto {
        repository: utf8(&repository)?.to_owned(),
        repo_hot: None,
        password: api::PasswordDto::File {
            path: utf8(&password)?.to_owned(),
        },
        options: BTreeMap::new(),
        options_hot: BTreeMap::new(),
        options_cold: BTreeMap::new(),
    };
    let daemon = daemon_at(
        Some(config),
        Some(root.join("data")),
        Some(root.join("cache")),
    )
    .await
    .map_err(Unseeded::Refused)?;
    let created = daemon
        .state
        .repositories
        .init(api::InitRepositoryDto {
            alias: None,
            settings,
            keep_in_keyring: false,
        })
        .await;
    if let Err(error) = created {
        daemon.log.close().await;
        return Err(Unseeded::Failed(format!(
            "the repository couldn't be created: {error:?}"
        )));
    }
    Ok(daemon)
}

/// The configuration of a scratch session: the repository at `repository`,
/// opened with the password in `password` and maintained here, and a plan
/// that backs `files` up to it.
fn scratch_config(files: &Path, repository: &Path, password: &Path) -> Result<AgentConfig, String> {
    let alias = RepositoryAlias::parse(SCRATCH_NAME).map_err(|error| error.to_string())?;
    let plan = PlanId::parse(SCRATCH_NAME).map_err(|error| error.to_string())?;
    let source = Source::parse(utf8(files)?).map_err(|error| error.to_string())?;
    let entry = RepositoryConfig {
        settings: RepositorySettings {
            repository: utf8(repository)?.to_owned(),
            repo_hot: None,
            password: SecretRef::File {
                path: utf8(password)?.to_owned(),
                read: FileRead::FirstLine,
            },
            options: BTreeMap::new(),
            options_hot: BTreeMap::new(),
            options_cold: BTreeMap::new(),
        },
        locations: Default::default(),
        id: None,
        maintenance: Maintenance::Local,
        group_by: Default::default(),
        hooks: Hooks::default(),
    };
    AgentConfig::new(
        AgentSettings::default(),
        Hooks::default(),
        BTreeMap::from([(alias.clone(), entry)]),
        vec![Plan::new(plan, alias, vec![source])],
    )
    .map_err(|error| error.to_string())
}

/// `path` as text: the configuration holds paths as text.
fn utf8(path: &Path) -> Result<&str, String> {
    path.to_str()
        .ok_or_else(|| format!("{} isn't valid UTF-8", path.display()))
}

/// `bytes` as lowercase hexadecimal.
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// The server over `daemon`'s state through the in-process backend, and
/// what the session holds. The queue's leftovers and the scheduler are left
/// unstarted: a session plans no runs, and runs only those it is asked for.
fn in_process(daemon: Daemon, scratch: Option<Scratch>) -> (::mcp::Server, Held) {
    let Daemon {
        state,
        log,
        lock,
        background,
        workers,
    } = daemon;
    let server = ::mcp::Server::new(
        Arc::new(InProcess::over(&state)),
        crate::build_info::running(),
        localizer(),
    );
    let held = Held {
        log,
        lock,
        scratch,
        workers,
        runs: background.runs(),
    };
    (server, held)
}

/// The in-process [`mcp::Backend`] of headless and scratch modes: the
/// handles the router of `serve` reads, called directly.
pub(crate) struct InProcess {
    catalog: Arc<dyn http_adapter::Catalog>,
    operations: Arc<dyn http_adapter::Operations>,
    repositories: Arc<dyn http_adapter::Repositories>,
    profiles: Arc<dyn http_adapter::Profiles>,
}

impl InProcess {
    /// A backend over the handles of `state`.
    pub(crate) fn over(state: &http_adapter::AppState) -> Self {
        Self {
            catalog: Arc::clone(&state.catalog),
            operations: Arc::clone(&state.operations),
            repositories: Arc::clone(&state.repositories),
            profiles: Arc::clone(&state.profiles),
        }
    }
}

#[async_trait]
impl ::mcp::Backend for InProcess {
    async fn status(&self) -> Result<::mcp::BackendStatus, api::ErrorDto> {
        let listed = self.plans().await?;
        let mut plans = Vec::with_capacity(listed.plans.len());
        for plan in listed.plans {
            let page = self.history(Some(plan.id.clone()), None, 1).await?;
            plans.push(::mcp::PlanStatus {
                last: page.operations.into_iter().next(),
                plan,
            });
        }
        let recent = self.history(None, None, ::mcp::RECENT_OPERATIONS).await?;
        Ok(::mcp::BackendStatus {
            access: ::mcp::Access::InProcess,
            daemon: None,
            plans,
            running: ::mcp::unfinished(recent.operations),
        })
    }

    async fn plans(&self) -> Result<api::PlansDto, api::ErrorDto> {
        self.catalog.plans().await.map_err(catalog_refusal)
    }

    async fn repositories(&self) -> Result<api::RepositoriesDto, api::ErrorDto> {
        self.catalog.repositories().await.map_err(catalog_refusal)
    }

    async fn snapshots(&self, repository: String) -> Result<api::SnapshotsDto, api::ErrorDto> {
        use http_adapter::SnapshotsError;
        self.catalog
            .snapshots(repository.clone())
            .await
            .map_err(|error| match error {
                SnapshotsError::Unknown => api::ErrorDto::new(
                    api::ErrorCode::RepositoryUnknown,
                    [("alias", MsgArg::Text(repository))],
                    None,
                ),
                SnapshotsError::SecretRefused(secret) => {
                    api::ErrorDto::new(secret.code, secret.args, secret.detail)
                }
                SnapshotsError::RepositoryNotFound => {
                    api::ErrorDto::new(api::ErrorCode::RepositoryNotFound, [], None)
                }
                SnapshotsError::RepositoryConfigNotFound { detail } => {
                    api::ErrorDto::new(api::ErrorCode::RepositoryConfigNotFound, [], Some(detail))
                }
                SnapshotsError::Unavailable { code, args, detail } => {
                    api::ErrorDto::new(code, args, detail)
                }
                SnapshotsError::TimedOut => {
                    api::ErrorDto::new(api::ErrorCode::RepositoryReadTimedOut, [], None)
                }
            })
    }

    async fn history(
        &self,
        plan: Option<String>,
        after: Option<String>,
        limit: u64,
    ) -> Result<api::OperationsPageDto, api::ErrorDto> {
        self.operations
            .history(plan, after.clone(), Some(limit))
            .await
            .map_err(|http_adapter::HistoryError::BadCursor| {
                api::ErrorDto::new(
                    api::ErrorCode::InvalidCursor,
                    after.map(|cursor| ("cursor", MsgArg::Text(cursor))),
                    None,
                )
            })
    }

    async fn operation(&self, id: String) -> Result<api::OperationDetailDto, api::ErrorDto> {
        self.operations
            .operation(id.clone())
            .await
            .map_err(|http_adapter::LookupError::Unknown| {
                api::ErrorDto::new(
                    api::ErrorCode::OperationUnknown,
                    [("operation", MsgArg::Text(id))],
                    None,
                )
            })
    }

    /// Read from the detail: this process reads its own store, with no cap
    /// on what it reads.
    async fn summary(&self, id: String) -> Result<api::OperationSummaryDto, api::ErrorDto> {
        Ok(self.operation(id).await?.operation)
    }

    async fn run(&self, plan: String) -> Result<String, api::ErrorDto> {
        use http_adapter::StartError;
        self.operations
            .start_backup(plan.clone(), domain::execution::Trigger::Mcp)
            .await
            .map_err(|error| match error {
                StartError::UnknownPlan => api::ErrorDto::new(
                    api::ErrorCode::PlanUnknown,
                    [("plan", MsgArg::Text(plan))],
                    None,
                ),
                StartError::SecretRefused(secret) => {
                    api::ErrorDto::new(secret.code, secret.args, secret.detail)
                }
                StartError::Unavailable { code, args, detail } => {
                    api::ErrorDto::new(code, args, detail)
                }
                StartError::KeyFilesNotLeftOut => {
                    api::ErrorDto::new(api::ErrorCode::KeyFilesNotLeftOut, [], None)
                }
            })
    }

    /// Cancels as an API token may, a backup only (decision 93), whose
    /// refusal the domain's rule gives.
    async fn cancel(&self, id: String) -> Result<(), api::ErrorDto> {
        use http_adapter::CancelError;
        self.operations
            .cancel(id.clone(), http_adapter::Canceller::ApiToken)
            .await
            .map_err(|error| match error {
                CancelError::SessionOnly => ::mcp::not_cancellable(),
                CancelError::Unknown => api::ErrorDto::new(
                    api::ErrorCode::OperationUnknown,
                    [("operation", MsgArg::Text(id))],
                    None,
                ),
                CancelError::NotRunning => api::ErrorDto::new(
                    api::ErrorCode::OperationNotRunning,
                    [("operation", MsgArg::Text(id))],
                    None,
                ),
            })
    }

    async fn test_repository(
        &self,
        repository: String,
    ) -> Result<api::LocationsTestDto, api::ErrorDto> {
        self.repositories
            .test_saved(repository)
            .await
            .map_err(|error| error.error())
    }

    async fn preview_forget(
        &self,
        repository: String,
    ) -> Result<api::MaintenancePreviewDto, api::ErrorDto> {
        self.operations
            .preview(repository.clone(), http_adapter::MaintenanceRequest::Forget)
            .await
            .map_err(|refusal| refusal.error(&repository))
    }

    async fn preview_import(
        &self,
        content: String,
        repository: String,
    ) -> Result<api::ProfileImportDto, api::ErrorDto> {
        self.profiles
            .preview(api::ProfilePreviewDto {
                content,
                repository,
            })
            .await
            .map_err(http_adapter::ProfilesError::error)
    }

    async fn export_profile(&self, plan: String) -> Result<api::ProfileExportDto, api::ErrorDto> {
        self.profiles
            .export(plan, api::ProfileCommands::LeftOut)
            .await
            .map_err(http_adapter::ProfilesError::error)
    }
}

/// A catalog's refusal as the API answers it: the configuration's
/// `config-*` code, its arguments and the store's words.
fn catalog_refusal(error: http_adapter::CatalogError) -> api::ErrorDto {
    let http_adapter::CatalogError::Unavailable { code, args, detail } = error;
    api::ErrorDto::new(code, args, detail)
}

#[cfg(test)]
mod tests;
