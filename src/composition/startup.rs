//! The daemon's start-up (technical design §4.2, §9): the hold on the data
//! folder, the configuration and the store read once, every adapter built
//! and handed to the router, and the work that no request starts.

use api::{MsgArg, WarningCode};
use app::conditions::NameWatch;
use app::event_bus::BroadcastEventBus;
use app::full_disk_access::{FullDiskAccessCheck, StartNote};
use app::locations::{ChooseLocation, LocationReport};
use app::notifications::{Notify, Report};
use app::ports::DELIVERY_TIMEOUT;
use app::ports::{
    Clock, ConfigError, ConfigStore, Engine, EventBus, Network, NormalizeSources, OperationRecord,
    OperationStore, PowerSupply, RELEASE_TIMEOUT, Random, ReleaseFeed, ReleaseFeedError,
    ScheduleStore,
};
use app::queue::RunQueue;
use app::reads::ReadTurns;
use app::recovery::RecoverOperations;
use app::repositories::{ConfigWrites, RecordOpened, Recording};
use app::schedule::{PlanRuns, Scheduler};
use app::secrets::ResolveRepository;
use app::updates::{CheckForUpdates, ReleaseCheckReport, UpdateNotice};
use async_trait::async_trait;
use config_adapter::FileConfigStore;
use domain::RepositoryId;
use domain::execution::locations::LocationOutcome;
use domain::planning::{NotRecorded, RestoreFolder};
use domain::updates::Version;
use engine::{Recorded, RemovedKind, WorkerEngine};
use hooks::ProcessHookRunner;
use http_adapter::{AppState, Catalog, Operations, Plans, Repositories, SignIn};
use l10n::{MessageId, msg};
use notify_adapter::HttpNotifier;
use platform::SystemClock;
use platform::admin_password::FileAdminPassword;
use platform::data_dir_lock::{DataDirLock, LOCK_FILE, LockError};
use platform::random::OsRandom;
use platform::{agent_id, dirs};
use releases_adapter::GitHubReleases;
use secrets::SystemSecrets;
use std::collections::BTreeSet;
use std::sync::Arc;
use store::SqliteStore;

use super::catalog::ConfigCatalog;
use super::key_files::KeyFilesLeftOut;
use super::plan_runner::PlanRunner;
use super::repositories::RepositoryService;
use super::{
    CancelRegistry, Daemon, EventEngine, OperationLog, Refused, SignInDecided, downloads, dry_runs,
    exposed_args, inline_secret_warnings, log_secret_warning, never_matching_glob_warnings, plain,
    plans, profiles, proposals, restores, runs, text,
};
use app::ports::config_unusable;

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
pub(super) struct WithinLimit<S>(pub(super) S, pub(super) std::time::Duration);

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
    /// The listings a paired machine's sync runs before each session, of
    /// each repository it maintains that has no listing standing, and what
    /// the daemon's listings showed of other agents' snapshots, which the
    /// sync tells its hub with each repository (§7.3; §16, decision 156).
    #[cfg(feature = "hub")]
    listings_due: Arc<app::others_snapshots::ListingsDue>,
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

    /// The configuration the daemon runs on, which a paired machine's sync
    /// reads for the leases it asks its hub for (§7.3).
    #[cfg(feature = "hub")]
    pub(crate) fn config(&self) -> Arc<dyn ConfigStore> {
        Arc::clone(&self.config)
    }

    /// Where this machine stands, which its sync with a hub it paired with
    /// tells what the hub made of it (decision 150).
    #[cfg(feature = "hub")]
    pub(crate) fn standing(&self) -> Arc<app::machine::MachineStanding> {
        Arc::clone(&self.runner.machine)
    }

    /// What a paired machine tells its hub of its plans (§7.2): their
    /// standing in the log this daemon keeps, and their planned runs.
    #[cfg(feature = "hub")]
    pub(crate) fn read_plans(&self) -> app::sync::ReadPlans {
        app::sync::ReadPlans::new(
            app::status::ReadStatus::new(
                Arc::clone(&self.config),
                Arc::clone(&self.store),
                Arc::new(SystemClock),
                domain::TimeZone::system(),
            ),
            Arc::clone(&self.schedules),
        )
    }

    /// The listings a paired machine's sync runs before each session, and
    /// what the daemon's listings showed of other agents' snapshots (§7.3,
    /// decision 156).
    #[cfg(feature = "hub")]
    pub(crate) fn listings_due(&self) -> Arc<app::others_snapshots::ListingsDue> {
        Arc::clone(&self.listings_due)
    }

    /// What the orderly stop freezes ([`Self::runs`]) of a daemon that
    /// leaves the queue's leftovers and the scheduler unstarted, as a
    /// headless or scratch session does: the backups queued before it stay
    /// in the log for the next `serve` (decision 73), so none waits in its
    /// queue, and no plan is refused a run by hand for one (decision 75,
    /// point 4).
    pub(crate) fn unstarted(self) -> runs::Runs {
        self.runner.queue.resumed();
        self.runs()
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
                Arc::clone(&self.schedules),
                Arc::clone(&events),
                runs.as_ref(),
            )
            .await;
            // Every backup queued before the start has its place again, or
            // has ended: a start by hand is refused only for a run waiting.
            queue.resumed();
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
pub(super) async fn resume_queued(
    queued: Vec<OperationRecord>,
    config: &dyn ConfigStore,
    store: Arc<dyn OperationStore>,
    schedules: Arc<dyn ScheduleStore>,
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
    // The bound a queued run waits for its keyring within is the next run
    // its plan's planner kept, handed to the run as the scheduler hands
    // one it starts its own bound (#1221). None kept, or none readable, is
    // no bound: such a run is refused at once, as it always was.
    let unlock_of = match schedules.planned_runs().await {
        Ok(planned) => planned
            .into_iter()
            .map(|run| {
                (
                    run.plan,
                    app::secrets::UnlockWait::new(Arc::new(SystemClock), run.at),
                )
            })
            .collect(),
        Err(error) => {
            log::warn!(
                "the planned runs can't be read, so a queued backup of a locked keyring is refused at once: {error}"
            );
            std::collections::BTreeMap::new()
        }
    };
    RecoverOperations::new(store)
        .with_events(events)
        .resume(queued, config.as_ref(), &unlock_of, runs, SystemClock.now())
        .await;
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
pub(super) async fn daemon_at(
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
    // Before anything maintains: a data folder copied from another computer
    // runs no forget, prune or adoption (§7.1, §16 decision 144).
    let this_machine: Arc<dyn app::ports::ThisMachine> =
        Arc::new(platform::this_machine::SystemMachine::in_data_dir(data_dir));
    let standing = Arc::new(app::machine::MachineStanding::new(machine_checked(
        this_machine.as_ref(),
    )));
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
    // A new machine ID gives up maintenance under the same lock, and not
    // while a maintenance operation the log holds still runs (I1).
    let machine_route = http_adapter::MachineRoute::new(Arc::new(app::machine::TakeNewId::new(
        this_machine,
        Arc::clone(&standing),
        Arc::clone(&config),
        Arc::clone(&store),
        Arc::clone(&writes),
    )));
    // What AI agents may do without a token: the configuration's
    // `[without-token]` table, changed under the same lock (§10.5).
    let without_token = Arc::new(app::without_token::WithoutToken::new(
        Arc::clone(&config),
        Arc::clone(&writes),
    ));
    let resolve = Arc::new(daemon_resolve(
        Arc::new(SystemSecrets::default()),
        Arc::new(SystemClock),
        started,
    ));
    // Every listing passes what it shows of other agents' snapshots on to
    // a paired machine's reports to its hub (§7.3, I1).
    let events_engine: Arc<dyn Engine> = Arc::new(EventEngine {
        inner: Arc::new(workers),
        bus: Arc::clone(&bus),
        clock: Arc::new(SystemClock),
    });
    #[cfg(feature = "hub")]
    let others = Arc::new(app::others_snapshots::OthersSnapshots::default());
    #[cfg(feature = "hub")]
    let events_engine: Arc<dyn Engine> = Arc::new(app::others_snapshots::NoteListings::new(
        events_engine,
        Arc::clone(&others),
        agent_id,
        Arc::new(SystemClock),
    ));
    // The choice of a location comes first (decision 80): a job opens the
    // location it chose, and the record of an opened repository's ID
    // (decision 81) sees that job, never a probe.
    let locations = Arc::new(ChooseLocation::new(
        Arc::new(RecordOpened::new(
            events_engine,
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
        machine: standing,
        leases: maintenance_leases(data_dir, &log.store),
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
    // Before each session with its hub, a paired machine lists each
    // repository it maintains that has no listing standing, in its turn
    // with the daemon's other listings (§7.3, decision 156).
    #[cfg(feature = "hub")]
    let listings_due = Arc::new(app::others_snapshots::ListingsDue::new(
        Arc::clone(&config),
        Arc::clone(&store),
        Arc::clone(&others),
        app::snapshots::ListSnapshots::new(
            Arc::clone(&engine),
            Arc::new(SystemClock),
            resolve.as_ref().clone(),
            Arc::clone(&listings),
        ),
        Arc::new(OsRandom),
        agent_id,
        Arc::new(SystemClock),
    ));
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
    // The daemon listens before the backups queued before the start are
    // handed back to the queue (decision 73): until each is, a start by
    // hand of its plan is refused as if it waited already, so a plan never
    // has two runs waiting (decision 75, point 4).
    runner.queue.resuming(
        recovered
            .queued
            .iter()
            .filter(|record| record.kind == domain::execution::OperationKind::Backup)
            .filter_map(|record| record.target.plan.clone()),
    );
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
        #[cfg(feature = "hub")]
        listings_due,
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
            without_token,
            full_disk_access: full_disk_access_route,
            machine: machine_route,
            sync: http_adapter::SyncHub::default(),
            #[cfg(feature = "hub")]
            pairing: Arc::new(app::pairing::PairingState::new(Arc::new(
                platform::identity::FileIdentity::in_data_dir(data_dir),
            ))),
        },
        background,
        stopping,
    ))
}

/// The leases a paired machine maintains under (§7.3, I1): its pin in
/// `data_dir` and its lease cache in `store`, which the sync with its hub
/// keeps (`hub::reporting`). An unpaired machine isn't held to them.
#[cfg(feature = "hub")]
fn maintenance_leases(
    data_dir: &std::path::Path,
    store: &Arc<SqliteStore>,
) -> Option<Arc<app::leases::MaintenanceLeases>> {
    Some(Arc::new(app::leases::MaintenanceLeases::new(
        Arc::new(platform::identity::FileIdentity::in_data_dir(data_dir)),
        Arc::<SqliteStore>::clone(store),
        Arc::new(SystemClock),
    )))
}

/// A build without the hub never pairs, so it holds no leases.
#[cfg(not(feature = "hub"))]
fn maintenance_leases(
    _data_dir: &std::path::Path,
    _store: &Arc<SqliteStore>,
) -> Option<Arc<app::leases::MaintenanceLeases>> {
    None
}

/// Where this machine stands, from the check at start of whether its data
/// folder was copied from another computer (§7.1, §16 decision 144), each
/// finding said in the log. A check that can't tell refuses nothing.
fn machine_checked(machine: &dyn app::ports::ThisMachine) -> app::machine::Standing {
    use app::machine::Found;
    let found = app::machine::check_at_start(machine);
    match &found {
        Found::Same => {}
        Found::Recorded => log::info!(
            "recorded this computer's machine identifier beside the machine ID, so a copy of this data folder on another computer can be told"
        ),
        Found::RecordFailed(error) => log::warn!(
            "this computer's machine identifier couldn't be recorded beside the machine ID, so a copy of this data folder on another computer can't be told yet; the next start tries again: {}",
            error.detail
        ),
        Found::Copied => log::warn!(
            "this data folder was copied from another computer: backups run as usual, and forget, prune and adoption are stopped so this computer can't remove the other's snapshots; if this is a new computer, take a new machine ID on the start page"
        ),
        Found::Unread(error) => log::warn!(
            "this computer's machine identifier couldn't be read, so a copy of this data folder from another computer can't be told, and maintenance runs as usual: {}",
            error.detail
        ),
    }
    found.standing()
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
pub(super) fn report_line(localizer: &l10n::Localizer, report: &Report) -> String {
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
pub(super) fn workers_cache(dir: Option<std::path::PathBuf>) -> Option<std::path::PathBuf> {
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
pub(super) fn log_recording(recording: Recording) {
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
        Err(ended) => log::warn!("{}", untidied_line(dir, &ended)),
    }
}

/// The log line for a tidying task that ended without a result: how it
/// ended, never a panic's message (I4).
fn untidied_line(dir: &std::path::Path, ended: &tokio::task::JoinError) -> String {
    let how = if ended.is_panic() {
        "panicked"
    } else {
        "was cancelled"
    };
    format!(
        "the cache folder {} was not tidied: the tidying {how}",
        dir.display()
    )
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

/// The refusal of a data folder another process holds, named and unnamed. On
/// Linux and macOS that process may be the installed service, which its
/// manager starts again once killed (#1035): the message names the manager's
/// stop and `remove-service`.
#[cfg(target_os = "linux")]
pub(super) const DATA_DIR_IN_USE: [MessageId; 2] = [
    msg::CLI_DATA_DIR_IN_USE_SYSTEMD,
    msg::CLI_DATA_DIR_IN_USE_UNNAMED_SYSTEMD,
];
#[cfg(target_os = "macos")]
pub(super) const DATA_DIR_IN_USE: [MessageId; 2] = [
    msg::CLI_DATA_DIR_IN_USE_LAUNCHD,
    msg::CLI_DATA_DIR_IN_USE_UNNAMED_LAUNCHD,
];
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub(super) const DATA_DIR_IN_USE: [MessageId; 2] =
    [msg::CLI_DATA_DIR_IN_USE, msg::CLI_DATA_DIR_IN_USE_UNNAMED];

/// A data folder another process holds, or a lock that couldn't be taken, as
/// the message for whoever started the daemon.
pub(super) fn lock_refused(data_dir: &std::path::Path, error: LockError) -> Refused {
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
pub(super) fn recovery_refused(
    database: &std::path::Path,
    error: &app::ports::StoreError,
) -> Refused {
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

/// What the choice of a location tells the daemon's log (decision 80): for
/// each job on a repository reached at several, which location it used and
/// why each earlier one was skipped, by position and outcome code, never an
/// address or a network's name.
pub(super) fn log_location(report: LocationReport) {
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

/// The daemon's resolution of secrets, through `secrets`: each warning it
/// gives is said in the log, and a run nobody waits on holds for the
/// session's keyring service while the daemon, which started at the
/// monotonic time `started` by `clock`, starts (§5, decision 116).
pub(super) fn daemon_resolve(
    secrets: Arc<dyn app::ports::SecretResolver>,
    clock: Arc<dyn Clock>,
    started: domain::SignedDuration,
) -> ResolveRepository {
    ResolveRepository::new(secrets)
        .telling(Arc::new(log_secret_warning))
        .waiting_for_keyring(app::secrets::KeyringWait::new(clock, started))
}

#[cfg(test)]
mod untidied_tests {
    use super::untidied_line;

    #[tokio::test]
    async fn a_panicked_tidying_is_logged_without_its_message() {
        let ended = tokio::task::spawn_blocking(|| -> () { panic!("canary-1384-secret") })
            .await
            .expect_err("the task panics");
        let line = untidied_line(std::path::Path::new("/cache"), &ended);
        assert!(!line.contains("canary-1384-secret"), "{line}");
        assert_eq!(
            line,
            "the cache folder /cache was not tidied: the tidying panicked"
        );
    }
}
