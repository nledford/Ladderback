//! The plan runner (technical design §4.2, §5): what starts a plan's backup,
//! on a schedule or by request, and runs it through the queue, and the
//! operations the API lists and cancels.

use std::sync::Arc;

use api::{ErrorCode, MsgArg};
use app::conditions::NetworkWait;
use app::execution::{Finished, RunOperation};
use app::full_disk_access::FullDiskAccessCheck;
use app::history::{GetOperation, ListOperations};
use app::locations::ChooseLocation;
use app::ports::Retention;
use app::ports::{
    BackupJob, Clock, ConfigStore, Engine, EventBus, HookRunner, HookSetup, Job, OperationStore,
};
use app::queue::{Place, RunQueue};
use app::repositories::ConfigWrites;
use app::schedule::{PlanRuns, ScheduleReport};
use app::secrets::ResolveRepository;
use async_trait::async_trait;
use domain::execution::FailureReason;
use domain::execution::deadline::backup_budget;
use domain::execution::plan_tags::PlanTags;
use domain::execution::{Target, Trigger};
use domain::secrets::SecretWarning;
use domain::{AgentId, OperationId, PlanId};
use http_adapter::{MaintenanceRefusal, Operations, SecretRefusal, StartError};
use platform::SystemClock;
use secrets::SystemSecrets;
use uuid::Uuid;

use super::key_files::KeyFilesLeftOut;
use super::{
    CancelRegistry, config_unusable, maintenance, refusal_redactor, refusal_with, runs, summary,
    warning_code,
};

/// Starts the operations the API asks for: a backup of a named plan, run to
/// its end on a task of its own. What happens to the run arrives over the
/// events WebSocket, so the route answers with the operation's ID alone.
pub(super) struct PlanRunner {
    pub(super) config: Arc<dyn ConfigStore>,
    /// The one lock of every write of the configuration, which a
    /// maintenance start holds while it reads again that this agent
    /// maintains the repository (I1).
    pub(super) writes: Arc<ConfigWrites>,
    pub(super) resolve: Arc<ResolveRepository>,
    pub(super) agent_id: AgentId,
    pub(super) engine: Arc<dyn Engine>,
    /// Chooses the location retention opens in-process for a repository
    /// reached at several (decision 80); `engine` makes the same choice for
    /// every job. Without it, such a repository's retention fails.
    pub(super) locations: Option<Arc<ChooseLocation>>,
    pub(super) store: Arc<dyn OperationStore>,
    /// Evaluates each plan's retention for a forget run (§7.3).
    pub(super) retention: Arc<dyn Retention>,
    /// The running operations' cancel switches, shared with the routes.
    pub(super) registry: Arc<CancelRegistry>,
    /// Runs the agent, repository and plan hooks around every run (§6.1).
    pub(super) hooks: Arc<dyn HookRunner>,
    /// Where each run announces how its operation ended, once the log has the
    /// end (§4.4).
    pub(super) events: Arc<dyn EventBus>,
    /// The agent's one queue of backups, which the scheduler shares: every
    /// backup takes its turn in it (§5, the concurrency limit).
    pub(super) queue: Arc<RunQueue>,
    /// Where each backup's run says how it ended, so the scheduler can
    /// retry a failed one (§5).
    pub(super) ends: app::retry::RunEndSender,
    /// What the daemon's orderly stop freezes: every run starts under it
    /// (decision 92).
    pub(super) runs: runs::Runs,
    /// Where each reading of a repository's stats is kept (§16, decision
    /// 104): read on request, and after each check.
    pub(super) readings: Arc<dyn app::ports::StatsStore>,
    /// The agent's Full Disk Access check, run before each backup on macOS
    /// (§16, decision 115); none elsewhere.
    pub(super) full_disk_access: Option<Arc<FullDiskAccessCheck>>,
    /// What keeps the secret key files in the data folder out of every
    /// backup (§16, decisions 120 and 122).
    pub(super) left_out: KeyFilesLeftOut,
}

impl PlanRunner {
    /// The backup job of plan `id`, the hooks around it and what reading its
    /// secrets told, as `config` stands now: a rewrite between runs is
    /// picked up. A run `trigger` started with nobody waiting on it holds
    /// for the session's keyring service while the daemon starts, and the
    /// log says so once (§5).
    pub(super) async fn backup_of(
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
pub(super) struct BuiltBackup {
    pub(super) job: Job,
    setup: HookSetup,
    /// What reading the repository's secrets told (#386), which the run
    /// carries from its start.
    warnings: Vec<domain::execution::Warning>,
}

/// Why a plan's backup couldn't be built, so its run doesn't start.
pub(super) enum Unbuilt {
    /// Refused as a request to start it is, and nothing is recorded.
    Refused(StartError),
    /// A setting of the plan's repository couldn't be resolved.
    Secret(Box<SecretUnbuilt>),
}

/// A plan's backup unbuilt for a setting of its repository: `refusal` is
/// what a request to start it is answered with, and `failure` and `target`
/// what a run the schedule started records (§5).
pub(super) struct SecretUnbuilt {
    refusal: SecretRefusal,
    failure: FailureReason,
    target: Target,
}

impl Unbuilt {
    /// The refusal of a request to start the run.
    pub(super) fn into_start_error(self) -> StartError {
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
pub(super) fn run_named(trigger: &Trigger) -> std::borrow::Cow<'static, str> {
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
pub(super) fn wire_name(code: api::ErrorCode) -> String {
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
        StartError::RunWaiting => todo!(),
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
pub(super) fn log_ended(finished: &Finished) {
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
pub(super) fn operation_of(text: &str) -> Option<OperationId> {
    text.parse::<Uuid>().ok().map(OperationId::from_uuid)
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
