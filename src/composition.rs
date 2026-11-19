//! The daemon's wiring (technical design §3.2, §4.2): the one place beside
//! `main` that knows every adapter. `serve` opens the store and the config,
//! builds the engine, secrets and the event bus, and hands the result to the
//! HTTP router. Adapters stay constructor-injected; nothing else sees them.
//!
//! A startup that cannot happen is a typed exit: [`Refused`] carries the
//! catalog message and its arguments, and `serve` prints it and fails — never
//! a silent empty configuration or a panic.

use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;

use api::{ErrorCode, MsgArg, UiEvent, WarningCode};
use app::announce::failure;
use app::browse::{BrowseError, BrowseSnapshot};
use app::event_bus::BroadcastEventBus;
use app::execution::{Finished, RunOperation};
use app::full_disk_access::{FullDiskAccessCheck, StartNote};
use app::history::{GetOperation, ListOperations};
use app::locations::{ChooseLocation, LocationReport};
use domain::execution::locations::{LocationOutcome, LocationsRecord};

use app::conditions::{NameWatch, NetworkWait};
use app::notifications::{Notify, Report};
use app::ports::DELIVERY_TIMEOUT;
use app::ports::Retention;
use app::ports::SecretResolver;
use app::ports::{
    AdminPasswordError, BackupJob, Clock, ConfigError, ConfigStore, Engine, EngineError,
    EngineEvent, EngineResult, EventBus, HookRunner, HookSetup, Job, Network, NetworkLink,
    NormalizeSources, OperationRecord, OperationStore, PowerSupply, RELEASE_TIMEOUT, Random,
    ReleaseFeed, ReleaseFeedError, RunningJob, ScheduleStore, SnapshotSummary,
};
use app::ports::{Keyring, KeyringError};
use app::progress::LiveProgress;
use app::queue::{Place, RunQueue};
use app::reads::ReadTurns;
use app::recovery::RecoverOperations;
use app::repositories::{
    ChangeMaintainer, ConfigWrites, ConnectRepository, CreateOffers, InitRepository,
    MaintainerError, RecordOpened, Recording, RepositoryError, SavedEntryError, TestRepository,
};
use app::schedule::{PlanRuns, ScheduleReport, Scheduler};
use app::secrets::{Cause, Field, ResolveError, ResolveRepository};
use app::snapshots::{ListSnapshots, SnapshotError};
use app::status::{ReadStatus, StatusError};
use app::updates::{CheckForUpdates, ReleaseCheckReport, UpdateNotice};
use async_trait::async_trait;
use config_adapter::FileConfigStore;
use domain::RepositoryAlias;
use domain::browse::FolderPath;
use domain::execution::FailureReason;
use domain::execution::deadline::backup_budget;
use domain::execution::plan_tags::PlanTags;
use domain::execution::{OperationKind, OperationStatus, Target, Trigger, WarningKind};
use domain::planning::{
    InterfaceKind, Link, Location, LocationRule, Maintenance, MixedLinkNote, NameFilter,
    NotRecorded, Plan, RepositoryConfig, RepositoryField, RepositorySettings, RestoreFolder,
    RuleNote,
};
use domain::secrets::{
    FileRead, KeyringEntry, MaskedSecrets, Part, Redactor, RepositorySecrets, SecretRef,
    SecretValue, SecretWarning, Template,
};
use domain::updates::Version;
use domain::{AgentId, OperationId, PlanId, RepositoryId, SnapshotId};
use engine::{Recorded, RemovedKind, WorkerEngine};
use futures::stream::StreamExt;
use hooks::ProcessHookRunner;
use http_adapter::{
    AppState, Catalog, CatalogError, FolderError, MaintenanceRefusal, Operations, Plans,
    Repositories, RepositoriesError, SecretRefusal, SignIn, SnapshotsError, StartError,
};
use l10n::{MessageId, msg};
use notify_adapter::HttpNotifier;
use platform::SystemClock;
use platform::admin_password::FileAdminPassword;
use platform::data_dir_lock::{DataDirLock, LOCK_FILE, LockError};
use platform::random::OsRandom;
use platform::{agent_id, dirs};
use releases_adapter::GitHubReleases;
use secrets::SystemSecrets;
use store::SqliteStore;
use uuid::Uuid;

mod downloads;
mod dry_runs;
mod key_files;
mod maintenance;
pub(crate) mod mcp_server;
mod plans;
pub(crate) mod profile_refusals;
mod profiles;
mod proposals;
mod restores;
mod runs;

use key_files::KeyFilesLeftOut;

/// Why the daemon cannot serve, as a message for the person who started it.
pub(crate) struct Refused {
    pub(crate) message: MessageId,
    pub(crate) args: Vec<(&'static str, MsgArg)>,
}

/// A refusal with no arguments.
fn plain(message: MessageId) -> Refused {
    Refused {
        message,
        args: Vec::new(),
    }
}

/// A text argument, the kind identifiers, paths and versions are.
fn text(value: impl std::fmt::Display) -> MsgArg {
    MsgArg::Text(value.to_string())
}

/// Says in the log, once at load, each secret the configuration holds as
/// text (§10.2), in every location, and each location that sends its
/// credentials where whoever answers at its address can read them (§10.6):
/// the repository and the setting or the cause, in the catalog's words,
/// never the secret (I4) nor the address.
fn log_inline_secrets(config: &domain::planning::AgentConfig) {
    let localizer = l10n::Localizer::new(&l10n::PosixEnv::from_process().preferences());
    for (alias, repository) in config.repositories() {
        let secrets = inline_secret_warnings(repository)
            .into_iter()
            .map(|(_, code, args)| (code, args));
        let exposed = repository.location_warnings().into_iter().map(|warning| {
            (
                WarningCode::LocationPlainHttpCredentials,
                exposed_args(repository, warning),
            )
        });
        for (code, args) in secrets.chain(exposed) {
            let message = localizer.warning(code, &args, None);
            log::warn!("repository {}: {message}", alias.as_str());
        }
    }
}

/// How long the daemon waits on one look at a plan's sources for its globs
/// (#510): a source on a mount that stopped answering is then read as
/// written, and holds nothing, so neither the daemon's start nor the plans
/// list waits on it.
const SOURCES_LOOK_LIMIT: std::time::Duration = std::time::Duration::from_secs(1);

/// `S`, with each look at the disk given up after its limit, as one that
/// can't be resolved.
struct WithinLimit<S>(S, std::time::Duration);

#[async_trait]
impl<S: NormalizeSources> NormalizeSources for WithinLimit<S> {
    async fn normalized(
        &self,
        sources: &[domain::planning::Source],
    ) -> Result<Vec<domain::planning::Source>, app::ports::SourcesError> {
        tokio::time::timeout(self.1, self.0.normalized(sources))
            .await
            .unwrap_or_else(|_| {
                Err(app::ports::SourcesError::Unreadable {
                    detail: format!("no answer within {:?}", self.1),
                })
            })
    }
}

/// Says in the log, once at load, each glob of a plan that can never match
/// (#510): the plan, the glob and the whole path to write instead, read
/// against the plan's sources as the backup walks them.
async fn log_never_matching_globs(
    config: &domain::planning::AgentConfig,
    sources: &dyn NormalizeSources,
) {
    let localizer = l10n::Localizer::new(&l10n::PosixEnv::from_process().preferences());
    for plan in config.plans() {
        for (code, args) in never_matching_glob_warnings(plan, sources).await {
            let message = localizer.warning(code, &args, None);
            log::warn!("{message}");
        }
    }
}

/// A `glob-never-matches` warning, with its arguments, for each glob of
/// `plan` that can never match, read against its sources as `sources` walks
/// them (#510).
async fn never_matching_glob_warnings(
    plan: &Plan,
    sources: &dyn NormalizeSources,
) -> Vec<(WarningCode, Vec<(&'static str, MsgArg)>)> {
    app::globs::never_matching_globs(plan, sources)
        .await
        .into_iter()
        .map(|glob| glob_never_matches(plan.id.as_str(), &glob.glob, glob.absolute.as_deref()))
        .collect()
}

/// The `glob-never-matches` warning for `glob` of the plan `plan`, with
/// `absolute`, the whole path to write instead, left out when there is none.
fn glob_never_matches(
    plan: &str,
    glob: &str,
    absolute: Option<&str>,
) -> (WarningCode, Vec<(&'static str, MsgArg)>) {
    let mut args = vec![("plan", text(plan)), ("glob", text(glob))];
    args.extend(absolute.map(|absolute| ("absolute", text(absolute))));
    (WarningCode::GlobNeverMatches, args)
}

/// A warning's arguments, by name.
type WarningArgs = Vec<(&'static str, MsgArg)>;

/// The warning for each secret `repository` holds as text, with the
/// position, from 1, of the location that holds it.
fn inline_secret_warnings(repository: &RepositoryConfig) -> Vec<(u8, WarningCode, WarningArgs)> {
    let settings = repository.location_settings();
    repository
        .inline_secrets()
        .into_iter()
        .filter_map(|(position, secret)| {
            let at = settings.get(usize::from(position).checked_sub(1)?)?;
            let (code, args) = inline_secret_warning(at, &secret, position);
            Some((position, code, args))
        })
        .collect()
}

/// The arguments of the `location-plain-http-credentials` warning about
/// `warning`, a location of `repository`: by position, cause and whether it
/// has a rule, and whether the repository has one location or several, for
/// the text of a repository of one names no location (#473).
fn exposed_args(
    repository: &RepositoryConfig,
    warning: domain::planning::LocationWarning,
) -> Vec<(&'static str, MsgArg)> {
    vec![
        ("position", text(warning.position)),
        ("cause", text(warning.cause.code())),
        ("rule", text(warning.rule_code())),
        ("locations", text(repository.locations_code())),
    ]
}

/// The warning for a secret the configuration holds as text, with its
/// arguments: one in a location is moved differently from a setting of its
/// own, and one in an rclone location differently again (§10.2). `settings`
/// open the location at `position`, from 1; a further one is named by it.
fn inline_secret_warning(
    settings: &RepositorySettings,
    secret: &domain::secrets::InlineSecret,
    position: u8,
) -> (WarningCode, Vec<(&'static str, MsgArg)>) {
    let field = ("field", MsgArg::Text(secret.field()));
    let (code, mut args) = match settings.location_of(secret) {
        Some(location) => (
            WarningCode::LocationSecret,
            vec![
                field,
                (
                    "kind",
                    location_kind(domain::secrets::rclone_location(location)),
                ),
            ],
        ),
        None => (WarningCode::ConfigInlineSecret, vec![field]),
    };
    if position > 1 {
        args.push(("position", text(position)));
    }
    (code, args)
}

/// The `kind` argument of a `location-secret` warning.
fn location_kind(rclone: bool) -> MsgArg {
    text(if rclone { "rclone" } else { "url" })
}

/// What `serve` runs: the router's state, the operation log it writes, the
/// hold on the data folder that keeps every other process off it, the work
/// it does with no request (the queue left from before it started and the
/// scheduler), and its workers. The caller stops the workers once it has
/// stopped serving, then closes the log, and keeps the lock until the
/// daemon is gone (§4.2, §9).
pub(crate) struct Daemon {
    pub(crate) state: AppState,
    pub(crate) log: OperationLog,
    pub(crate) lock: DataDirLock,
    pub(crate) background: Background,
    pub(crate) workers: engine::Workers,
}

/// What the conditions before a scheduled run ask the machine (§5): what
/// powers it, whether it is online, and the network link the rules of a
/// repository's locations are evaluated against (decision 80), its reads of
/// the Wi-Fi network's name watched for the Repositories page.
pub(crate) struct RunConditions {
    pub(crate) power: Arc<dyn PowerSupply>,
    pub(crate) network: Arc<dyn Network>,
    pub(crate) link: Arc<NameWatch>,
}

impl RunConditions {
    /// This machine's. The first name it can't read is said once in the
    /// log, never with what was read (decision 80).
    fn system() -> Self {
        Self {
            power: platform::power::system_power(),
            network: platform::network::system_network(),
            link: Arc::new(NameWatch::new(
                platform::network_link::system_link(),
                || {
                    log::warn!(
                        "can't read the name of the Wi-Fi network this machine is on: a location's rule that names Wi-Fi networks doesn't hold here, so that location is skipped; a rule of `wired` or `wifi` alone needs no name"
                    );
                },
            )),
        }
    }
}

/// The daemon's work that no request starts (§5): the backups the operation
/// log still holds as queued from before the start, then the scheduler; and
/// beside them the release check, when there is a feed to ask.
pub(crate) struct Background {
    runner: Arc<PlanRunner>,
    /// Where a restore makes its new folder when `[agent] restore-folder`
    /// is unset (§10.4): none on a machine without a home folder.
    restore_folder: Option<RestoreFolder>,
    /// What the proposals are kept and applied over, for the service the
    /// daemon builds again once sign-in is decided.
    proposals: proposals::ProposalParts,
    /// The queued operations recovery found, oldest first.
    queued: Vec<OperationRecord>,
    config: Arc<dyn ConfigStore>,
    schedules: Arc<dyn ScheduleStore>,
    store: Arc<dyn OperationStore>,
    conditions: RunConditions,
    /// How each backup ended, which the scheduler reads to retry a failed
    /// one (§5).
    ends: app::retry::RunEnds,
    /// The changes of the configuration a request saved, which wake the
    /// scheduler to read it again at once (decision 96).
    changes: app::schedule::ConfigChanges,
    /// What the header shows of the release check's findings.
    notice: Arc<UpdateNotice>,
    /// Where the release check asks, `None` for a daemon that never checks.
    releases: Option<Releases>,
}

/// What the release check asks (decision 107): its feed, and the version it
/// compares the feed's newest release with.
pub(crate) struct Releases {
    feed: Arc<dyn ReleaseFeed>,
    running: Version,
}

impl Background {
    /// What the daemon's orderly stop freezes: every run of an operation,
    /// whoever started it (decision 92).
    pub(crate) fn runs(&self) -> runs::Runs {
        self.runner.runs.clone()
    }

    /// Runs the queue's leftovers, oldest first, then the scheduler, with
    /// `random` spreading its runs over their jitter, and beside them the
    /// release check, with `random` spreading its checks too, until the
    /// future is dropped.
    pub(crate) async fn run(self, random: Arc<dyn Random>) {
        let check = self.releases.map(|releases| CheckForUpdates {
            clock: Arc::new(SystemClock),
            random: Arc::clone(&random),
            feed: releases.feed,
            schedules: Arc::clone(&self.schedules),
            config: Arc::clone(&self.config),
            running: releases.running,
            notice: self.notice,
            reports: Arc::new(log_release_check),
        });
        let checking = async {
            if let Some(check) = &check {
                check.run().await;
            }
        };
        let queue = Arc::clone(&self.runner.queue);
        let events = Arc::clone(&self.runner.events);
        let runs: Arc<dyn PlanRuns> = self.runner;
        let scheduling = async {
            resume_queued(
                self.queued,
                self.config.as_ref(),
                self.store,
                Arc::clone(&events),
                runs.as_ref(),
            )
            .await;
            Scheduler {
                clock: Arc::new(SystemClock),
                random,
                schedules: self.schedules,
                config: self.config,
                power: self.conditions.power,
                network: self.conditions.network,
                link: self.conditions.link,
                runs,
                queue,
                zone: domain::TimeZone::system(),
                ends: self.ends,
                events,
                changes: self.changes,
            }
            .run()
            .await;
        };
        tokio::join!(scheduling, checking);
    }
}

/// The variable that points the release check at another address than
/// GitHub's (decision 107). The tests, `just try` and the smoke test set it
/// to one nothing answers at, so none of them asks the network.
const RELEASES_URL_VARIABLE: &str = "LADDERBACK_RELEASES_URL";

/// Where the release check asks: GitHub Releases, or the address `url`
/// (the value of [`RELEASES_URL_VARIABLE`]) names. `None`, said once in the
/// log, when that address isn't one or the client can't be built: the
/// daemon then never checks, rather than ask an address it wasn't given.
fn releases(url: Option<std::ffi::OsString>) -> Option<Releases> {
    let running = match Version::parse(env!("CARGO_PKG_VERSION")) {
        Ok(running) => running,
        Err(_) => {
            log::warn!("the release check is off: this build's version isn't one it can compare");
            return None;
        }
    };
    let url = match url.map(std::ffi::OsString::into_string) {
        None => releases_adapter::LATEST_RELEASE.to_owned(),
        Some(Ok(url)) => url,
        Some(Err(_)) => {
            log::warn!("the release check is off: {RELEASES_URL_VARIABLE} isn't an address");
            return None;
        }
    };
    match GitHubReleases::new(&url, &running, RELEASE_TIMEOUT) {
        Ok(feed) => Some(Releases {
            feed: Arc::new(feed),
            running,
        }),
        Err(error) => {
            log::warn!("the release check is off: {error}");
            None
        }
    }
}

/// Says in the log what the release check did (decision 107): a new
/// available update once, with where its notes are; a check that couldn't
/// read the newest release only for debugging, as nothing is wrong with
/// the agent, and never with the feed's own words; the last check that
/// couldn't be read or kept by the store's code alone (I4).
fn log_release_check(report: ReleaseCheckReport) {
    match report {
        ReleaseCheckReport::UpdateAvailable(version) => log::info!(
            "Ladderback {version} is available; its release notes are at {}",
            version.release_notes()
        ),
        ReleaseCheckReport::Failed(ReleaseFeedError::Rejected { status }) => log::debug!(
            "the release check couldn't read the newest release: it was answered with status {status}"
        ),
        ReleaseCheckReport::Failed(error) => log::debug!(
            "the release check couldn't read the newest release ({})",
            error.reason()
        ),
        ReleaseCheckReport::Store(error) => log::warn!(
            "the release check couldn't read or keep its last check in the operation log ({}); it checks again on its schedule",
            error.code()
        ),
        // The scheduler says the configuration can't be read already.
        ReleaseCheckReport::Config(error) => {
            log::debug!("the release check didn't ask: the configuration can't be read: {error}")
        }
    }
}

/// Hands each of `queued` to the queue, oldest first, against the
/// configuration read once now. A configuration that can't be read builds
/// no job, so each ends as failed with `daemon-stopped`, and the log says
/// why (decision 73): left queued, it would show as queued with nothing to
/// run it until the next start. Each that ends is announced on `events`.
async fn resume_queued(
    queued: Vec<OperationRecord>,
    config: &dyn ConfigStore,
    store: Arc<dyn OperationStore>,
    events: Arc<dyn EventBus>,
    runs: &dyn PlanRuns,
) {
    let config = match config.load().await {
        Ok(config) => Some(config),
        Err(error) => {
            log::warn!(
                "the configuration can't be read, so any operation queued before the start ends as failed: {error}"
            );
            None
        }
    };
    RecoverOperations::new(store)
        .with_events(events)
        .resume(queued, config.as_ref(), runs, SystemClock.now())
        .await;
}

/// How long the daemon waits for the operation log to close once it stops
/// (§9): as long as a run waits for one write of its own
/// ([`app::log_write::SAVE_BOUND`]), which is past the store's own bound on
/// a call (about 15 s: 10 s for a connection, 5 s for the write lock). A
/// close waits for the calls in flight, so a busy store still finishes
/// them; one that hasn't by then is stalled (a disk that stopped
/// answering), and the daemon stops without it rather than hold up the
/// service manager, which would kill it anyway.
pub(crate) const LOG_CLOSE_BOUND: std::time::Duration =
    std::time::Duration::from_secs(app::log_write::SAVE_BOUND.as_secs().unsigned_abs());

/// The operation log the daemon writes, which it closes before it lets go
/// of the data folder.
pub(crate) struct OperationLog {
    pub(crate) store: Arc<SqliteStore>,
    database: std::path::PathBuf,
}

impl OperationLog {
    /// Closes the log once the calls in flight have ended, so no connection
    /// to it, and none of its `-wal` and `-shm` files, outlives the daemon.
    /// It waits [`LOG_CLOSE_BOUND`] at most, and says in the log when it
    /// gave up; what a call cut off then was still writing is ended at the
    /// next start, as after a crash.
    pub(crate) async fn close(&self) {
        close_within(self.store.close(), &self.database, LOG_CLOSE_BOUND).await;
    }

    /// Ends each operation still running as failed with `daemon-shut-down`
    /// at this moment, once the calls in flight have ended, and closes the
    /// log, however long that takes: the daemon's orderly stop, which calls
    /// it once its workers have stopped and bounds it as one of its steps
    /// (`shutdown`, decision 92). Nothing is announced. Ends it couldn't
    /// write, said in the log, are the next start's, as after a crash.
    pub(crate) async fn close_at_orderly_stop(&self) {
        let ended = self
            .store
            .close_ending_running(&FailureReason::DaemonShutDown, SystemClock.now())
            .await;
        match ended {
            Ok(0) => {}
            Ok(ended) => {
                log::info!("stopping: {ended} running operation(s) ended as daemon-shut-down")
            }
            Err(error) => log::warn!(
                "stopping: the operations still running couldn't be ended as daemon-shut-down, so the next start ends them as daemon-stopped: {error}"
            ),
        }
    }
}

/// Whether sign-in is on (§10.1, decision 70), decided once as the daemon
/// starts, before its store opens: on when the data folder keeps an admin
/// password's hash, off when it keeps none. Either way with where the hash
/// is kept: off, the session route looks there for one set since.
pub(crate) enum SignInDecided {
    Off(Arc<FileAdminPassword>),
    On(Arc<FileAdminPassword>),
}

impl SignInDecided {
    pub(crate) fn is_on(&self) -> bool {
        matches!(self, Self::On(_))
    }
}

/// Decides [`SignInDecided`]. A hash file that can't be read, or isn't a
/// hash this build checks, is a refusal to start, never sign-in off.
pub(crate) fn sign_in() -> Result<SignInDecided, Refused> {
    let data_dir = dirs::data_dir().ok_or_else(|| plain(msg::CLI_NO_DATA_DIR))?;
    let kept = Arc::new(FileAdminPassword::in_data_dir(&data_dir));
    match kept.is_set() {
        Ok(false) => Ok(SignInDecided::Off(kept)),
        Ok(true) => Ok(SignInDecided::On(kept)),
        Err(error) => {
            // `is_set` never answers `NotSet`: no file is sign-in off.
            let detail = match error {
                AdminPasswordError::Unreadable(said) | AdminPasswordError::Unwritable(said) => said,
                AdminPasswordError::NotSet => String::new(),
            };
            Err(Refused {
                message: msg::CLI_ADMIN_PASSWORD_UNREADABLE,
                args: vec![
                    (
                        "path",
                        text(data_dir.join(platform::admin_password::FILE).display()),
                    ),
                    ("detail", text(detail)),
                ],
            })
        }
    }
}

/// What the daemon's router runs against (§4.2), with `sign_in` deciding who
/// may use it, or why it cannot, and `exposure` who can reach it, which the
/// session route tells the page. With sign-in on, the sessions draw their
/// tokens from `random`, and the API tokens are kept in the daemon's store.
/// A restart is offered when `service_managed`: a service manager runs the
/// daemon, and starts it again once it exits (decision 94).
pub(crate) async fn daemon(
    sign_in: SignInDecided,
    random: Arc<dyn Random>,
    exposure: http_adapter::Exposure,
    service_managed: bool,
    recent_log: Arc<dyn app::ports::RecentLogStore>,
) -> Result<Daemon, Refused> {
    let mut daemon = daemon_at(dirs::config_dir(), dirs::data_dir(), dirs::cache_dir()).await?;
    // The one the daemon's log feeds, masked as the log is (I4, #618).
    daemon.state.recent_log = recent_log;
    daemon.state.restart = http_adapter::Restart::new(
        service_managed,
        Arc::new(runs::Work {
            queue: Arc::clone(&daemon.background.runner.queue),
            runs: daemon.background.runs(),
        }),
    );
    daemon.state.sign_in = match sign_in {
        SignInDecided::Off(kept) => SignIn::Off {
            password: kept,
            sessions: Arc::<SqliteStore>::clone(&daemon.log.store),
        },
        SignInDecided::On(password) => {
            let access = Arc::new(app::access::Access::new(
                password,
                Arc::clone(&random),
                Arc::new(SystemClock),
                Arc::<SqliteStore>::clone(&daemon.log.store),
                Arc::<SqliteStore>::clone(&daemon.log.store),
            ));
            restore_sessions(&access).await;
            SignIn::On(access)
        }
    };
    // A restore's and a proposal's step-up follow the sign-in just decided,
    // their one source: the admin password, in the sign-in waits, with
    // sign-in on (§10.4; decision 89, point 6).
    daemon.state.restores = Arc::new(restores::RestoreService::new(
        &daemon.background.runner,
        daemon.background.restore_folder.clone(),
        &daemon.state.sign_in,
    ));
    daemon.state.proposals = Arc::new(proposals::ProposalService::new(
        &daemon.background.proposals,
        &daemon.state.sign_in,
        daemon.background.runner.maintenance(),
    ));
    // So is a download's, its tickets drawn from the daemon's one source.
    daemon.state.downloads = Arc::new(downloads::DownloadService::new(
        &daemon.background.runner,
        &daemon.state.sign_in,
        random,
        Arc::new(SystemClock),
    ));
    daemon.state.exposure = exposure;
    daemon.background.releases = releases(std::env::var_os(RELEASES_URL_VARIABLE));
    Ok(daemon)
}

/// Brings back the sessions kept from before the start (§10.1): a failure
/// starts the daemon with none, and says so in the log.
async fn restore_sessions(access: &app::access::Access) {
    match access.restore().await {
        Ok(restored) if restored.unreadable > 0 => log::warn!(
            "{} kept session(s) could not be read and were forgotten",
            restored.unreadable
        ),
        Ok(_) => {}
        Err(error) => {
            log::warn!("the sessions kept from before the start were not brought back: {error:?}");
        }
    }
}

/// [`daemon`] with the folders supplied, for tests. The data folder is locked
/// before anything else is read or opened (§9), and what an earlier run left
/// unfinished is ended before the state is handed over, so nothing serves or
/// listens until both are done. The cache folder is no reason to refuse:
/// without one, the workers run without a cache.
async fn daemon_at(
    config_dir: Option<std::path::PathBuf>,
    data_dir: Option<std::path::PathBuf>,
    cache_dir: Option<std::path::PathBuf>,
) -> Result<Daemon, Refused> {
    let config_dir = config_dir.ok_or_else(|| plain(msg::CLI_NO_CONFIG_DIR))?;
    let data_dir = data_dir.ok_or_else(|| plain(msg::CLI_NO_DATA_DIR))?;
    // The data dir must be there before it is locked and the store opens in it.
    std::fs::create_dir_all(&data_dir)
        .map_err(|detail| store_refused(&data_dir, &detail.to_string()))?;
    // One daemon per data folder, before anything is read or opened: a second
    // process must not migrate the configuration the first is running on, read
    // what it runs, or end it as the leftover of a crash (§9).
    let lock = DataDirLock::acquire(&data_dir).map_err(|error| lock_refused(&data_dir, error))?;
    // The configuration next: a file this build cannot read is a refusal,
    // whatever the rest of the machine says.
    let config_file = config_dir.join("config.toml");
    let config: Arc<dyn ConfigStore> = Arc::new(FileConfigStore::new(
        config_file.clone(),
        Arc::new(SystemClock),
    ));
    let loaded = config.load().await.map_err(config_refused)?;
    log_inline_secrets(&loaded);
    log_never_matching_globs(
        &loaded,
        &WithinLimit(engine::RusticSources, SOURCES_LOOK_LIMIT),
    )
    .await;
    let recorded = recorded_repositories(&loaded);
    let database = data_dir.join("ladderback.db");
    let log = OperationLog {
        store: Arc::new(
            SqliteStore::open(&database)
                .await
                .map_err(|detail| store_refused(&database, &detail.to_string()))?,
        ),
        database,
    };
    // Every refusal from here on closes the log first: the refused daemon
    // leaves it as tidy as one that served and stopped.
    match state_over(
        config,
        config_file,
        &data_dir,
        cache_dir,
        recorded,
        &log,
        RunConditions::system(),
    )
    .await
    {
        Ok((state, background, workers)) => Ok(Daemon {
            state,
            log,
            lock,
            background,
            workers,
        }),
        Err(refused) => {
            log.close().await;
            Err(refused)
        }
    }
}

/// The router's state over the operation log `log`, the work the daemon
/// does with no request and its workers, once it has ended what an earlier
/// run left running, or why the daemon cannot serve. The workers' cache folder is
/// tidied first, keeping the caches of the repositories `recorded` names.
async fn state_over(
    config: Arc<dyn ConfigStore>,
    config_file: std::path::PathBuf,
    data_dir: &std::path::Path,
    cache_dir: Option<std::path::PathBuf>,
    recorded: Recorded,
    log: &OperationLog,
    conditions: RunConditions,
) -> Result<(AppState, Background, engine::Workers), Refused> {
    // When the daemon started, in monotonic time, as far as a run that waits
    // for the session's keyring service counts it (§5, decision 116).
    let started = SystemClock.monotonic();
    let store: Arc<dyn OperationStore> = Arc::<SqliteStore>::clone(&log.store);
    let schedules: Arc<dyn ScheduleStore> = Arc::<SqliteStore>::clone(&log.store);
    // The bus and the notification rules come first, so that recovery can
    // announce each operation it ends (decision 91).
    let bus: Arc<dyn EventBus> = Arc::new(BroadcastEventBus::new());
    notify_over(bus.as_ref(), Arc::clone(&config), Arc::clone(&store)).await;
    // Nothing has run yet, so an operation the log still shows as running was
    // left by a process that stopped. It ends as failed now, which also lets
    // go of the repository a running maintenance operation held (I7). A log
    // that cannot be changed is a refusal: serving over it would keep that
    // repository held for good. Queued operations stay queued (decision 73).
    let recovered = RecoverOperations::new(Arc::clone(&store))
        .with_events(Arc::clone(&bus))
        .recover(SystemClock.now())
        .await
        .map_err(|error| recovery_refused(&log.database, &error))?;
    let ended = recovered.ended.len();
    if ended > 0 {
        log::warn!(
            "ended {ended} operation(s) the operation log still showed as running: the daemon that ran them stopped before they finished"
        );
    }
    // By its code alone: the store's account of a row repeats what the row
    // holds, which nobody has checked for a value (I4).
    for error in &recovered.unreadable_ended {
        log::warn!(
            "a running operation in the operation log can't be read: it has ended as failed without being named ({})",
            error.code()
        );
    }
    for error in &recovered.unreadable_queued {
        log::warn!(
            "a queued operation in the operation log can't be read, and stays queued ({})",
            error.code()
        );
    }
    let agent_id = agent_id::load_or_create(data_dir)
        .map_err(|detail| store_refused(data_dir, &detail.to_string()))?;
    // Workers are this program: `ladderback worker` (§4.1).
    let program = std::env::current_exe().map_err(|detail| Refused {
        message: msg::CLI_CANNOT_FIND_OWN_PATH,
        args: vec![("detail", text(detail))],
    })?;
    // Before serving: a missing grant is said in the log once, at the start.
    let (full_disk_access, full_disk_access_route) = full_disk_access_over(&program).await;
    let mut workers = WorkerEngine::new(program, ["worker"]);
    if let Some(dir) = workers_cache(cache_dir) {
        tidy_workers_cache(&dir, recorded).await;
        workers = workers.with_cache_root(dir);
    }
    let stopping = workers.workers();
    // One lock for every write of the configuration the daemon makes: a
    // connection, a creation, the record of an opened repository's ID and a
    // plan's change (§6.1, decisions 57 and 96).
    let writes = Arc::new(ConfigWrites::default());
    let resolve = Arc::new(daemon_resolve(
        Arc::new(SystemSecrets::default()),
        Arc::new(SystemClock),
        started,
    ));
    // The choice of a location comes first (decision 80): a job opens the
    // location it chose, and the record of an opened repository's ID
    // (decision 81) sees that job, never a probe.
    let locations = Arc::new(ChooseLocation::new(
        Arc::new(RecordOpened::new(
            Arc::new(EventEngine {
                inner: Arc::new(workers),
                bus: Arc::clone(&bus),
            }),
            Arc::clone(&config),
            Arc::clone(&writes),
            Arc::new(log_recording),
        )),
        Arc::new(SystemClock),
        Arc::<NameWatch>::clone(&conditions.link),
        resolve.as_ref().clone(),
        Arc::new(log_location),
    ));
    let engine: Arc<dyn Engine> = Arc::<ChooseLocation>::clone(&locations);
    let (run_ends, ends) = app::retry::run_ends();
    let runner = Arc::new(PlanRunner {
        config: Arc::clone(&config),
        writes: Arc::clone(&writes),
        resolve: Arc::clone(&resolve),
        agent_id,
        left_out: KeyFilesLeftOut::of(data_dir),
        engine: Arc::clone(&engine),
        locations: Some(locations),
        store: Arc::clone(&store),
        retention: Arc::new(engine::RusticRetention),
        registry: Arc::new(CancelRegistry::new()),
        hooks: Arc::new(ProcessHookRunner::new()),
        events: Arc::clone(&bus),
        queue: Arc::new(RunQueue::new()),
        ends: run_ends,
        runs: runs::Runs::default(),
        // A repository's stats, read on request and after each check, are
        // kept in the operation log's database (§16, decision 104).
        readings: Arc::<SqliteStore>::clone(&log.store),
        full_disk_access,
    });
    // `~/ladderback-restore`. With no home folder, or one that isn't
    // UTF-8, there is none: a restore to a new folder then needs
    // `[agent] restore-folder`, and never lands beside the database and
    // the secrets in the data folder (§10.4).
    let restore_folder = std::env::home_dir().and_then(|home| {
        home.join("ladderback-restore")
            .to_str()
            .and_then(|folder| RestoreFolder::parse(folder).ok())
    });
    let sign_in = SignIn::Off {
        password: Arc::new(FileAdminPassword::in_data_dir(data_dir)),
        sessions: Arc::<SqliteStore>::clone(&log.store),
    };
    let restores: Arc<dyn http_adapter::Restores> = Arc::new(restores::RestoreService::new(
        &runner,
        restore_folder.clone(),
        &sign_in,
    ));
    let downloads: Arc<dyn http_adapter::Downloads> = Arc::new(downloads::DownloadService::new(
        &runner,
        &sign_in,
        Arc::new(OsRandom),
        Arc::new(SystemClock),
    ));
    let operations: Arc<dyn Operations> = Arc::<PlanRunner>::clone(&runner);
    let repositories: Arc<dyn Repositories> = Arc::new(RepositoryService::new(
        Arc::new(SystemClock),
        Arc::clone(&engine),
        Arc::<NameWatch>::clone(&conditions.link),
        resolve.as_ref().clone(),
        Arc::clone(&config),
        Arc::clone(&store),
        Arc::clone(&writes),
    ));
    let (changed, changes) = app::schedule::config_changes();
    // The turns to list a repository's snapshots: the snapshot list's
    // listings and the forget previews' reads take them alike (#710, #967).
    let listings: Arc<ReadTurns> = Arc::default();
    let proposal_parts = proposals::ProposalParts {
        file: Arc::new(FileConfigStore::new(
            config_file.clone(),
            Arc::new(SystemClock),
        )),
        store: Arc::<SqliteStore>::clone(&log.store),
        config: Arc::clone(&config),
        writes: Arc::clone(&writes),
        changed: changed.another(),
        events: Arc::clone(&bus),
        revision_key: proposals::revision_key(),
        listings: Arc::clone(&listings),
    };
    // As with the operations: nothing has been approved yet, so a proposal
    // the store still shows being applied was left by a process that
    // stopped mid-apply. It ends as failed (decision 89). A store that
    // can't be read or changed leaves it so, and the daemon starts all the
    // same: no repository waits on a proposal.
    match proposal_parts.proposals().recover().await {
        Ok(ended) if !ended.is_empty() => log::warn!(
            "ended {} proposal(s) the store still showed being applied: the daemon that applied them stopped before it recorded the end",
            ended.len()
        ),
        Ok(_) => {}
        // By its code alone (I4).
        Err(error) => log::warn!(
            "the proposals left being applied could not be ended ({})",
            error.code()
        ),
    }
    // The plan dry run reads outside the backup concurrency limit, one at a
    // time, and gives way to every backup the queue holds (decision 89,
    // point 13). It leaves out the key files each backup does, and is
    // refused where the backup is (decision 120), so it predicts the backup.
    let queue = Arc::clone(&runner.queue);
    let dry_runs: Arc<dyn http_adapter::DryRuns> = Arc::new(dry_runs::DryRunService::new(
        app::dry_run::PlanDryRuns::new(
            Arc::clone(&config),
            Arc::new(WithinLimit(engine::RusticSources, SOURCES_LOOK_LIMIT)),
            Arc::new(engine::RusticWalk),
            Arc::new(SystemClock),
            Arc::new(move || queue.is_busy()),
            app::dry_run::DryRunLimits::DAEMON,
            runner.left_out.for_dry_runs(),
        ),
        app::dry_run::DryRunAllowlists::new(
            Arc::clone(&config),
            Arc::clone(&writes),
            changed.another(),
        ),
        std::env::home_dir().and_then(|home| home.to_str().map(str::to_owned)),
    ));
    let plans: Arc<dyn Plans> = Arc::new(plans::PlanService::new(
        Arc::clone(&config),
        app::plans::EditPlans::new(Arc::clone(&config), writes, changed)
            .with_proposals(Arc::new(proposal_parts.proposals())),
    ));
    let proposals: Arc<dyn http_adapter::Proposals> = Arc::new(proposals::ProposalService::new(
        &proposal_parts,
        &sign_in,
        runner.maintenance(),
    ));
    let profiles: Arc<dyn http_adapter::Profiles> = Arc::new(profiles::ProfileService::new(
        Arc::clone(&config),
        config_file,
        agent_id,
        Arc::new(WithinLimit(engine::RusticSources, SOURCES_LOOK_LIMIT)),
    ));
    let catalog: Arc<dyn Catalog> = Arc::new(ConfigCatalog {
        config: Arc::clone(&config),
        engine,
        operations: Arc::clone(&store),
        schedules: Arc::clone(&schedules),
        link: Arc::clone(&conditions.link),
        sources: Arc::new(WithinLimit(engine::RusticSources, SOURCES_LOOK_LIMIT)),
        reads: Arc::default(),
        listings,
    });
    let notice = Arc::new(UpdateNotice::new());
    let background = Background {
        runner,
        restore_folder,
        proposals: proposal_parts,
        queued: recovered.queued,
        config,
        schedules,
        store,
        conditions,
        ends,
        changes,
        notice: Arc::clone(&notice),
        releases: None,
    };
    Ok((
        AppState {
            events: bus,
            operations,
            catalog,
            repositories,
            plans,
            profiles,
            restores,
            proposals,
            dry_runs,
            downloads,
            sign_in,
            exposure: http_adapter::Exposure::Loopback,
            build: crate::build_info::running(),
            restart: http_adapter::Restart::default(),
            updates: notice,
            // An empty one: the daemon's own puts the one its log feeds here.
            recent_log: Arc::new(app::recent_log::RecentLogRing::new()),
            full_disk_access: full_disk_access_route,
            sync: http_adapter::SyncHub::default(),
        },
        background,
        stopping,
    ))
}

/// The agent's Full Disk Access check and its routes (§16, decision 115): on
/// macOS, the probe of `~/Library/Safari` under the home the daemon runs
/// with, which a test's daemon has in its scratch folder, and System
/// Settings' pane, showing `program`; nothing elsewhere. The probe runs once
/// here, and a grant found missing is said in the log.
async fn full_disk_access_over(
    program: &std::path::Path,
) -> (
    Option<Arc<FullDiskAccessCheck>>,
    http_adapter::FullDiskAccessRoute,
) {
    let Some(probe) = platform::full_disk_access::system_probe() else {
        return (None, http_adapter::FullDiskAccessRoute::default());
    };
    let check = Arc::new(FullDiskAccessCheck::new(Arc::new(probe)));
    if let Some(StartNote::FullDiskAccessMissing) = check.at_start().await {
        log::warn!(
            "this program hasn't been given Full Disk Access, so backups can't read Mail, Messages, Safari and other apps' data on this Mac: turn it on in System Settings, under Privacy & Security, then Full Disk Access; the start page shows how"
        );
    }
    let pane = SystemSettings(platform::full_disk_access::SettingsPane::new(
        program.to_path_buf(),
    ));
    (
        Some(Arc::clone(&check)),
        http_adapter::FullDiskAccessRoute::new(check, Arc::new(pane)),
    )
}

/// System Settings' Full Disk Access pane, as the routes open it.
struct SystemSettings(platform::full_disk_access::SettingsPane);

#[async_trait]
impl http_adapter::SettingsOpener for SystemSettings {
    async fn open(&self) -> Result<(), http_adapter::SettingsUnopened> {
        self.0
            .open()
            .await
            .map_err(|detail| http_adapter::SettingsUnopened { detail })
    }
}

/// Tells the channels of the agent's notification rules of the news on `bus`
/// for as long as the daemon runs (§11). A notification that wasn't delivered
/// is said in the log by its warning code, never with a secret (I4), and an
/// event the configuration couldn't be read for by the configuration's error
/// code. A client that can't be built is said once, and nothing is sent.
async fn notify_over(
    bus: &dyn EventBus,
    config: Arc<dyn ConfigStore>,
    store: Arc<dyn OperationStore>,
) {
    let localizer = || l10n::Localizer::new(&l10n::PosixEnv::from_process().preferences());
    let notifier = match HttpNotifier::new(localizer(), DELIVERY_TIMEOUT) {
        Ok(notifier) => notifier,
        Err(error) => {
            log::warn!("no notification is sent: {error}");
            return;
        }
    };
    let notify = Notify::new(
        config,
        store,
        Arc::new(SystemSecrets::default()),
        Arc::new(notifier),
    )
    .telling(Arc::new(log_secret_warning));
    // Subscribed before any run can start, so no news is missed.
    let news = bus.subscribe().await;
    let localizer = localizer();
    tokio::spawn(async move {
        notify
            .run(news, |report| {
                log::warn!("{}", report_line(&localizer, &report))
            })
            .await;
    });
}

/// What telling an event left to report, as the log says it, by its code: a
/// notification that wasn't delivered, with the event, the rule, the channel
/// and why; or a configuration that couldn't be read, as its code's message
/// says it.
fn report_line(localizer: &l10n::Localizer, report: &Report) -> String {
    match report {
        Report::Undelivered(undelivered) => {
            let args = [
                ("event", text(undelivered.event.name())),
                ("rule", text(undelivered.rule.as_str())),
                ("channel", text(undelivered.channel.as_str())),
                ("reason", text(undelivered.reason.reason())),
            ];
            let message = localizer.warning(WarningCode::NotificationUndelivered, &args, None);
            format!("{:?}: {message}", WarningCode::NotificationUndelivered)
        }
        Report::ConfigUnavailable(error) => config_unusable(error.clone(), |code, args, detail| {
            let message = localizer.error(code, &args, detail.as_deref());
            format!("no notification was sent for an event: {code:?}: {message}")
        }),
    }
}

/// The folder the workers keep rustic's caches in (§4.1): `dir`, made for
/// the daemon's user alone when it isn't there yet. `None`, said once in the
/// log, when there is no such folder, it can't be made or files can't be
/// made in it: the workers then run without a cache, which costs each job
/// time and nothing else, instead of each job warning about it again.
fn workers_cache(dir: Option<std::path::PathBuf>) -> Option<std::path::PathBuf> {
    let Some(dir) = dir else {
        log::warn!(
            "workers run without a cache: there is no home directory to keep one under; set LADDERBACK_CACHE_DIR to give them one"
        );
        return None;
    };
    if let Err(error) = dirs::create_private_dir(&dir) {
        log::warn!(
            "workers run without a cache: the cache folder {} can't be made: {error}",
            dir.display()
        );
        return None;
    }
    if let Err(error) = dirs::probe_writable(&dir) {
        log::warn!(
            "workers run without a cache: the cache folder {} can't be written to: {error}",
            dir.display()
        );
        return None;
    }
    Some(dir)
}

/// Says in the log what became of the ID of a repository a job opened,
/// whose entry recorded none. It names the entry by its alias and the
/// repository by its ID, neither of which opens it (I4).
fn log_recording(recording: Recording) {
    match recording {
        Recording::Recorded { alias, id } => log::info!(
            "recorded the repository ID {} in the entry {} of the configuration: a job opened it",
            id.as_str(),
            alias.as_str()
        ),
        Recording::NotRecorded {
            alias,
            id,
            why: NotRecorded::SameRepository { other },
        } => log::warn!(
            "did not record the repository ID {} in the entry {} of the configuration: the entry {} records it, so the two entries are one repository; remove one of them",
            id.as_str(),
            alias.as_str(),
            other.as_str()
        ),
        Recording::NotRecorded { alias, id, why } => log::info!(
            "did not record the repository ID {} in the entry {} of the configuration ({why:?}); the next job that opens it tries again",
            id.as_str(),
            alias.as_str()
        ),
        Recording::NotInPlace { alias, id, why } => log::warn!(
            "did not record the repository ID {} in the entry {} of the configuration: it can't be added without changing something else in the file ({why:?}); the next job that opens it tries again",
            id.as_str(),
            alias.as_str()
        ),
        Recording::Busy { alias, id } => log::info!(
            "did not record the repository ID {} in the entry {} of the configuration: another change to it was being saved; the next job that opens it tries again",
            id.as_str(),
            alias.as_str()
        ),
        Recording::Failed { alias, id, error } => log::warn!(
            "did not record the repository ID {} in the entry {} of the configuration ({error:?}); the next job that opens it tries again",
            id.as_str(),
            alias.as_str()
        ),
    }
}

/// The repositories the entries of `config` record. While an entry records
/// no ID, its repository's cache may be any in the cache folder (§16
/// decision 61).
fn recorded_repositories(config: &domain::planning::AgentConfig) -> Recorded {
    config
        .repositories()
        .values()
        .map(|repository| repository.id.clone())
        .collect::<Option<BTreeSet<RepositoryId>>>()
        .map_or(Recorded::NotAllKnown, Recorded::Exactly)
}

/// Tidies the workers' cache folder `dir` before any worker runs (§4.1,
/// §16 decision 61), keeping the caches of the repositories `recorded`
/// names, and says in the log what went and its size. Nothing here stops
/// the daemon from starting.
async fn tidy_workers_cache(dir: &std::path::Path, recorded: Recorded) {
    let root = dir.to_owned();
    let tidied = tokio::task::spawn_blocking(move || {
        engine::tidy_cache(&root, &recorded, std::time::SystemTime::now())
    })
    .await;
    match tidied {
        Ok(tidying) => log_tidying(&tidying),
        Err(error) => log::warn!("the cache folder {} was not tidied: {error}", dir.display()),
    }
}

/// Says in the log what tidying the cache removed, and what it couldn't.
fn log_tidying(tidying: &engine::CacheTidying) {
    for removed in &tidying.removed {
        let (id, count, bytes) = (removed.repository.as_str(), removed.count, removed.bytes);
        match removed.what {
            RemovedKind::Unrecorded => log::info!(
                "removed the cache of repository {id}, which no repository entry records: {count} cache slot(s), {bytes} bytes"
            ),
            RemovedKind::SpareSlots => {
                log::info!("removed {count} spare cache slot(s) of repository {id}: {bytes} bytes")
            }
            RemovedKind::UnfinishedFiles => log::info!(
                "removed {count} unfinished cache file(s) of repository {id}: {bytes} bytes"
            ),
        }
    }
    for failed in &tidying.failed {
        log::warn!(
            "the cache {} could not be tidied: {}",
            failed.path.display(),
            failed.error
        );
    }
}

/// Waits for `close`, the closing of the operation log at `database`, for
/// at most `bound`, and says in the log when it gave up.
async fn close_within(
    close: impl Future<Output = ()>,
    database: &std::path::Path,
    bound: std::time::Duration,
) {
    if tokio::time::timeout(bound, close).await.is_err() {
        log::warn!(
            "the operation log {} did not close within {} s, and the daemon stops without it: an operation it was still recording is ended at the next start",
            database.display(),
            bound.as_secs_f64()
        );
    }
}

/// The refusal of a data folder another process holds, named and unnamed. On
/// Linux and macOS that process may be the installed service, which its
/// manager starts again once killed (#1035): the message names the manager's
/// stop and `remove-service`.
#[cfg(target_os = "linux")]
const DATA_DIR_IN_USE: [MessageId; 2] = [
    msg::CLI_DATA_DIR_IN_USE_SYSTEMD,
    msg::CLI_DATA_DIR_IN_USE_UNNAMED_SYSTEMD,
];
#[cfg(target_os = "macos")]
const DATA_DIR_IN_USE: [MessageId; 2] = [
    msg::CLI_DATA_DIR_IN_USE_LAUNCHD,
    msg::CLI_DATA_DIR_IN_USE_UNNAMED_LAUNCHD,
];
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
const DATA_DIR_IN_USE: [MessageId; 2] =
    [msg::CLI_DATA_DIR_IN_USE, msg::CLI_DATA_DIR_IN_USE_UNNAMED];

/// A data folder another process holds, or a lock that couldn't be taken, as
/// the message for whoever started the daemon.
fn lock_refused(data_dir: &std::path::Path, error: LockError) -> Refused {
    let [named, unnamed] = DATA_DIR_IN_USE;
    match error {
        LockError::InUse { holder: Some(pid) } => Refused {
            message: named,
            args: vec![("path", text(data_dir.display())), ("pid", text(pid))],
        },
        LockError::InUse { holder: None } => Refused {
            message: unnamed,
            args: vec![("path", text(data_dir.display()))],
        },
        LockError::Unavailable { detail } => Refused {
            message: msg::CLI_CANNOT_LOCK_DATA_DIR,
            args: vec![
                ("path", text(data_dir.join(LOCK_FILE).display())),
                ("detail", text(detail)),
            ],
        },
    }
}

/// An operation log that couldn't end what an earlier run left unfinished, as
/// the message for whoever started the daemon.
fn recovery_refused(database: &std::path::Path, error: &app::ports::StoreError) -> Refused {
    let (app::ports::StoreError::Unavailable { detail }
    | app::ports::StoreError::Corrupt { detail }) = error;
    Refused {
        message: msg::CLI_CANNOT_RECOVER_OPERATIONS,
        args: vec![("path", text(database.display())), ("detail", text(detail))],
    }
}

/// A configuration the daemon cannot use, as the message for whoever wrote it.
pub(crate) fn config_refused(error: ConfigError) -> Refused {
    fn refused(message: MessageId, args: Vec<(&'static str, MsgArg)>) -> Refused {
        Refused { message, args }
    }
    match error {
        ConfigError::Unavailable { file, detail } => refused(
            msg::CLI_CONFIG_UNAVAILABLE,
            vec![("file", text(file)), ("detail", text(detail))],
        ),
        ConfigError::TooLarge { file, limit } => refused(
            msg::CLI_CONFIG_TOO_LARGE,
            vec![("file", text(file)), ("limit", MsgArg::Bytes(limit))],
        ),
        ConfigError::SchemaMissing { file } => {
            refused(msg::CLI_CONFIG_SCHEMA_MISSING, vec![("file", text(file))])
        }
        ConfigError::SchemaNewer {
            file,
            found,
            supported,
        } => refused(
            msg::CLI_CONFIG_SCHEMA_NEWER,
            vec![
                ("file", text(file)),
                ("found", text(found)),
                ("supported", text(supported)),
            ],
        ),
        ConfigError::Invalid {
            file,
            key,
            at,
            problem,
            detail,
        } => {
            // `repository-id` is required only of a repository with further
            // locations (decision 80), and the way out is to record it.
            let needs_id = problem == app::ports::Problem::MissingKey
                && key
                    .as_deref()
                    .is_some_and(|key| key.ends_with(".repository-id"));
            let mut where_ = key.unwrap_or_else(|| "an earlier rule".to_owned());
            if let Some(at) = at {
                where_ = format!("{where_} (line {}, column {})", at.line, at.column);
            }
            if needs_id {
                return refused(
                    msg::CLI_CONFIG_LOCATIONS_NEED_ID,
                    vec![("file", text(file)), ("where", text(where_))],
                );
            }
            refused(
                msg::CLI_CONFIG_INVALID,
                vec![
                    ("file", text(file)),
                    ("where", text(where_)),
                    ("detail", text(format!("{problem:?}: {detail}"))),
                ],
            )
        }
    }
}

/// A folder or database the daemon cannot open.
fn store_refused(path: &std::path::Path, detail: &str) -> Refused {
    Refused {
        message: msg::CLI_CANNOT_OPEN_STORE,
        args: vec![("path", text(path.display())), ("detail", text(detail))],
    }
}

/// The engine as the daemon runs it (§4.2): a job's progress and warnings go
/// to the event bus as [`UiEvent`]s on their way through, in the order the
/// engine reported them. The daemon never coalesces: that is the worker's,
/// once (§4.1). How an operation started and ended is not announced here:
/// the run that records it announces each once the log has it (§4.4,
/// #305). A job that is no operation tells the pages nothing (#308).
struct EventEngine {
    inner: Arc<dyn Engine>,
    bus: Arc<dyn EventBus>,
}

#[async_trait]
impl Engine for EventEngine {
    async fn run(&self, op: OperationId, job: Job) -> Result<RunningJob, EngineError> {
        if !is_operation(&job) {
            return self.inner.run(op, job).await;
        }
        let operation = op.as_uuid().hyphenated().to_string();
        let running = self.inner.run(op, job).await?;
        let mut publish = Publish {
            operation,
            progress: LiveProgress::new(),
        };
        let bus = Arc::clone(&self.bus);
        Ok(RunningJob {
            // `then` keeps the engine's order: an event is published before
            // the runner sees it.
            events: running
                .events
                .then(move |event| {
                    let bus = Arc::clone(&bus);
                    let ui = publish.ui(&event);
                    async move {
                        if let Some(ui) = ui {
                            bus.publish(ui).await;
                        }
                        event
                    }
                })
                .boxed(),
            control: running.control,
        })
    }
}

/// What the choice of a location tells the daemon's log (decision 80): for
/// each job on a repository reached at several, which location it used and
/// why each earlier one was skipped, by position and outcome code, never an
/// address or a network's name.
fn log_location(report: LocationReport) {
    match report {
        LocationReport::ExposedCredentials {
            alias,
            position: Some(position),
            cause,
        } => log::warn!(
            "repository {}: location {position} sends its credentials where whoever answers at its address can read them ({})",
            alias.as_str(),
            cause.code()
        ),
        LocationReport::ExposedCredentials {
            alias,
            position: None,
            cause,
        } => log::warn!(
            "repository {}: this repository sends its credentials where whoever answers at its address can read them ({})",
            alias.as_str(),
            cause.code()
        ),
        LocationReport::OtherRepository { alias, position } => log::warn!(
            "repository {}: location {position} holds another repository, so it was not used",
            alias.as_str()
        ),
        LocationReport::Chosen { alias, record } => {
            let count = record.locations.len();
            let skipped: Vec<String> = record
                .locations
                .iter()
                .filter(|tried| {
                    !matches!(
                        tried.outcome,
                        LocationOutcome::Used | LocationOutcome::NotTried
                    )
                })
                .map(|tried| format!("location {} {}", tried.position, tried.outcome.code()))
                .collect();
            let skipped = if skipped.is_empty() {
                String::new()
            } else {
                format!(" (skipped: {})", skipped.join(", "))
            };
            match record.used() {
                Some(used) => {
                    log::info!(
                        "repository {}: used location {used} of {count}{skipped}",
                        alias.as_str()
                    );
                }
                None => {
                    log::warn!(
                        "repository {}: no location of {count} could be used{skipped}",
                        alias.as_str()
                    )
                }
            }
        }
    }
}

/// Whether the job is an operation, which the history has a row for, so a
/// page can show its progress and warnings there (§4.4). The reads (the look
/// that names a repository, a listing, a stats read, a prune's or a
/// restore's plan) and the initialising of a repository answer their request
/// and are written to no log (§9): a page told of their news would show it
/// on no row, or on the row of the run that read, which keeps none of it
/// (#308). A download has a row, but its use case writes it, not a run, and
/// its news is the pieces its browser reads, never told to a page.
fn is_operation(job: &Job) -> bool {
    match job {
        Job::Identify(_)
        | Job::Probe(_)
        | Job::Init(_)
        | Job::Ls(_)
        | Job::Browse(_)
        | Job::Stats(_)
        | Job::PrunePlan(_)
        | Job::RestorePlan(_)
        | Job::Dump(_) => false,
        Job::Fake(_)
        | Job::Backup(_)
        | Job::Forget(_)
        | Job::Rewrite(_)
        | Job::Prune(_)
        | Job::Check(_)
        | Job::Restore(_) => true,
    }
}

/// Turns one run's [`EngineEvent`]s into [`UiEvent`]s, but for how the run
/// started and ended (§4.4). Of its progress bars, a page is told of the one
/// [`LiveProgress`] shows: the bar begun last that is still open.
struct Publish {
    operation: String,
    progress: LiveProgress,
}

impl Publish {
    fn ui(&mut self, event: &EngineEvent) -> Option<UiEvent> {
        use EngineEvent as E;
        match event {
            E::Progress(progress) => self
                .progress
                .apply(progress)
                .map(|shown| shown.ui(self.operation.clone())),
            E::Warning(warning) => Some(UiEvent::OperationWarning {
                operation: self.operation.clone(),
                code: warning_code(warning.kind),
                args: warning_args(warning),
            }),
            // The engine's start comes before the run's write that says the
            // operation is running, and its end before the run's ending hooks
            // and final write, so a page told of either could read a history
            // that doesn't have it yet. The run announces both.
            // The choice of location is the run's record, which the
            // operation's row shows once the run writes it.
            // A dump's header and pieces go to its download alone.
            E::Started { .. }
            | E::Succeeded(_)
            | E::Failed(_)
            | E::Located(_)
            | E::Dumping(_)
            | E::Piece(_) => None,
        }
    }
}

/// Starts the operations the API asks for: a backup of a named plan, run to
/// its end on a task of its own. What happens to the run arrives over the
/// events WebSocket, so the route answers with the operation's ID alone.
struct PlanRunner {
    config: Arc<dyn ConfigStore>,
    /// The one lock of every write of the configuration, which a
    /// maintenance start holds while it reads again that this agent
    /// maintains the repository (I1).
    writes: Arc<ConfigWrites>,
    resolve: Arc<ResolveRepository>,
    agent_id: AgentId,
    engine: Arc<dyn Engine>,
    /// Chooses the location retention opens in-process for a repository
    /// reached at several (decision 80); `engine` makes the same choice for
    /// every job. Without it, such a repository's retention fails.
    locations: Option<Arc<ChooseLocation>>,
    store: Arc<dyn OperationStore>,
    /// Evaluates each plan's retention for a forget run (§7.3).
    retention: Arc<dyn Retention>,
    /// The running operations' cancel switches, shared with the routes.
    registry: Arc<CancelRegistry>,
    /// Runs the agent, repository and plan hooks around every run (§6.1).
    hooks: Arc<dyn HookRunner>,
    /// Where each run announces how its operation ended, once the log has the
    /// end (§4.4).
    events: Arc<dyn EventBus>,
    /// The agent's one queue of backups, which the scheduler shares: every
    /// backup takes its turn in it (§5, the concurrency limit).
    queue: Arc<RunQueue>,
    /// Where each backup's run says how it ended, so the scheduler can
    /// retry a failed one (§5).
    ends: app::retry::RunEndSender,
    /// What the daemon's orderly stop freezes: every run starts under it
    /// (decision 92).
    runs: runs::Runs,
    /// Where each reading of a repository's stats is kept (§16, decision
    /// 104): read on request, and after each check.
    readings: Arc<dyn app::ports::StatsStore>,
    /// The agent's Full Disk Access check, run before each backup on macOS
    /// (§16, decision 115); none elsewhere.
    full_disk_access: Option<Arc<FullDiskAccessCheck>>,
    /// What keeps the secret key files in the data folder out of every
    /// backup (§16, decisions 120 and 122).
    left_out: KeyFilesLeftOut,
}

impl PlanRunner {
    /// The backup job of plan `id`, the hooks around it and what reading its
    /// secrets told, as `config` stands now: a rewrite between runs is
    /// picked up. A run `trigger` started with nobody waiting on it holds
    /// for the session's keyring service while the daemon starts, and the
    /// log says so once (§5).
    async fn backup_of(
        &self,
        config: &domain::planning::AgentConfig,
        id: &PlanId,
        trigger: &Trigger,
    ) -> Result<BuiltBackup, Unbuilt> {
        let plan = config
            .plan(id)
            .ok_or(Unbuilt::Refused(StartError::UnknownPlan))?;
        let repository = config
            .repository(&plan.repository)
            .ok_or(Unbuilt::Refused(StartError::UnknownPlan))?;
        // The secret key files stay out of every backup that walks the data
        // folder, in every build, or the backup is refused when no glob can
        // name them (§16, decisions 120 and 122).
        let left_out = self
            .left_out
            .for_backup(&plan.sources)
            .map_err(Unbuilt::Refused)?;
        // The engine opens only the repository the configuration recorded,
        // when it recorded one (§7.3), and the job carries the entry it was
        // made from, so the ID it opens is recorded in an entry that records
        // none (§6.1, decision 57). A secret that can't be resolved is
        // refused with its own code, masked of every secret of the
        // repository, as the repository routes refuse it (I4, #309).
        let resolving = if trigger.is_unattended() {
            let waiting = |source: &domain::secrets::SourceName| {
                log_keyring_wait(trigger, id, source);
            };
            self.resolve
                .resolve_saved_unattended(&plan.repository, repository, &waiting)
                .await
        } else {
            self.resolve
                .resolve_saved(&plan.repository, repository)
                .await
        };
        let resolved = match resolving {
            Ok(resolved) => resolved,
            Err(error) => {
                let redactor =
                    refusal_redactor(&error, &repository.settings, &SystemSecrets::default()).await;
                return Err(Unbuilt::Secret(Box::new(SecretUnbuilt {
                    refusal: refusal_with(&error, &redactor),
                    failure: error.failure(&redactor),
                    target: Target {
                        plan: Some(plan.id.clone()),
                        repository: repository.id.clone(),
                    },
                })));
            }
        };
        // The run's hooks are masked of every secret they can print, built
        // once, now that the run's secrets are read (I4, security.md §10.2).
        let redactor = Arc::new(
            resolved
                .access
                .hooks_redactor(config, &self.hooks.environment_secrets()),
        );
        let job = Job::Backup(BackupJob {
            repository: resolved.access,
            sources: plan.sources.iter().map(|s| s.as_str().to_owned()).collect(),
            tags: PlanTags::new(plan.id.clone(), self.agent_id),
            extra_tags: plan.tags.clone(),
            label: plan.label.clone(),
            host: plan.snapshot_host(config.agent()).cloned(),
            filter: plan.filter.clone(),
            left_out: left_out.to_vec(),
            deadline_budget: backup_budget(None, None),
        });
        // The agent, repository and plan hooks of §6.1 wrap the run.
        let setup = HookSetup {
            agent: config.hooks().clone(),
            repository: repository.hooks.clone(),
            plan: plan.hooks.clone(),
            plan_id: plan.id.clone(),
            repository_alias: plan.repository.clone(),
            redactor,
        };
        Ok(BuiltBackup {
            job,
            setup,
            warnings: resolved
                .warnings
                .iter()
                .map(SecretWarning::warning)
                .collect(),
        })
    }

    /// The use case that runs a backup with `setup`'s hooks.
    fn backup_use_case(&self, setup: HookSetup) -> RunOperation {
        let use_case = RunOperation::new(
            Arc::clone(&self.engine),
            Arc::new(SystemClock),
            Arc::clone(&self.store),
        )
        .with_hooks(Arc::clone(&self.hooks), setup)
        .with_events(Arc::clone(&self.events));
        match &self.full_disk_access {
            Some(check) => use_case.with_full_disk_access(Arc::clone(check)),
            None => use_case,
        }
    }

    /// Parks `operation`'s cancel switch before its run starts — not lazily
    /// when the run first polls it — so a cancel that arrives the instant
    /// the route answers finds it (§4.2). A cancel that fires only resolves;
    /// whether the receiver was still listening is nobody's business here.
    fn cancel_switch(&self, operation: OperationId) -> impl Future<Output = ()> + Send + use<> {
        let cancel = self.registry.register(operation);
        async move {
            let _ = cancel.await;
        }
    }

    /// Starts a backup of plan `id`, started by `trigger`, on a task of its
    /// own, and returns its operation's ID once the log has it queued. It
    /// runs in `place` when the scheduler gave it one, else in a place it
    /// takes in the queue now, as any run started by hand does (§5).
    async fn start_plan(
        &self,
        id: &PlanId,
        trigger: Trigger,
        place: Option<Place>,
    ) -> Result<OperationId, Unbuilt> {
        let config = self.config.load().await.map_err(|error| {
            Unbuilt::Refused(config_unusable(error, |code, args, detail| {
                StartError::Unavailable { code, args, detail }
            }))
        })?;
        let BuiltBackup {
            job,
            setup,
            warnings,
        } = self.backup_of(&config, id, &trigger).await?;
        let place = place.unwrap_or_else(|| self.queue.push(id, config.agent().concurrency));
        // A UUIDv7: the log orders by ID, so the newest run is the highest.
        let operation = OperationId::from_uuid(Uuid::now_v7());
        let cancel = self.cancel_switch(operation);
        let use_case = self.backup_use_case(setup);
        // Recorded as queued before the route answers, so a cancel that
        // arrives the instant it does finds the operation in the log. The
        // task does the recording and the run, so a handler that is dropped
        // meanwhile (the client went away) strands neither.
        let (recorded, wait_for_log) = futures::channel::oneshot::channel::<()>();
        let registry = Arc::clone(&self.registry);
        let ends = self.ends.clone();
        let runs = self.runs.clone();
        tokio::spawn(async move {
            let queued = use_case
                .queue(operation, trigger, job)
                .await
                .with_warnings(warnings);
            let _ = recorded.send(());
            // Nothing waits on the run: it reports through the event bus. It
            // waits in the log as queued until its turn comes. A run the
            // daemon's stop freezes stays queued, for the next start to run
            // (decision 73): the stop came after the log had it.
            runs.until_frozen(async move {
                let finished = use_case.run_in_turn(queued, place, cancel).await;
                registry.finish(&operation);
                log_ended(&finished);
                say_how_it_ended(&ends, &finished);
            })
            .await;
        });
        // A task that ended without recording has nothing to wait for.
        let _ = wait_for_log.await;
        Ok(operation)
    }
}

/// A plan's backup as [`PlanRunner::backup_of`] builds it.
struct BuiltBackup {
    job: Job,
    setup: HookSetup,
    /// What reading the repository's secrets told (#386), which the run
    /// carries from its start.
    warnings: Vec<domain::execution::Warning>,
}

/// Why a plan's backup couldn't be built, so its run doesn't start.
enum Unbuilt {
    /// Refused as a request to start it is, and nothing is recorded.
    Refused(StartError),
    /// A setting of the plan's repository couldn't be resolved.
    Secret(Box<SecretUnbuilt>),
}

/// A plan's backup unbuilt for a setting of its repository: `refusal` is
/// what a request to start it is answered with, and `failure` and `target`
/// what a run the schedule started records (§5).
struct SecretUnbuilt {
    refusal: SecretRefusal,
    failure: FailureReason,
    target: Target,
}

impl Unbuilt {
    /// The refusal of a request to start the run.
    fn into_start_error(self) -> StartError {
        match self {
            Self::Refused(error) => error,
            Self::Secret(secret) => StartError::SecretRefused(secret.refusal),
        }
    }
}

/// Logs that plan `plan`'s run, started by `trigger`, didn't start.
fn log_not_started(plan: &PlanId, trigger: &Trigger, error: &StartError) {
    log::warn!(
        "{} of plan {} didn't start: {}",
        run_named(trigger),
        plan.as_str(),
        start_refused(error)
    );
}

/// A run started by `trigger`, in the log's words: "the scheduled run",
/// never the trigger's name in the code. A run an API token asked for is
/// named by the token's label and ID, never its secret (I4).
fn run_named(trigger: &Trigger) -> std::borrow::Cow<'static, str> {
    match trigger {
        Trigger::Schedule => "the scheduled run".into(),
        Trigger::Manual => "the manual run".into(),
        Trigger::CatchUp => "the catch-up run".into(),
        Trigger::Retry => "the retry".into(),
        Trigger::Hub => "the hub's run".into(),
        Trigger::Mcp => "the MCP client's run".into(),
        Trigger::ApiToken { id, label } => format!(
            "the run API token \"{}\" ({}) asked for",
            label.as_str(),
            id.as_str()
        )
        .into(),
    }
}

/// `code` as the API and the page name it, such as `secret-missing`, never
/// its name in the code.
fn wire_name(code: api::ErrorCode) -> String {
    match serde_json::to_value(code) {
        Ok(serde_json::Value::String(name)) => name,
        _ => format!("{code:?}"),
    }
}

/// Why a plan's run didn't start, for the daemon's log: a code, and for a
/// secret the setting and where it was looked for (an entry, a file, a
/// variable or a program, masked as the refusal shows them), never the
/// words of a secret's refusal, which may quote what was read (I4).
fn start_refused(error: &StartError) -> String {
    match error {
        StartError::UnknownPlan => "the plan or its repository isn't configured".to_owned(),
        StartError::Unavailable { code, .. } => {
            format!(
                "the configuration can't be read or used ({})",
                wire_name(*code)
            )
        }
        StartError::SecretRefused(refusal) => {
            let named = |key: &str| {
                refusal.args.iter().find_map(|(name, arg)| match arg {
                    MsgArg::Text(text) if *name == key => Some(text.as_str()),
                    _ => None,
                })
            };
            let place = match (named("field"), named("source")) {
                (Some(field), Some(source)) => format!(": its {field}, from {source}"),
                (Some(field), None) => format!(": its {field}"),
                _ => String::new(),
            };
            format!(
                "a secret of its repository was refused ({}){place}",
                wire_name(refusal.code)
            )
        }
        StartError::KeyFilesNotLeftOut => format!(
            "its sources hold the data folder, whose secret key files no glob can leave out, \
             as its path isn't UTF-8 ({})",
            wire_name(ErrorCode::KeyFilesNotLeftOut)
        ),
    }
}

#[async_trait]
impl PlanRuns for PlanRunner {
    async fn start(&self, plan: &PlanId, trigger: Trigger, place: Place) {
        match self.start_plan(plan, trigger.clone(), Some(place)).await {
            Ok(_) => {}
            // Refused for a secret of its repository, it ends failed with the
            // refusal, so the history and the page say why the schedule's
            // backups stopped (§5), and it is retried as any failed backup the
            // schedule started (decision 76). Its place is given up already.
            Err(Unbuilt::Secret(secret)) => {
                let SecretUnbuilt {
                    refusal,
                    failure,
                    target,
                } = *secret;
                let operation = OperationId::from_uuid(Uuid::now_v7());
                // As a refusal by hand is logged (#483), never the refusal's
                // words, which only the history shows, masked (I4).
                log::warn!(
                    "{} of plan {} didn't start, and ends as failed operation {}: {}",
                    run_named(&trigger),
                    plan.as_str(),
                    operation.as_uuid().hyphenated(),
                    start_refused(&StartError::SecretRefused(refusal))
                );
                let finished = RunOperation::new(
                    Arc::clone(&self.engine),
                    Arc::new(SystemClock),
                    Arc::clone(&self.store),
                )
                .with_events(Arc::clone(&self.events))
                .refused(operation, trigger, target, failure)
                .await;
                log_recording_errors(&finished);
                say_how_it_ended(&self.ends, &finished);
            }
            Err(Unbuilt::Refused(error)) => log_not_started(plan, &trigger, &error),
        }
    }

    async fn resume(&self, mut operation: domain::execution::Operation) {
        let id = operation.id();
        let built = match operation.target().plan.clone() {
            Some(plan) => match self.config.load().await {
                Ok(config) => self
                    .backup_of(&config, &plan, operation.trigger())
                    .await
                    .map(|built| (built, plan, config.agent().concurrency)),
                Err(error) => Err(Unbuilt::Refused(config_unusable(
                    error,
                    |code, args, detail| StartError::Unavailable { code, args, detail },
                ))),
            },
            None => Err(Unbuilt::Refused(StartError::UnknownPlan)),
        };
        // Refused for a secret of the repository its plan still backs up to,
        // it ends failed with the refusal, as a refused scheduled run does
        // (§5, #490), and is heard as any run's end, so retries follow it.
        let built = match built {
            Err(Unbuilt::Secret(secret))
                if operation.target().is_on(secret.target.repository.as_ref()) =>
            {
                let SecretUnbuilt {
                    refusal, failure, ..
                } = *secret;
                log::warn!(
                    "the queued backup {} can't run again, and ends as failed: {}",
                    id.as_uuid().hyphenated(),
                    start_refused(&StartError::SecretRefused(refusal))
                );
                let finished = RunOperation::new(
                    Arc::clone(&self.engine),
                    Arc::new(SystemClock),
                    Arc::clone(&self.store),
                )
                .with_events(Arc::clone(&self.events))
                .refuse_resumed(operation, failure)
                .await;
                log_recording_errors(&finished);
                say_how_it_ended(&self.ends, &finished);
                return;
            }
            Err(Unbuilt::Secret(_)) => {
                Err("its plan now backs up to another repository".to_owned())
            }
            other => other.map_err(|error| start_refused(&error.into_start_error())),
        };
        let built = built.and_then(|(built, plan, limit)| {
            // The plan may have moved to another repository since the
            // queue's own read: it isn't run there under a record of the old.
            if operation
                .target()
                .is_on(built.job.target().repository.as_ref())
            {
                Ok((built, plan, limit))
            } else {
                Err("its plan now backs up to another repository".to_owned())
            }
        });
        match built {
            Ok((
                BuiltBackup {
                    job,
                    setup,
                    warnings,
                },
                plan,
                limit,
            )) => {
                // Oldest first, as the queue hands them on: each waits for
                // its turn like any other backup (§5).
                let place = self.queue.push(&plan, limit);
                let cancel = self.cancel_switch(id);
                let use_case = self.backup_use_case(setup);
                let registry = Arc::clone(&self.registry);
                let ends = self.ends.clone();
                tokio::spawn(self.runs.until_frozen(async move {
                    let queued =
                        app::execution::Queued::resumed(operation, job).with_warnings(warnings);
                    let finished = use_case.run_in_turn(queued, place, cancel).await;
                    registry.finish(&id);
                    log_ended(&finished);
                    say_how_it_ended(&ends, &finished);
                }));
            }
            // The queue can't run it again, as decision 40 ended every
            // queued operation: its plan or repository is gone or changed, its
            // plan moved and the secret is refused, or the configuration can't be read. Left
            // queued, it would show as queued with nothing to run it until
            // the next start (decision 73). An end the log took is announced,
            // as every end a start makes is (decision 91).
            Err(why) => {
                let hyphenated = id.as_uuid().hyphenated();
                log::warn!(
                    "the queued backup {hyphenated} can't run again, and ends as failed: {why}"
                );
                if operation
                    .fail(FailureReason::DaemonStopped, SystemClock.now())
                    .is_err()
                {
                    return;
                }
                match self.store.save(&operation, None).await {
                    Ok(()) => {
                        if let Some(news) = app::announce::end_of(&operation) {
                            self.events.publish(news).await;
                        }
                    }
                    Err(refused) => log::warn!(
                        "operation {hyphenated}: a write to the operation log failed ({}); it stays queued",
                        refused.code()
                    ),
                }
            }
        }
    }

    fn report(&self, report: ScheduleReport) {
        match report {
            ScheduleReport::Skipped { plan, unmet } => log::info!(
                "the scheduled run of plan {} was skipped: a condition it sets isn't met ({unmet:?})",
                plan.as_str()
            ),
            ScheduleReport::Overlapped { plan, overlapping } => log::info!(
                "the scheduled run of plan {} was skipped: {}",
                plan.as_str(),
                match overlapping {
                    domain::execution::queue::Overlapping::Running =>
                        "its last run is still running",
                    domain::execution::queue::Overlapping::Waiting =>
                        "a run of it is already waiting for its turn",
                }
            ),
            ScheduleReport::PowerUnknown { plan, error } => log::warn!(
                "the scheduled run of plan {} starts, though what powers the machine is unknown: {error:?}",
                plan.as_str()
            ),
            ScheduleReport::NetworkWaited(NetworkWait::Online { waited }) => {
                log::info!("the catch-up runs after a wake waited {waited:#} for the network")
            }
            ScheduleReport::NetworkWaited(NetworkWait::GaveUp) => log::warn!(
                "the catch-up runs after a wake start, though the machine is still offline"
            ),
            ScheduleReport::NetworkWaited(NetworkWait::Unknown(error)) => log::warn!(
                "the catch-up runs after a wake start, though whether the machine is online is unknown: {error:?}"
            ),
            ScheduleReport::NoNextRun { plan } => log::warn!(
                "plan {} isn't run again: its schedule has no run after now",
                plan.as_str()
            ),
            ScheduleReport::Store(error) => log::warn!(
                "the scheduler couldn't read or write the operation log ({}); it goes on with what it holds",
                error.code()
            ),
            ScheduleReport::Config(error) => log::warn!(
                "the scheduler couldn't read the configuration, and goes on with the plans it last read: {error}"
            ),
            ScheduleReport::QueuedEnded {
                operation,
                kind,
                why,
            } => log::warn!(
                "the queued {kind:?} {} can't run again, and ends as failed ({why:?})",
                operation.as_uuid().hyphenated()
            ),
            ScheduleReport::Retrying { plan, retry } => log::info!(
                "the backup of plan {} failed, and retry {} runs at {}",
                plan.as_str(),
                retry.attempt,
                retry.at
            ),
            ScheduleReport::RetriesEnded { plan, why } => log::info!(
                "the retries of plan {} ended without a success: {}",
                plan.as_str(),
                match why {
                    domain::execution::retry::NoRetry::Exhausted => "its attempts are spent",
                    domain::execution::retry::NoRetry::NextRun =>
                        "its next scheduled run comes first",
                    domain::execution::retry::NoRetry::RunGoing => "a run of it is going",
                }
            ),
        }
    }
}

#[async_trait]
impl Operations for PlanRunner {
    async fn start_backup(&self, plan: String, trigger: Trigger) -> Result<String, StartError> {
        let id = PlanId::try_from(plan).map_err(|_| StartError::UnknownPlan)?;
        // Refused by hand as on a schedule, the run leaves a line in the log
        // too, not only the answer the page shows for a moment (#483).
        let operation = self
            .start_plan(&id, trigger.clone(), None)
            .await
            .map_err(Unbuilt::into_start_error)
            .inspect_err(|error| log_not_started(&id, &trigger, error))?;
        Ok(operation.as_uuid().hyphenated().to_string())
    }

    async fn start_maintenance(
        &self,
        repository: String,
        request: http_adapter::MaintenanceRequest,
        selection: Option<String>,
    ) -> Result<String, MaintenanceRefusal> {
        maintenance::start(self, repository, request, selection).await
    }

    async fn preview(
        &self,
        repository: String,
        request: http_adapter::MaintenanceRequest,
    ) -> Result<api::MaintenancePreviewDto, MaintenanceRefusal> {
        maintenance::preview(self, repository, request).await
    }

    async fn groups(
        &self,
        repository: String,
    ) -> Result<api::SnapshotGroupsDto, MaintenanceRefusal> {
        maintenance::groups(self, repository).await
    }

    async fn stats(
        &self,
        repository: String,
    ) -> Result<api::RepositoryStatsDto, MaintenanceRefusal> {
        maintenance::stats(self, repository).await
    }

    async fn read_stats(
        &self,
        repository: String,
    ) -> Result<api::StatsReadingDto, MaintenanceRefusal> {
        maintenance::read_stats(self, repository).await
    }

    async fn history(
        &self,
        plan: Option<String>,
        after: Option<String>,
        limit: Option<u64>,
    ) -> Result<api::OperationsPageDto, http_adapter::HistoryError> {
        let empty = || api::OperationsPageDto {
            operations: Vec::new(),
            next: None,
        };
        // A filter that names no plan that could exist matches nothing.
        let plan = match plan.map(PlanId::try_from) {
            Some(Ok(plan)) => Some(plan),
            Some(Err(invalid)) => {
                log::warn!("a plan filter no plan could match: {invalid}");
                return Ok(empty());
            }
            None => None,
        };
        let after = match after {
            Some(cursor) => match cursor.parse::<Uuid>() {
                Ok(uuid) => Some(OperationId::from_uuid(uuid)),
                Err(_) => return Err(http_adapter::HistoryError::BadCursor),
            },
            None => None,
        };
        let limit = limit.map(|asked| usize::try_from(asked).unwrap_or(usize::MAX));
        let page = match ListOperations::new(Arc::clone(&self.store))
            .list(plan, after, limit)
            .await
        {
            Ok(page) => page,
            Err(error) => {
                log::error!("the operation log could not be read: {error}");
                return Ok(empty());
            }
        };
        Ok(api::OperationsPageDto {
            operations: page.operations.iter().filter_map(summary).collect(),
            next: page.next.map(|id| id.as_uuid().hyphenated().to_string()),
        })
    }

    async fn operation(
        &self,
        id: String,
    ) -> Result<api::OperationDetailDto, http_adapter::LookupError> {
        let id = operation_of(&id).ok_or(http_adapter::LookupError::Unknown)?;
        let detail = GetOperation::new(Arc::clone(&self.store))
            .get(id)
            .await
            .map_err(|error| {
                log::error!("the operation log could not be read: {error}");
                http_adapter::LookupError::Unknown
            })?
            .ok_or(http_adapter::LookupError::Unknown)?;
        let Some(operation) = summary(&detail.record) else {
            log::error!(
                "a stored operation the page cannot show: {}",
                detail.record.id.as_uuid().hyphenated()
            );
            return Err(http_adapter::LookupError::Unknown);
        };
        Ok(api::OperationDetailDto {
            operation,
            warnings: detail
                .warnings
                .iter()
                .map(|warning| api::OperationWarningDto {
                    code: warning_code(warning.kind),
                    item: warning.item.clone(),
                    during: warning.during.clone(),
                    detail: warning.detail.clone(),
                })
                .collect(),
        })
    }

    async fn cancel(
        &self,
        id: String,
        by: http_adapter::Canceller,
    ) -> Result<(), http_adapter::CancelError> {
        let Some(id) = operation_of(&id) else {
            return Err(http_adapter::CancelError::Unknown);
        };
        // The log says which of the refusals this is: unknown, known but not
        // one a token may stop, or known but ended. Only a running operation
        // has a handle to fire (§4.2).
        let record = match GetOperation::new(Arc::clone(&self.store)).get(id).await {
            Ok(Some(detail)) => detail.record,
            Ok(None) => return Err(http_adapter::CancelError::Unknown),
            Err(error) => {
                log::error!("the operation log could not be read: {error}");
                return Err(http_adapter::CancelError::Unknown);
            }
        };
        if by == http_adapter::Canceller::ApiToken
            && !domain::access::api_token::cancels(record.kind)
        {
            return Err(http_adapter::CancelError::SessionOnly);
        }
        if !record.status.is_finished() && self.registry.cancel(&id) {
            Ok(())
        } else {
            Err(http_adapter::CancelError::NotRunning)
        }
    }
}

/// Tells the scheduler how a backup's run ended, so that it can retry a
/// failed one (§5). The run's place has left the queue by now.
fn say_how_it_ended(ends: &app::retry::RunEndSender, finished: &Finished) {
    if let Some(end) = app::retry::RunEnd::of(&finished.operation) {
        ends.send(end);
    }
}

/// Says in the log that an operation ended, and each write the operation log
/// refused it (§9): a run goes on without a log that refuses its writes, so
/// these lines are the only sign that the log has stopped taking them. A
/// refused write is named by the code of its failure and never by the
/// failure's own words, which nobody has checked for a value (I4).
fn log_ended(finished: &Finished) {
    log::info!(
        "operation {} ended: {:?}",
        finished.operation.id().as_uuid().hyphenated(),
        finished.operation.status()
    );
    log_recording_errors(finished);
    if let Some(failed) = &finished.stats_failed {
        log::warn!(
            "operation {}: the repository's stats couldn't be read after the check ({}); the check's outcome stands",
            finished.operation.id().as_uuid().hyphenated(),
            stats_failure(failed)
        );
    }
}

/// Says in the log each write of `finished` the operation log refused, as
/// [`log_ended`] does.
fn log_recording_errors(finished: &Finished) {
    let operation = finished.operation.id().as_uuid().hyphenated();
    for refused in &finished.recording_errors {
        log::warn!(
            "operation {operation}: a write to the operation log failed ({}); its history may be incomplete",
            refused.code()
        );
    }
}

/// Why a stats read failed, for the daemon's log: by the code of the failure,
/// never its own words, which nobody has checked for a value (I4).
fn stats_failure(failed: &app::stats::StatsError) -> String {
    use app::stats::StatsError as E;
    match failed {
        E::Resolve(_) => "its secrets couldn't be resolved".to_owned(),
        E::Engine(_) => "the engine couldn't start the read".to_owned(),
        E::Failed(reason) => format!("the read failed: {}", reason.code()),
        E::TimedOut => "the read wasn't answered in time".to_owned(),
        E::Store(error) => format!("the history refused the reading: {}", error.code()),
    }
}

/// An operation's ID as the wire carries it, when the text is one.
fn operation_of(text: &str) -> Option<OperationId> {
    text.parse::<Uuid>().ok().map(OperationId::from_uuid)
}

/// One log record as the history page shows it, or `None` when it holds
/// something no page could show. The page carries the status and each
/// warning's code once, from the kept warnings (#508); the warnings
/// themselves come with the detail.
fn summary(record: &OperationRecord) -> Option<api::OperationSummaryDto> {
    let failure = match &record.status {
        OperationStatus::Failed { reason } => Some(failure(reason)),
        _ => None,
    };
    api::OperationSummaryDto::new(
        record.id.as_uuid().hyphenated().to_string(),
        record
            .target
            .plan
            .as_ref()
            .map_or_default(|plan| plan.as_str().to_owned()),
        kind_text(record.kind).to_owned(),
        status_text(&record.status).to_owned(),
        record.queued_at.to_string(),
        record.started_at.map(|at| at.to_string()),
        record.finished_at.map(|at| at.to_string()),
        record
            .warning_kinds
            .iter()
            .map(|kind| warning_code(*kind))
            .collect(),
        failure,
        record
            .result
            .as_ref()
            .and_then(|result| result_dto(result, record)),
        record
            .repository_alias
            .as_ref()
            .map(|alias| alias.as_str().to_owned()),
    )
    .map(|summary| {
        let summary =
            summary.with_locations(record.locations.as_ref().map_or_default(locations_tried));
        match &record.entry {
            Some(entry) => summary.with_entry(entry.as_str().to_owned()),
            None => summary,
        }
    })
    .map_err(|invalid| log::warn!("an operation the page cannot show: {invalid:?}"))
    .ok()
}

/// What a successful operation produced, as the page shows it: the small
/// typed summary of its kind, with the run's duration.
fn result_dto(result: &EngineResult, record: &OperationRecord) -> Option<api::OperationResultDto> {
    let duration_ms = record
        .started_at
        .zip(record.finished_at)
        .and_then(|(from, to)| u64::try_from(to.as_millisecond() - from.as_millisecond()).ok())
        .unwrap_or(0);
    match result {
        EngineResult::Backup(backup) => Some(api::OperationResultDto::Backup {
            files: backup
                .files_new
                .saturating_add(backup.files_changed)
                .saturating_add(backup.files_unmodified),
            bytes: backup.data_added,
            duration_ms,
            snapshot: Some(backup.snapshot.as_str().to_owned()),
            bytes_processed: backup.bytes_processed,
        }),
        EngineResult::Prune(prune) => Some(api::OperationResultDto::Prune {
            packs_deleted: prune.packs_deleted,
            duration_ms,
        }),
        EngineResult::Check(check) => Some(api::OperationResultDto::Check {
            errors: check.errors,
            warnings: check.warnings,
            duration_ms,
        }),
        EngineResult::Forget(forget) => Some(api::OperationResultDto::Forget {
            removed: u64::try_from(forget.removed.len()).unwrap_or(0),
            duration_ms,
        }),
        EngineResult::Rewrite(rewrite) => Some(api::OperationResultDto::Adopt {
            rewritten: u64::try_from(rewrite.created.len()).unwrap_or(0),
            duration_ms,
        }),
        EngineResult::Restore(restore) => Some(api::OperationResultDto::Restore {
            files: restore.files_restored,
            bytes: restore.bytes_restored,
            duration_ms,
        }),
        // Reads: a result no operation row holds, so the page shows none.
        EngineResult::Fake { .. }
        | EngineResult::Repository { .. }
        | EngineResult::PrunePlan(_)
        | EngineResult::RestorePlan(_)
        | EngineResult::Probed(_)
        | EngineResult::Snapshots(_)
        | EngineResult::Folder(_)
        | EngineResult::Stats(_)
        | EngineResult::Dump(_) => None,
    }
}

/// An operation's kind, as the wire names it.
fn kind_text(kind: OperationKind) -> &'static str {
    match kind {
        OperationKind::Fake => "fake",
        OperationKind::Identify => "identify",
        OperationKind::Init => "init",
        OperationKind::Backup => "backup",
        OperationKind::Forget => "forget",
        OperationKind::Adopt => "adopt",
        OperationKind::Prune => "prune",
        OperationKind::Check => "check",
        OperationKind::Ls => "ls",
        OperationKind::Restore => "restore",
        OperationKind::Download => "download",
    }
}

/// An operation's status, as the wire names it.
fn status_text(status: &OperationStatus) -> &'static str {
    match status {
        OperationStatus::Queued => "queued",
        OperationStatus::Running => "running",
        OperationStatus::Succeeded => "succeeded",
        OperationStatus::SucceededWithWarnings => "succeeded-with-warnings",
        OperationStatus::Failed { .. } => "failed",
        OperationStatus::Cancelled => "cancelled",
    }
}

/// A warning's kind, as the wire names it.
fn warning_code(kind: WarningKind) -> WarningCode {
    match kind {
        WarningKind::SourceUnreadable => WarningCode::SourceUnreadable,
        WarningKind::Hook => WarningCode::HookWarning,
        WarningKind::MaintenancePartial => WarningCode::MaintenancePartial,
        WarningKind::CheckWarning => WarningCode::CheckWarning,
        WarningKind::CheckError => WarningCode::CheckError,
        WarningKind::OtherHosts => WarningCode::MaintenanceOtherHosts,
        WarningKind::LocationSkipped => WarningCode::LocationSkipped,
        WarningKind::SecretFileReadable => WarningCode::SecretFileReadable,
        WarningKind::FullDiskAccessMissing => WarningCode::FullDiskAccessMissing,
        WarningKind::Other => WarningCode::Unknown,
    }
}

/// The arguments a warning's message takes, as the page gets them: a
/// readable secret file's path and mode (#386), otherwise the item the
/// warning names, if any.
fn warning_args(warning: &domain::execution::Warning) -> Vec<MsgArg> {
    match warning.kind {
        WarningKind::SecretFileReadable => warning
            .item
            .iter()
            .cloned()
            .chain([warning.detail.clone()])
            .map(MsgArg::Text)
            .collect(),
        _ => warning
            .item
            .as_ref()
            .map_or_default(|item| vec![MsgArg::Text(item.clone())]),
    }
}

/// The running operations' cancel switches (§4.2): what the API cancels
/// through. A handle is registered when an operation starts running and
/// dropped when it ends, so only a running operation can be cancelled, once.
struct CancelRegistry {
    running: std::sync::Mutex<HashMap<OperationId, futures::channel::oneshot::Sender<()>>>,
}

/// The catalog the API asks of the daemon (§4.2): the configuration's plans,
/// with the scheduler's planned runs, and repositories, and one repository's
/// snapshots, listed live through the engine (§9: a read stores nothing and
/// takes no slot). Which repositories a row offers to create is read from
/// the operation log (decision 53).
struct ConfigCatalog {
    config: Arc<dyn ConfigStore>,
    engine: Arc<dyn Engine>,
    operations: Arc<dyn OperationStore>,
    schedules: Arc<dyn ScheduleStore>,
    /// Whether this machine reads the Wi-Fi network's name, for the notes
    /// on rules that name networks (decision 80).
    link: Arc<NameWatch>,
    /// A plan's sources as the backup walks them, which its globs are read
    /// against (#510).
    sources: Arc<dyn NormalizeSources>,
    /// The turns to read a folder of a snapshot, which every browse shares
    /// (decision 102).
    reads: Arc<ReadTurns>,
    /// The turns to list a repository's snapshots, which every listing
    /// shares (#710).
    listings: Arc<ReadTurns>,
}

#[async_trait]
impl Catalog for ConfigCatalog {
    async fn status(
        &self,
        full_disk_access: api::FullDiskAccessStateDto,
    ) -> Result<api::StatusDto, CatalogError> {
        let listed = self.plans().await?;
        let read = ReadStatus::new(
            Arc::clone(&self.config),
            Arc::clone(&self.operations),
            Arc::new(SystemClock),
            domain::TimeZone::system(),
        );
        let status = read.read().await.map_err(|error| match error {
            StatusError::Config(error) => catalog_refusal(*error),
            // The log's own words say what it couldn't do, as the history
            // tells it.
            StatusError::Store(
                app::ports::StoreError::Unavailable { detail }
                | app::ports::StoreError::Corrupt { detail },
            ) => CatalogError::Unavailable {
                code: ErrorCode::OperationFailed,
                args: Vec::new(),
                detail: Some(detail),
            },
        })?;
        let mut standings: HashMap<String, app::status::PlanStanding> = status
            .plans
            .into_iter()
            .map(|standing| (standing.plan.as_str().to_owned(), standing))
            .collect();
        // A plan the configuration gained or lost between the two readings
        // is left out until the next one.
        let plans = listed
            .plans
            .into_iter()
            .filter_map(|plan| {
                let standing = standings.remove(&plan.id)?;
                Some(api::PlanStatusDto {
                    plan,
                    last: standing.last.as_ref().and_then(summary),
                    last_success: standing.last_success.map(|success| api::LastSuccessDto {
                        operation: success.operation.id.as_uuid().hyphenated().to_string(),
                        finished_at: success
                            .operation
                            .finished_at
                            .unwrap_or(success.operation.queued_at)
                            .to_string(),
                        ago_seconds: u64::try_from(success.ago.as_secs()).unwrap_or(0),
                    }),
                    overdue: standing.overdue,
                })
            })
            .collect();
        Ok(api::StatusDto {
            plans,
            running: status.queue.iter().filter_map(summary).collect(),
            unreadable_repositories: status
                .unreadable
                .iter()
                .filter_map(|unreadable| {
                    Some(api::UnreadableRepositoryDto {
                        repository: unreadable.repository.as_str().to_owned(),
                        operation: summary(&unreadable.operation)?,
                    })
                })
                .collect(),
            full_disk_access,
        })
    }

    async fn plans(&self) -> Result<api::PlansDto, CatalogError> {
        let config = self.config.load().await.map_err(catalog_refusal)?;
        // A planned run that can't be read is no reason to hide the plans:
        // each scheduled one shows that it has none planned.
        let planned: HashMap<PlanId, domain::Timestamp> = match self.schedules.planned_runs().await
        {
            Ok(runs) => runs.into_iter().map(|run| (run.plan, run.at)).collect(),
            Err(error) => {
                log::warn!(
                    "the planned runs couldn't be read ({}); the plans are listed without them",
                    error.code()
                );
                HashMap::new()
            }
        };
        let mut plans = Vec::with_capacity(config.plans().len());
        for plan in config.plans() {
            let scheduled = plan.schedule.is_some();
            plans.push(api::PlanSummaryDto {
                id: plan.id.as_str().to_owned(),
                repository: plan.repository.as_str().to_owned(),
                scheduled,
                // A run kept for a plan whose schedule was just removed is
                // the scheduler's to forget, not a run.
                next_run: planned
                    .get(&plan.id)
                    .filter(|_| scheduled)
                    .map(ToString::to_string),
                warnings: never_matching_glob_warnings(plan, self.sources.as_ref())
                    .await
                    .into_iter()
                    .map(|(code, args)| api::WarningDto::new(code, args))
                    .collect(),
            });
        }
        Ok(api::PlansDto { plans })
    }

    async fn repositories(&self) -> Result<api::RepositoriesDto, CatalogError> {
        let config = self.config.load().await.map_err(catalog_refusal)?;
        let offered = self.offers(&config).await;
        // A rule is noted on a mixed link, and a rule that names networks
        // when the name can't be read: the first listing that has a rule
        // reads the link, and the name when one is named, once, in the
        // background, as the reads may take seconds.
        let rules = || {
            config
                .repositories()
                .values()
                .flat_map(|repository| repository.location_rules().into_iter().flatten())
        };
        if rules().next().is_some() {
            let names_asked = rules().any(LocationRule::asks_name);
            let link = Arc::clone(&self.link);
            tokio::spawn(async move { link.check_once(names_asked).await });
        }
        let reads_names = self.link.reads_names();
        let last_link = self.link.last_link();
        Ok(api::RepositoriesDto {
            repositories: config
                .repositories()
                .iter()
                .map(|(alias, repository)| {
                    let plans: Vec<&Plan> = config
                        .plans()
                        .iter()
                        .filter(|plan| &plan.repository == alias)
                        .collect();
                    let mut row = listed_repository(
                        alias,
                        repository,
                        &plans,
                        reads_names,
                        offered.contains(alias),
                    );
                    row.warnings.extend(mixed_link_note(repository, last_link));
                    row
                })
                .collect(),
        })
    }

    async fn snapshots(&self, alias: String) -> Result<api::SnapshotsDto, SnapshotsError> {
        // An alias no repository could have is as unknown as one the
        // configuration happens not to have.
        let alias = RepositoryAlias::parse(&alias).map_err(|_| SnapshotsError::Unknown)?;
        let config = self.config.load().await.map_err(|error| {
            config_unusable(error, |code, args, detail| SnapshotsError::Unavailable {
                code,
                args,
                detail,
            })
        })?;
        let repository = config.repository(&alias).ok_or(SnapshotsError::Unknown)?;
        let listing = ListSnapshots::new(
            Arc::clone(&self.engine),
            Arc::new(SystemClock),
            ResolveRepository::new(Arc::new(SystemSecrets::default()))
                .telling(Arc::new(log_secret_warning)),
            Arc::clone(&self.listings),
        )
        .list(OperationId::from_uuid(Uuid::now_v7()), &alias, repository)
        .await;
        let listed = match listing {
            Ok(listed) => listed,
            Err(error) => return Err(listing_refusal(&alias, &repository.settings, error).await),
        };
        let snapshots = listed
            .iter()
            .filter_map(|snapshot| {
                snapshot_dto(snapshot)
                    .map_err(|problem| {
                        log::error!(
                            "a snapshot of {} the page cannot show: {problem:?}",
                            alias.as_str()
                        );
                    })
                    .ok()
            })
            .collect();
        Ok(api::SnapshotsDto { snapshots })
    }

    async fn folder(
        &self,
        alias: String,
        snapshot: SnapshotId,
        folder: FolderPath,
        from: u64,
    ) -> Result<api::FolderDto, FolderError> {
        // The repository is refused as its listing is (`snapshots`).
        let unknown = || FolderError::Repository(SnapshotsError::Unknown);
        let alias = RepositoryAlias::parse(&alias).map_err(|_| unknown())?;
        let config = self.config.load().await.map_err(|error| {
            FolderError::Repository(config_unusable(error, |code, args, detail| {
                SnapshotsError::Unavailable { code, args, detail }
            }))
        })?;
        let repository = config.repository(&alias).ok_or_else(unknown)?;
        let path = folder.to_string();
        let read = BrowseSnapshot::new(
            Arc::clone(&self.engine),
            Arc::new(SystemClock),
            ResolveRepository::new(Arc::new(SystemSecrets::default()))
                .telling(Arc::new(log_secret_warning)),
            Arc::clone(&self.reads),
        )
        .folder(
            OperationId::from_uuid(Uuid::now_v7()),
            &alias,
            repository,
            snapshot,
            folder,
            from,
        )
        .await;
        let page = match read {
            Ok(page) => page,
            Err(error) => return Err(folder_refusal(&alias, &repository.settings, error).await),
        };
        Ok(api::FolderDto {
            path,
            entries: page.entries.into_iter().map(entry_dto).collect(),
            total: page.total,
            next: page.next,
        })
    }
}

/// A listing or a folder read the engine ran and failed, told by the code of
/// its failure as a read for maintenance tells it (#637): a wrong password is
/// `password-not-accepted`, no repository at the location 404. `None` for a
/// failure in the engine's own words or an engine that can't run, which the
/// caller tells as `engine-unavailable`, its words only in the log.
fn told_failure(reason: &FailureReason) -> Option<SnapshotsError> {
    use http_adapter::MaintenanceRefusal as M;
    match maintenance::failed(reason.clone()) {
        M::Unavailable {
            code: ErrorCode::EngineUnavailable,
            ..
        }
        | M::OperationFailed { .. } => None,
        M::Unavailable { code, args, detail } => {
            Some(SnapshotsError::Unavailable { code, args, detail })
        }
        M::SecretRefused(refusal) => Some(SnapshotsError::SecretRefused(refusal)),
        M::RepositoryNotFound => Some(SnapshotsError::RepositoryNotFound),
        M::RepositoryConfigNotFound { detail } => {
            Some(SnapshotsError::RepositoryConfigNotFound { detail })
        }
        // `failed` gives none of these: they refuse a request before
        // anything runs, or time a read the listing times itself. Each is
        // named, so a refusal added later is decided here and is never told
        // as `engine-unavailable` unnoticed.
        M::UnknownRepository
        | M::NotOwner
        | M::UnknownPlan { .. }
        | M::Busy { .. }
        | M::KeepsNothing { .. }
        | M::AdoptionNoMatch
        | M::AdoptionAmbiguous { .. }
        | M::InvalidBody
        | M::SelectionChanged
        | M::TimedOut => None,
    }
}

/// An entry of a snapshot's folder as the page is told of it.
fn entry_dto(entry: domain::browse::Entry) -> api::EntryDto {
    use domain::browse::EntryKind;
    api::EntryDto {
        name: entry.name.into(),
        display: entry.display.into(),
        kind: match entry.kind {
            EntryKind::Folder => api::EntryKindDto::Folder,
            EntryKind::File => api::EntryKindDto::File,
            EntryKind::Link => api::EntryKindDto::Link,
            EntryKind::Other => api::EntryKindDto::Other,
        },
        size: entry.size,
        modified: entry.modified.map(|at| at.to_string()),
    }
}

/// A configuration the lists can't read, refused as a save of it is.
fn catalog_refusal(error: ConfigError) -> CatalogError {
    config_unusable(error, |code, args, detail| CatalogError::Unavailable {
        code,
        args,
        detail,
    })
}

/// The refusal of a read of a folder of a snapshot of `alias`, configured
/// with `settings`, that failed with `error`. A secret: its code and the
/// words a command or the platform gave, masked of every secret of the
/// repository, in the log as in the answer (I4). A read that ran out of
/// time by its own code; the rest by its code alone, its masked words in
/// the log.
async fn folder_refusal(
    alias: &RepositoryAlias,
    settings: &RepositorySettings,
    error: BrowseError,
) -> FolderError {
    use app::maintenance::MaintenanceError;
    let refused = |refusal: SecretRefusal| {
        log::warn!(
            "a folder of a snapshot of {} could not be read: {:?}{}",
            alias.as_str(),
            refusal.code,
            refusal
                .detail
                .as_deref()
                .map_or_default(|detail| format!(" ({detail})"))
        );
        FolderError::Repository(SnapshotsError::SecretRefused(refusal))
    };
    match error {
        BrowseError::SnapshotUnknown => FolderError::SnapshotUnknown,
        BrowseError::FolderUnknown => FolderError::FolderUnknown,
        BrowseError::Resolve(error) => {
            refused(secret_refusal(&error, settings, &SystemSecrets::default()).await)
        }
        BrowseError::Read(MaintenanceError::Failed(FailureReason::SecretsTooLargeToMask {
            field,
            limit,
        })) => refused(too_much_to_mask(field, limit)),
        BrowseError::Read(MaintenanceError::TimedOut) => {
            log::warn!(
                "a folder of a snapshot of {} could not be read in time",
                alias.as_str()
            );
            FolderError::Repository(SnapshotsError::TimedOut)
        }
        error => {
            if let BrowseError::Read(MaintenanceError::Failed(reason)) = &error
                && let Some(told) = told_failure(reason)
            {
                log::warn!(
                    "a folder of a snapshot of {} could not be read: {told:?}",
                    alias.as_str()
                );
                return FolderError::Repository(told);
            }
            let said = failure_redactor(settings, &SystemSecrets::default())
                .await
                .redact(&format!("{error:?}"));
            log::warn!(
                "a folder of a snapshot of {} could not be read: {said}",
                alias.as_str()
            );
            FolderError::Repository(SnapshotsError::Unavailable {
                code: ErrorCode::EngineUnavailable,
                args: Vec::new(),
                detail: None,
            })
        }
    }
}

/// The refusal of a listing of the snapshots of `alias`, configured with
/// `settings`, that failed with `error`. A secret: its code and the words a
/// command or the platform gave, masked of every secret of the repository,
/// in the log as in the answer (I4). A read that ran out of time by its own
/// code; the rest by its code alone, its masked words in the log.
async fn listing_refusal(
    alias: &RepositoryAlias,
    settings: &RepositorySettings,
    error: SnapshotError,
) -> SnapshotsError {
    let refused = |refusal: SecretRefusal| {
        log::warn!(
            "the snapshots of {} could not be listed: {:?}{}",
            alias.as_str(),
            refusal.code,
            refusal
                .detail
                .as_deref()
                .map_or_default(|detail| format!(" ({detail})"))
        );
        SnapshotsError::SecretRefused(refusal)
    };
    match error {
        SnapshotError::Resolve(error) => {
            refused(secret_refusal(&error, settings, &SystemSecrets::default()).await)
        }
        SnapshotError::Engine(EngineError::SecretsTooLargeToMask { field, limit }) => {
            refused(too_much_to_mask(field, limit))
        }
        SnapshotError::TimedOut => {
            log::warn!(
                "the snapshots of {} could not be listed in time",
                alias.as_str()
            );
            SnapshotsError::TimedOut
        }
        error => {
            if let SnapshotError::Failed(reason) = &error
                && let Some(told) = told_failure(reason)
            {
                log::warn!(
                    "the snapshots of {} could not be listed: {told:?}",
                    alias.as_str()
                );
                return told;
            }
            let said = failure_redactor(settings, &SystemSecrets::default())
                .await
                .redact(&format!("{error:?}"));
            log::warn!(
                "the snapshots of {} could not be listed: {said}",
                alias.as_str()
            );
            SnapshotsError::Unavailable {
                code: ErrorCode::EngineUnavailable,
                args: Vec::new(),
                detail: None,
            }
        }
    }
}

impl ConfigCatalog {
    /// The repositories whose rows offer to create them (decision 53). A log
    /// that can't be read offers none: the list is still shown, and Create
    /// is never offered on a guess.
    async fn offers(
        &self,
        config: &domain::planning::AgentConfig,
    ) -> std::collections::BTreeSet<RepositoryAlias> {
        match CreateOffers::new(Arc::clone(&self.operations))
            .offered(config)
            .await
        {
            Ok(offered) => offered,
            Err(error) => {
                log::warn!(
                    "the operation log could not be read, so no repository is offered to be created: {}",
                    error.code()
                );
                std::collections::BTreeSet::new()
            }
        }
    }
}

/// A listed snapshot as the page is told of it, with what the engine cut
/// from it said.
fn snapshot_dto(snapshot: &SnapshotSummary) -> Result<api::SnapshotDto, api::InvalidSnapshot> {
    api::SnapshotDto::new(
        snapshot.id.as_str().to_owned(),
        snapshot.time.to_string(),
        snapshot.hostname.clone(),
        snapshot.label.clone(),
        snapshot.tags.clone(),
        snapshot.paths.clone(),
    )
    .map(|dto| {
        dto.truncated(snapshot.paths_truncated, snapshot.tags_truncated)
            .taken_at_offset(snapshot.offset)
    })
}

/// Tests, connects and initialises repositories for the API (§4.2): each
/// request's settings become domain values, run through the use cases of
/// `app`, and every refusal becomes a typed one. Writes to the configuration
/// go one at a time, so two connections made at the same moment are both
/// kept.
struct RepositoryService {
    clock: Arc<dyn Clock>,
    engine: Arc<dyn Engine>,
    /// The network link a Test evaluates the locations' rules against.
    link: Arc<dyn NetworkLink>,
    resolve: ResolveRepository,
    config: Arc<dyn ConfigStore>,
    /// The operation log, which says whether a saved repository's row
    /// offers Create (decision 53).
    operations: Arc<dyn OperationStore>,
    /// Held across a use case's read-modify-write of the configuration, and
    /// shared with the record of an opened repository's ID.
    writes: Arc<ConfigWrites>,
    /// Where a typed password is kept when the person chose the system
    /// keyring (§10.2): the OS keyring the resolver reads.
    keyring: Arc<dyn Keyring>,
}

impl RepositoryService {
    fn new(
        clock: Arc<dyn Clock>,
        engine: Arc<dyn Engine>,
        link: Arc<dyn NetworkLink>,
        resolve: ResolveRepository,
        config: Arc<dyn ConfigStore>,
        operations: Arc<dyn OperationStore>,
        writes: Arc<ConfigWrites>,
    ) -> Self {
        Self {
            clock,
            engine,
            link,
            resolve,
            config,
            operations,
            writes,
            keyring: Arc::new(SystemSecrets::default()),
        }
    }

    /// The service keeping typed passwords in `keyring` rather than in the
    /// OS keyring, so that no test writes to the developer's own.
    #[cfg(test)]
    fn with_keyring(self, keyring: Arc<dyn Keyring>) -> Self {
        Self { keyring, ..self }
    }
}

#[async_trait]
impl Repositories for RepositoryService {
    async fn test(
        &self,
        request: api::TestRepositoryDto,
    ) -> Result<api::RepositoryIdDto, RepositoriesError> {
        let settings = settings_of(request.settings)?;
        let id = match TestRepository::new(
            Arc::clone(&self.clock),
            Arc::clone(&self.engine),
            self.resolve.clone(),
        )
        .test(new_operation(), &settings)
        .await
        {
            Ok(value) => value,
            Err(error) => return Err(refusal_of(error, &settings).await),
        };
        Ok(api::RepositoryIdDto {
            id: id.as_str().to_owned(),
        })
    }

    async fn connect(
        &self,
        request: api::ConnectRepositoryDto,
    ) -> Result<api::RepositoryIdDto, RepositoriesError> {
        // The name first: a repository that can't be saved isn't tested.
        let alias =
            RepositoryAlias::parse(&request.alias).map_err(|_| RepositoriesError::AliasInvalid)?;
        let settings = settings_of(request.settings)?;
        let keep_in_keyring = keeps_in_keyring(request.keep_in_keyring, Some(&alias), &settings)?;
        // Asked for, this agent maintains it; otherwise nobody is asked, for
        // a repository that exists may already be maintained elsewhere (I1).
        let maintenance = if request.maintain_here {
            Maintenance::Local
        } else {
            Maintenance::None
        };
        let _writing = self.writes.lock().await;
        let connecting = ConnectRepository::new(
            Arc::clone(&self.clock),
            Arc::clone(&self.engine),
            self.resolve.clone(),
            Arc::clone(&self.config),
        );
        let connecting = if keep_in_keyring {
            connecting.keeping_password_in(Arc::clone(&self.keyring))
        } else {
            connecting
        };
        let id = match connecting
            .connect(new_operation(), alias, &settings, maintenance)
            .await
        {
            Ok(value) => value,
            Err(error) => return Err(refusal_of(error, &settings).await),
        };
        Ok(api::RepositoryIdDto {
            id: id.as_str().to_owned(),
        })
    }

    async fn init(
        &self,
        request: api::InitRepositoryDto,
    ) -> Result<api::RepositoryInitDto, RepositoriesError> {
        // The name first: a repository that was created and then can't be
        // saved is one the person has no way to find again.
        let alias = request
            .alias
            .as_deref()
            .map(RepositoryAlias::parse)
            .transpose()
            .map_err(|_| RepositoriesError::AliasInvalid)?;
        let settings = settings_of(request.settings)?;
        let keep_in_keyring = keeps_in_keyring(request.keep_in_keyring, alias.as_ref(), &settings)?;
        // Only a saving initialisation writes the configuration.
        let _writing = match alias {
            Some(_) => Some(self.writes.lock().await),
            None => None,
        };
        let initialising = InitRepository::new(
            Arc::clone(&self.clock),
            Arc::clone(&self.engine),
            self.resolve.clone(),
            Arc::clone(&self.config),
        );
        let initialising = if keep_in_keyring {
            initialising.keeping_password_in(Arc::clone(&self.keyring))
        } else {
            initialising
        };
        let initialised = match initialising.init(new_operation(), &settings, alias).await {
            Ok(value) => value,
            Err(error) => return Err(refusal_of(error, &settings).await),
        };
        Ok(api::RepositoryInitDto {
            id: initialised.id.as_str().to_owned(),
            saved: initialised.saved,
        })
    }

    async fn create_saved(&self, alias: String) -> Result<api::RepositoryIdDto, RepositoriesError> {
        // A name that is no alias is one the configuration can't hold.
        let Ok(parsed) = RepositoryAlias::parse(&alias) else {
            return Err(RepositoriesError::Unknown { alias });
        };
        // Create records the new repository's ID in its entry, and whether
        // the row offers it is read under the same lock, so two Creates
        // can't both find it offered.
        let _writing = self.writes.lock().await;
        let saved = match InitRepository::new(
            Arc::clone(&self.clock),
            Arc::clone(&self.engine),
            self.resolve.clone(),
            Arc::clone(&self.config),
        )
        .init_saved(
            new_operation(),
            &parsed,
            &CreateOffers::new(Arc::clone(&self.operations)),
        )
        .await
        {
            Ok(saved) => saved,
            // Before the entry is found and its row known to offer Create,
            // nothing ran: the configuration can't be read, it doesn't hold
            // the alias, or the operation log can't be read.
            Err(SavedEntryError::Config(error)) => return Err(config_refusal(error)),
            Err(SavedEntryError::Unknown(_)) => return Err(RepositoriesError::Unknown { alias }),
            // The log's own words say what it couldn't do, as maintenance
            // tells it.
            Err(SavedEntryError::Log(
                app::ports::StoreError::Unavailable { detail }
                | app::ports::StoreError::Corrupt { detail },
            )) => {
                return Err(unavailable(
                    ErrorCode::OperationFailed,
                    Vec::new(),
                    Some(detail),
                ));
            }
        };
        match saved.created {
            Ok(id) => Ok(api::RepositoryIdDto {
                id: id.as_str().to_owned(),
            }),
            Err(error) => Err(refusal_of(error, &saved.settings).await),
        }
    }

    async fn test_saved(&self, alias: String) -> Result<api::LocationsTestDto, RepositoriesError> {
        // A name that is no alias is one the configuration can't hold.
        let Ok(parsed) = RepositoryAlias::parse(&alias) else {
            return Err(RepositoriesError::Unknown { alias });
        };
        let config = self.config.load().await.map_err(config_refusal)?;
        let Some(entry) = config.repositories().get(&parsed).cloned() else {
            return Err(RepositoriesError::Unknown { alias });
        };
        // Probes go through the daemon's own engine, which hands a probe
        // straight to a worker; the log hears what a job's choice tells it.
        let saved = app::ports::SavedEntry {
            alias: parsed,
            entry,
        };
        let tested = ChooseLocation::new(
            Arc::clone(&self.engine),
            Arc::clone(&self.clock),
            Arc::clone(&self.link),
            self.resolve.clone(),
            Arc::new(log_location),
        )
        .test_each(new_operation(), &saved)
        .await;
        match tested {
            Ok(record) => Ok(api::LocationsTestDto {
                locations: locations_tried(&record),
            }),
            Err(error) => Err(refusal_of(error, &saved.entry.settings).await),
        }
    }

    async fn change_maintainer(
        &self,
        alias: String,
        request: api::MaintainerDto,
    ) -> Result<(), RepositoriesError> {
        // A name that is no alias is one the configuration can't hold.
        let Ok(parsed) = RepositoryAlias::parse(&alias) else {
            return Err(RepositoriesError::Unknown { alias });
        };
        // The change runs under the configuration's write lock, which a
        // maintenance start holds too while it reads again whether this agent
        // maintains the repository and takes its slot (I1).
        match ChangeMaintainer::new(
            Arc::clone(&self.config),
            Arc::clone(&self.operations),
            Arc::clone(&self.writes),
        )
        .change(&parsed, request.maintain_here)
        .await
        {
            Ok(_) => Ok(()),
            Err(MaintainerError::Unknown(_)) => Err(RepositoriesError::Unknown { alias }),
            Err(MaintainerError::Busy { running }) => Err(RepositoriesError::MaintenanceBusy {
                operation: running.as_uuid().hyphenated().to_string(),
            }),
            Err(MaintainerError::Config(error)) => Err(config_refusal(error)),
            // The log's own words say what it couldn't do, as Create tells
            // it.
            Err(MaintainerError::Log(
                app::ports::StoreError::Unavailable { detail }
                | app::ports::StoreError::Corrupt { detail },
            )) => Err(unavailable(
                ErrorCode::OperationFailed,
                Vec::new(),
                Some(detail),
            )),
        }
    }

    /// Asks the OS keyring the page's question, and only it: never a run. A
    /// configuration that can't be read notes nothing; the page's list says
    /// why it can't be read.
    async fn notes(&self) -> api::RepositoryNotesDto {
        let notes = match self.config.load().await {
            Ok(config) => app::repositories::page_notes(&*self.keyring, &config).await,
            Err(_) => Vec::new(),
        };
        api::RepositoryNotesDto { notes }
    }
}

/// Every location of a choice or a Test, as the wire carries it: by
/// position, the address masked as recorded, and the outcome's code.
fn locations_tried(record: &LocationsRecord) -> Vec<api::LocationTriedDto> {
    record
        .locations
        .iter()
        .map(|tried| api::LocationTriedDto {
            position: tried.position,
            address: tried.address.clone(),
            outcome: tried.outcome.code().to_owned(),
            source: tried.source.clone(),
        })
        .collect()
}

/// A fresh operation ID: a UUIDv7, as every operation of the daemon has.
fn new_operation() -> OperationId {
    OperationId::from_uuid(Uuid::now_v7())
}

/// A request's settings as domain values, read as the configuration reader
/// reads the same keys: a password file is an absolute path, and a keyring
/// entry is `service/name`. A typed password and a location (hot ones too)
/// are not empty. What it can't take is an invalid body, except a setting
/// that names a path on this machine by a relative one, which is refused at
/// that setting.
fn settings_of(dto: api::RepositorySettingsDto) -> Result<RepositorySettings, RepositoriesError> {
    let empty_password =
        matches!(&dto.password, api::PasswordDto::Inline { value } if value.is_empty());
    if empty_password || dto.repository.is_empty() || dto.repo_hot.as_deref() == Some("") {
        return Err(RepositoriesError::InvalidBody);
    }
    let password = match dto.password {
        api::PasswordDto::Inline { value } => SecretRef::Inline(SecretValue::new(value)),
        api::PasswordDto::File { path } if path.starts_with('/') => SecretRef::File {
            path,
            read: FileRead::FirstLine,
        },
        api::PasswordDto::File { path } if path.is_empty() => {
            return Err(RepositoriesError::InvalidBody);
        }
        api::PasswordDto::File { .. } => return Err(path_relative(Field::Password)),
        api::PasswordDto::Keyring { entry } => SecretRef::Keyring(
            KeyringEntry::parse(&entry).map_err(|_| RepositoriesError::InvalidBody)?,
        ),
        api::PasswordDto::Env { name } => SecretRef::Env(name),
    };
    let settings = RepositorySettings {
        repository: dto.repository,
        repo_hot: dto.repo_hot,
        password,
        options: dto.options,
        options_hot: dto.options_hot,
        options_cold: dto.options_cold,
    };
    // What the configuration would refuse when it loads next is not saved,
    // nor tested: a request can't take more than a job does. A value over
    // its cap is refused for that first, as the configuration refuses it.
    settings
        .check_sizes()
        .map_err(|_| RepositoriesError::InvalidBody)?;
    match settings.relative_path() {
        Some(field) => Err(path_relative(match field {
            RepositoryField::Repository => Field::Repository,
            RepositoryField::RepoHot => Field::RepoHot,
            // A request has no password command; the field is the password.
            RepositoryField::PasswordCommand => Field::Password,
            RepositoryField::Option(name) => Field::Option(name),
            RepositoryField::OptionHot(name) => Field::OptionHot(name),
            RepositoryField::OptionCold(name) => Field::OptionCold(name),
        })),
        None => Ok(settings),
    }
}

/// The refusal of a relative path at `field`, named as the configuration
/// names it.
fn path_relative(field: Field) -> RepositoriesError {
    RepositoriesError::PathRelative {
        field: field.name(),
    }
}

/// Why a repository use case failed, as the typed refusal the route answers
/// with. `settings` are the request's: a location the refusal names is shown
/// redacted.
async fn refusal_of(error: RepositoryError, settings: &RepositorySettings) -> RepositoriesError {
    match error {
        RepositoryError::Exists => exists(settings),
        RepositoryError::CreateNotOffered => RepositoriesError::CreateNotOffered,
        RepositoryError::AliasTaken(alias) => RepositoriesError::AliasTaken {
            alias: alias.as_str().to_owned(),
        },
        RepositoryError::AlreadyConfigured(alias) => RepositoriesError::AlreadyConfigured {
            alias: alias.as_str().to_owned(),
        },
        RepositoryError::Resolve(error) => RepositoriesError::SecretRefused(
            secret_refusal(&error, settings, &SystemSecrets::default()).await,
        ),
        RepositoryError::Engine(EngineError::Unavailable { detail }) => {
            unavailable(ErrorCode::EngineUnavailable, Vec::new(), Some(detail))
        }
        RepositoryError::Engine(EngineError::StartTimedOut) => {
            unavailable(ErrorCode::EngineStartTimedOut, Vec::new(), None)
        }
        RepositoryError::Engine(EngineError::SecretsTooLargeToMask { field, limit }) => {
            RepositoriesError::SecretRefused(too_much_to_mask(field, limit))
        }
        RepositoryError::Failed(reason) => failed_refusal(reason, settings),
        RepositoryError::Config(error) => config_refusal(error),
        RepositoryError::Keyring { entry, error } => {
            RepositoriesError::SecretRefused(keyring_refusal(&entry, error, settings).await)
        }
        RepositoryError::TimedOut => unavailable(ErrorCode::RepositoryTimedOut, Vec::new(), None),
    }
}

/// Whether a request that asks to keep its password in the system keyring
/// may: the keyring keeps a password typed for a repository saved under a
/// name (§10.2), so a request with no name, or with a password that is
/// already a reference, is one the form never sends. A password longer than
/// the resolver reads from an entry (`MAX_SECRET_BYTES`) is refused too, as
/// settings over their caps are, rather than kept where no job could read it.
fn keeps_in_keyring(
    asked: bool,
    alias: Option<&RepositoryAlias>,
    settings: &RepositorySettings,
) -> Result<bool, RepositoriesError> {
    let readable = settings
        .password
        .inline()
        .is_some_and(|typed| typed.expose().len() <= domain::secrets::MAX_SECRET_BYTES);
    if asked && (alias.is_none() || !readable) {
        return Err(RepositoriesError::InvalidBody);
    }
    Ok(asked)
}

/// A typed password the keyring couldn't keep, as the secret refusal the
/// routes answer with: the entry it was to be kept in, and what the platform
/// said, masked of every secret of the repository, the typed password first
/// (I4). A keyring that never answered is one that couldn't keep it.
async fn keyring_refusal(
    entry: &KeyringEntry,
    error: KeyringError,
    settings: &RepositorySettings,
) -> SecretRefusal {
    let (code, said) = match error {
        KeyringError::Taken => (ErrorCode::KeyringEntryTaken, None),
        KeyringError::Unavailable { detail } => (ErrorCode::KeyringUnavailable, Some(detail)),
        KeyringError::Locked { detail } => (ErrorCode::KeyringLocked, Some(detail)),
        KeyringError::Unwritable { detail } => (ErrorCode::KeyringUnwritable, Some(detail)),
        KeyringError::TimedOut { .. } => (ErrorCode::KeyringUnwritable, None),
    };
    let detail = match said.filter(|said| !said.is_empty()) {
        Some(said) => Some(
            failure_redactor(settings, &SystemSecrets::default())
                .await
                .redact(&said),
        ),
        None => None,
    };
    SecretRefusal {
        code,
        args: vec![("entry", MsgArg::Text(entry.to_string()))],
        detail,
    }
}

/// Secrets that come to more than a job masks, as the secret refusal the
/// routes answer with: the cap, and the setting where they passed it, when
/// one is known (§10.2). Neither is a secret or tells one (I4).
pub(crate) fn too_much_to_mask(field: Option<String>, limit: usize) -> SecretRefusal {
    let mut args = vec![(
        "limit",
        MsgArg::Bytes(u64::try_from(limit).unwrap_or(u64::MAX)),
    )];
    args.extend(field.map(|field| ("field", MsgArg::Text(field))));
    SecretRefusal {
        code: ErrorCode::SecretsTooLargeToMask,
        args,
        detail: None,
    }
}

fn unavailable(
    code: ErrorCode,
    args: Vec<(&'static str, MsgArg)>,
    detail: Option<String>,
) -> RepositoriesError {
    RepositoriesError::Unavailable { code, args, detail }
}

fn exists(settings: &RepositorySettings) -> RepositoriesError {
    RepositoriesError::Exists {
        location: redacted_location(settings, &settings.repository),
    }
}

/// Says in the daemon's log what reading a set of secrets warned of: a file
/// other users can read, by its path, masked of the secrets read with it,
/// and its mode, never the secret (#386, I4). Each job, snapshot listing
/// and notification that reads secrets tells it; a re-read that only masks
/// a refusal doesn't (§10.2).
fn log_secret_warning(warning: &SecretWarning) {
    match warning {
        SecretWarning::FileReadableByOthers { path, mode } => log::warn!(
            "{:?}: other users of this machine can read the secret file {path} \
             (permissions {mode:04o}); run `chmod 600` on it so that only its \
             owner can read it",
            WarningCode::SecretFileReadable
        ),
    }
}

/// The daemon's resolution of secrets, through `secrets`: each warning it
/// gives is said in the log, and a run nobody waits on holds for the
/// session's keyring service while the daemon, which started at the
/// monotonic time `started` by `clock`, starts (§5, decision 116).
fn daemon_resolve(
    secrets: Arc<dyn app::ports::SecretResolver>,
    clock: Arc<dyn Clock>,
    started: domain::SignedDuration,
) -> ResolveRepository {
    ResolveRepository::new(secrets)
        .telling(Arc::new(log_secret_warning))
        .waiting_for_keyring(app::secrets::KeyringWait::new(clock, started))
}

/// Says once that the run `trigger` started of plan `plan` waits for the
/// session's keyring service to read the entry `source` names: the entry,
/// never a secret (I4).
fn log_keyring_wait(trigger: &Trigger, plan: &PlanId, source: &domain::secrets::SourceName) {
    use domain::secrets::SourceName;
    let name = match source {
        SourceName::Keyring { entry } => entry,
        SourceName::Command { program } => program,
        SourceName::File { path } => path,
        SourceName::Env { name } => name,
    };
    log::info!(
        "{} of plan {} waits for the session's keyring service to read {name}, \
         until it answers or the daemon has been up for {:#}",
        run_named(trigger),
        plan.as_str(),
        app::secrets::KEYRING_WAIT
    );
}

/// A secret that couldn't be resolved, as the typed refusal of its cause
/// ([`ResolveError::refusal`]). What a platform or a command said is masked
/// of every secret of the repository first (I4), read again through
/// `resolver`. A refusal that shows nothing a source said reads nothing: a
/// location with too many credentials, or a reference that would reach a
/// process's arguments, is refused before any secret is read (§10.2).
async fn secret_refusal(
    error: &ResolveError,
    settings: &RepositorySettings,
    resolver: &dyn SecretResolver,
) -> SecretRefusal {
    refusal_with(error, &refusal_redactor(error, settings, resolver).await)
}

/// [`secret_refusal`], with what `redactor` masks.
fn refusal_with(error: &ResolveError, redactor: &Redactor) -> SecretRefusal {
    match error.refusal(redactor) {
        Ok(refusal) => {
            let (code, args, detail) = app::announce::refused(&error.field.name(), &refusal);
            SecretRefusal { code, args, detail }
        }
        Err(limit) => too_much_to_mask(Some(error.field.name()), limit),
    }
}

/// What masks the texts of `error`'s refusal: only a secret that couldn't be
/// read has any, where it was looked for and what was said, so only then are
/// the repository's secrets read again ([`failure_redactor`]).
async fn refusal_redactor(
    error: &ResolveError,
    settings: &RepositorySettings,
    resolver: &dyn SecretResolver,
) -> Redactor {
    match error.cause {
        Cause::Unresolved(_) => failure_redactor(settings, resolver).await,
        Cause::Invalid(_)
        | Cause::ExpansionTooLong { .. }
        | Cause::ReferenceInProcessArguments
        | Cause::TooManyCredentials { .. }
        | Cause::TooMuchToMask { .. }
        | Cause::RelativePath => Redactor::new([]),
    }
}

/// How long a secret is read again for a redactor: a source that does not
/// answer in this time adds nothing, and a failure doesn't wait for it.
const REREAD_LIMIT: std::time::Duration = std::time::Duration::from_secs(5);

/// A secret read again, for a redactor: `None` when it can't be read, since
/// what can't be read can't show in what a failure says.
async fn read_again(resolver: &dyn SecretResolver, secret: &SecretRef) -> Option<SecretValue> {
    tokio::time::timeout(REREAD_LIMIT, resolver.resolve(secret))
        .await
        .ok()?
        .ok()
        .map(|found| found.value)
}

/// A redactor for what a failure of `settings` says: its inline password and
/// credential options, and what the references of its settings, and a
/// password read from the environment or a file, resolve to through
/// `resolver`. A failed resolution stops at the first fault, so the other
/// secrets of the repository were not read yet: they are read again here, so
/// that a command that prints one of them in its error doesn't show it. A
/// command is not run a second time. Once what was read again passes what a
/// redactor masks, nothing more is read and the redactor masks every text
/// whole, since a secret not read could be in it (I4).
async fn failure_redactor(
    settings: &RepositorySettings,
    resolver: &dyn SecretResolver,
) -> Redactor {
    let mut resolved = Vec::new();
    let mut masked = MaskedSecrets::default();
    if matches!(
        settings.password,
        SecretRef::Env(_) | SecretRef::File { .. }
    ) && let Some(value) = read_again(resolver, &settings.password).await
    {
        if masked.secret(&value).is_err() {
            return Redactor::masking_everything();
        }
        resolved.push(value);
    }
    let texts = std::iter::once(&settings.repository)
        .chain(&settings.repo_hot)
        .chain(settings.options.values())
        .chain(settings.options_hot.values())
        .chain(settings.options_cold.values());
    // Each distinct reference once, as the resolution reads it.
    let mut references = std::collections::BTreeSet::new();
    for text in texts {
        if let Ok(template) = Template::parse(text) {
            for part in template.parts() {
                if let Part::Reference(reference) = part {
                    references.insert(reference.clone());
                }
            }
        }
    }
    for reference in references {
        if let Some(value) = read_again(resolver, &reference.secret_ref()).await {
            if masked.secret(&value).is_err() {
                return Redactor::masking_everything();
            }
            resolved.push(value);
        }
    }
    let none = SecretValue::new("");
    Redactor::for_repository(RepositorySecrets {
        repository: &settings.repository,
        repo_hot: settings.repo_hot.as_deref(),
        password: settings.password.inline().unwrap_or(&none),
        options: &settings.options,
        options_hot: &settings.options_hot,
        options_cold: &settings.options_cold,
        resolved: &resolved,
    })
}

/// A job that ran and failed: the engine's own words for a failure it
/// reports, a typed code for the rest. The engine redacts what it says.
fn failed_refusal(reason: FailureReason, settings: &RepositorySettings) -> RepositoriesError {
    use FailureReason as F;
    match reason {
        F::EngineFailed { detail } => RepositoriesError::OperationFailed { detail },
        F::EngineUnavailable { detail } | F::JobRejected { detail } => {
            unavailable(ErrorCode::EngineUnavailable, Vec::new(), Some(detail))
        }
        F::EngineExited { exit_code } => unavailable(
            ErrorCode::EngineExited,
            exit_code
                .map(|code| ("exit_code", text(code)))
                .into_iter()
                .collect(),
            None,
        ),
        F::ProtocolViolated { transition } => unavailable(
            ErrorCode::ProtocolViolated,
            vec![("transition", text(format!("{transition:?}")))],
            None,
        ),
        F::DeadlinePassed => unavailable(ErrorCode::DeadlinePassed, Vec::new(), None),
        F::RestoreTargetUnwritable => {
            unavailable(ErrorCode::RestoreTargetUnwritable, Vec::new(), None)
        }
        F::EngineStartTimedOut => unavailable(ErrorCode::EngineStartTimedOut, Vec::new(), None),
        F::EngineExitTimedOut => unavailable(ErrorCode::EngineExitTimedOut, Vec::new(), None),
        // Only a backup's before-hook stops a job this way: a repository job
        // runs no hooks. It is still a failure, in the catalog's plainest words.
        F::HookFailed { .. } => unavailable(ErrorCode::OperationFailed, Vec::new(), None),
        F::RepositoryExists => exists(settings),
        F::RepositoryReplaced => unavailable(ErrorCode::RepositoryReplaced, Vec::new(), None),
        F::RepositoryNotFound => RepositoriesError::NotFound,
        F::RepositoryConfigNotFound { detail } => RepositoriesError::ConfigNotFound { detail },
        F::CredentialsRefused { half } => unavailable(
            ErrorCode::CredentialsRefused,
            half.map(|half| ("half", text(half.code())))
                .into_iter()
                .collect(),
            None,
        ),
        F::SecretsTooLargeToMask { field, limit } => {
            RepositoriesError::SecretRefused(too_much_to_mask(field, limit))
        }
        F::DaemonStopped => unavailable(ErrorCode::DaemonStopped, Vec::new(), None),
        F::DaemonShutDown => unavailable(ErrorCode::DaemonShutDown, Vec::new(), None),
        F::NoLocationUsable => unavailable(ErrorCode::NoLocationUsable, Vec::new(), None),
        F::NoLocationRuleHolds => unavailable(ErrorCode::NoLocationRuleHolds, Vec::new(), None),
        F::PasswordNotAccepted => unavailable(ErrorCode::PasswordNotAccepted, Vec::new(), None),
        // Only a run the schedule started records a refusal (§5): a
        // repository job is refused before it runs, as `secret_refusal` says.
        F::SecretRefused { field, refusal } => {
            let (code, args, detail) = app::announce::refused(&field, &refusal);
            RepositoriesError::SecretRefused(SecretRefusal { code, args, detail })
        }
    }
}

/// A configuration that couldn't be read or written, refused by a repository
/// route.
fn config_refusal(error: ConfigError) -> RepositoriesError {
    config_unusable(error, unavailable)
}

/// A configuration that couldn't be read or written, as the code and the
/// arguments its message declares, which `unavailable` makes a route's
/// refusal of: every route that reads the file says the same of it (#347).
/// The store never puts a value in what it reports (`ConfigError`), so its
/// words can be shown.
fn config_unusable<R>(
    error: ConfigError,
    unavailable: impl FnOnce(ErrorCode, Vec<(&'static str, MsgArg)>, Option<String>) -> R,
) -> R {
    match error {
        ConfigError::Unavailable { file, detail } => unavailable(
            ErrorCode::ConfigUnavailable,
            vec![("file", text(file))],
            Some(detail),
        ),
        ConfigError::TooLarge { file, limit } => unavailable(
            ErrorCode::ConfigTooLarge,
            vec![("file", text(file)), ("limit", MsgArg::Bytes(limit))],
            None,
        ),
        ConfigError::SchemaMissing { file } => unavailable(
            ErrorCode::ConfigSchemaMissing,
            vec![("file", text(file))],
            None,
        ),
        ConfigError::SchemaNewer {
            file,
            found,
            supported,
        } => unavailable(
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
            detail,
            ..
        } => {
            let mut args = vec![("file", text(file))];
            if let Some(at) = at {
                args.push(("line", text(at.line)));
            }
            let detail = match (key, detail.is_empty()) {
                (Some(key), false) => Some(format!("{key}: {detail}")),
                (Some(key), true) => Some(key),
                (None, false) => Some(detail),
                (None, true) => None,
            };
            unavailable(ErrorCode::ConfigInvalid, args, detail)
        }
    }
}

/// The Repositories page's note on `repository` while `link`, the link the
/// daemon last read, is mixed and the entry has a rule (decision 80): the
/// kind of link each family's traffic leaves by, never an interface's name.
fn mixed_link_note(repository: &RepositoryConfig, link: Option<Link>) -> Option<api::WarningDto> {
    let kind = |kind| {
        text(match kind {
            InterfaceKind::Wired => "wired",
            InterfaceKind::WiFi => "wifi",
            InterfaceKind::Other => "other",
        })
    };
    repository
        .link_note(link)
        .map(|MixedLinkNote { ipv4, ipv6 }| api::WarningDto {
            code: api::WarningCode::LocationRuleMixedLink,
            args: [
                ("ipv4".to_owned(), kind(ipv4)),
                ("ipv6".to_owned(), kind(ipv6)),
                ("locations".to_owned(), text(repository.locations_code())),
            ]
            .into(),
        })
}

/// A configured entry as the repositories list shows it: its row, with the
/// secrets each further location writes as text by its position; for an
/// entry reached at several locations, each location in order with its
/// rule; a warning for each location that sends its credentials where an
/// impostor could read them, by position, cause and whether it has a rule,
/// never the address; and the notes about what the locations' rules mean
/// for `plans`, the plans that back up to it (decision 80). The warnings
/// and notes of a repository of one location say so, for its row lists no
/// locations.
fn listed_repository(
    alias: &RepositoryAlias,
    repository: &RepositoryConfig,
    plans: &[&Plan],
    reads_names: bool,
    create_offered: bool,
) -> api::RepositoryDto {
    let mut row = repository_row(
        alias,
        &repository.settings,
        repository.maintenance,
        create_offered,
    );
    let dto = |code, args: Vec<(&'static str, MsgArg)>| api::WarningDto {
        code,
        args: args
            .into_iter()
            .map(|(name, value)| (name.to_owned(), value))
            .collect(),
    };
    // The row holds the first location's secrets already.
    row.warnings.extend(
        inline_secret_warnings(repository)
            .into_iter()
            .filter(|(position, _, _)| *position > 1)
            .map(|(_, code, args)| dto(code, args)),
    );
    row.warnings
        .extend(repository.location_warnings().into_iter().map(|warning| {
            dto(
                api::WarningCode::LocationPlainHttpCredentials,
                exposed_args(repository, warning),
            )
        }));
    let locations = || ("locations".to_owned(), text(repository.locations_code()));
    row.warnings.extend(
        repository
            .rule_notes(plans, reads_names)
            .into_iter()
            .map(|note| {
                let (code, args) = match note {
                    RuleNote::EveryLocationRuled => {
                        (api::WarningCode::LocationEveryRuled, vec![locations()])
                    }
                    RuleNote::LastRuledWithoutRetry { plan } => (
                        api::WarningCode::LocationLastRuledNoRetry,
                        vec![("plan".to_owned(), text(plan.as_str())), locations()],
                    ),
                    RuleNote::NameUnreadable { position } => (
                        api::WarningCode::LocationRuleNameUnreadable,
                        vec![("position".to_owned(), text(position)), locations()],
                    ),
                };
                api::WarningDto {
                    code,
                    args: args.into_iter().collect(),
                }
            }),
    );
    if repository.has_several_locations() {
        let first = Location {
            when: repository.locations.first_when.clone(),
            ..Location::of(&repository.settings)
        };
        row.locations = std::iter::once(first)
            .chain(repository.locations.further.iter().cloned())
            .zip(1u8..)
            .map(|(location, position)| listed_location(&location, position))
            .collect();
    }
    row
}

/// One location of an entry as the list shows it: its address masked, and
/// each option by name, with its value only when that is references alone.
fn listed_location(location: &Location, position: u8) -> api::LocationDto {
    let options = [
        ("options", &location.options),
        ("options-hot", &location.options_hot),
        ("options-cold", &location.options_cold),
    ]
    .into_iter()
    .flat_map(|(set, options)| {
        options
            .iter()
            .map(move |(name, value)| api::LocationOptionDto {
                set: set.to_owned(),
                name: name.clone(),
                reference: domain::secrets::only_references(value).then(|| value.clone()),
            })
    })
    .collect();
    api::LocationDto {
        position,
        repository: location.masked_address(),
        repo_hot: location.masked_repo_hot(),
        options,
        when: location
            .when
            .iter()
            .flat_map(LocationRule::alternatives)
            .map(|alternative| {
                let names = |filter: Option<&NameFilter>| {
                    filter.map_or_default(|filter| {
                        filter
                            .names()
                            .iter()
                            .map(|name| name.as_str().to_owned())
                            .collect()
                    })
                };
                let filter = alternative.names();
                api::LocationAlternativeDto {
                    network: alternative.kind().as_str().to_owned(),
                    only: names(filter.filter(|filter| matches!(filter, NameFilter::Only(_)))),
                    except: names(filter.filter(|filter| matches!(filter, NameFilter::Except(_)))),
                }
            })
            .collect(),
    }
}

/// One configured repository as a list shows it: its locations masked, and
/// where its password comes from, never the password (I4). `create_offered`
/// is whether its row offers to create it (decision 53).
fn repository_row(
    alias: &RepositoryAlias,
    settings: &RepositorySettings,
    maintenance: domain::planning::Maintenance,
    create_offered: bool,
) -> api::RepositoryDto {
    api::RepositoryDto {
        alias: alias.as_str().to_owned(),
        maintained_here: domain::maintenance::Ownership::owned_here(maintenance),
        create_offered,
        repository: redacted_location(settings, &settings.repository),
        repo_hot: settings
            .repo_hot
            .as_deref()
            .map(|hot| redacted_location(settings, hot)),
        password: match &settings.password {
            SecretRef::Inline(_) => api::PasswordSourceDto::Inline,
            SecretRef::File { path, .. } => api::PasswordSourceDto::File { path: path.clone() },
            SecretRef::Keyring(entry) => api::PasswordSourceDto::Keyring {
                entry: entry.to_string(),
            },
            SecretRef::Env(name) => api::PasswordSourceDto::Env { name: name.clone() },
            // The program only: its arguments may hold the secret itself.
            SecretRef::Command(command) => api::PasswordSourceDto::Command {
                program: command.program().to_owned(),
            },
        },
        warnings: settings
            .inline_secrets()
            .iter()
            .map(|secret| {
                let (code, args) = inline_secret_warning(settings, secret, 1);
                api::WarningDto {
                    code,
                    args: args
                        .into_iter()
                        .map(|(name, value)| (name.to_owned(), value))
                        .collect(),
                }
            })
            .collect(),
        locations: Vec::new(),
    }
}

/// A location of `settings`, as it may be shown: with the password a URL
/// holds masked. The typed password is not masked: it isn't in a location,
/// and a location that happens to spell it is still a location.
fn redacted_location(settings: &RepositorySettings, text: &str) -> String {
    Redactor::for_repository(RepositorySecrets {
        repository: &settings.repository,
        repo_hot: settings.repo_hot.as_deref(),
        password: &SecretValue::new(""),
        options: &settings.options,
        options_hot: &settings.options_hot,
        options_cold: &settings.options_cold,
        resolved: &[],
    })
    .redact(text)
}

impl CancelRegistry {
    fn new() -> Self {
        Self {
            running: std::sync::Mutex::new(HashMap::new()),
        }
    }

    /// The map, recovered even from a poisoned lock: the alternative —
    /// handing out a receiver whose sender is gone — would cancel every
    /// operation that starts next.
    fn running(
        &self,
    ) -> std::sync::MutexGuard<'_, HashMap<OperationId, futures::channel::oneshot::Sender<()>>>
    {
        self.running
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Registers `id` as running, returning the future its run ends on when
    /// the API cancels it.
    fn register(&self, id: OperationId) -> futures::channel::oneshot::Receiver<()> {
        let (sender, receiver) = futures::channel::oneshot::channel();
        self.running().insert(id, sender);
        receiver
    }

    /// Cancels `id`: `true` when it was running and is now cancelled, `false`
    /// when nothing by that ID is running, so there is nothing to cancel.
    /// A second cancel of the same operation is `false`: the one handle is
    /// spent.
    fn cancel(&self, id: &OperationId) -> bool {
        self.running()
            .remove(id)
            .is_some_and(|sender| sender.send(()).is_ok())
    }

    /// The operation ended, cancelled or not: its handle, fired or spent, is
    /// dropped, so a later cancel is a typed refusal.
    fn finish(&self, id: &OperationId) {
        let _ = self.running().remove(id);
    }
}

/// What the daemon logs, for the tests that look at the log: the warnings it
/// gives (`lines`) and what it says at info level (`infos`). `log` takes one
/// logger for a process, so the tests of every module that reads the log
/// share this one, and each reads only the lines about what it started.
#[cfg(test)]
mod captured_log {
    use std::sync::{Mutex, PoisonError};

    struct Captured;

    /// Each record's level, target and text.
    static LOGGED: Mutex<Vec<(log::Level, String, String)>> = Mutex::new(Vec::new());

    impl log::Log for Captured {
        fn enabled(&self, _: &log::Metadata<'_>) -> bool {
            true
        }
        fn log(&self, record: &log::Record<'_>) {
            LOGGED.lock().unwrap_or_else(PoisonError::into_inner).push((
                record.level(),
                record.target().to_owned(),
                record.args().to_string(),
            ));
        }
        fn flush(&self) {}
    }

    /// Starts capturing, before the code that logs runs, at info level or
    /// more, as another test of the process may have asked.
    pub(super) fn capture() {
        capture_at(log::LevelFilter::Info);
    }

    /// Starts capturing at `level` or more: a test that asks for less never
    /// lowers what another test of the process asked for.
    pub(super) fn capture_at(level: log::LevelFilter) {
        let _ = log::set_logger(&Captured);
        log::set_max_level(log::max_level().max(level));
    }

    /// Every line logged so far at warning level or above, with its level.
    pub(super) fn lines() -> Vec<(log::Level, String)> {
        LOGGED
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .filter(|(level, _, _)| *level <= log::Level::Warn)
            .map(|(level, _, line)| (*level, line.clone()))
            .collect()
    }

    /// Every line logged so far at info level.
    pub(super) fn infos() -> Vec<String> {
        LOGGED
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .filter(|(level, _, _)| *level == log::Level::Info)
            .map(|(_, _, line)| line.clone())
            .collect()
    }

    /// The targets of every record logged so far at `level`.
    pub(super) fn targets_at(level: log::Level) -> Vec<String> {
        LOGGED
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .filter(|(at, _, _)| *at == level)
            .map(|(_, target, _)| target.clone())
            .collect()
    }

    /// Every line logged so far that the daemon's log, under `RUST_LOG` set
    /// to `rust_log`, would write.
    pub(super) fn written_under(rust_log: &str) -> Vec<String> {
        use log::Log as _;
        let daemon = crate::logging::logger(
            Some(rust_log),
            env_logger::Target::Pipe(Box::new(std::io::sink())),
            env_logger::WriteStyle::Never,
        );
        LOGGED
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .filter(|(level, target, _)| {
                daemon.enabled(
                    &log::Metadata::builder()
                        .level(*level)
                        .target(target)
                        .build(),
                )
            })
            .map(|(_, _, line)| line.clone())
            .collect()
    }
}

#[cfg(test)]
mod tests;
