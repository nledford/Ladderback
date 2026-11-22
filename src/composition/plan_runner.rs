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
use app::secrets::{KeyringWaitsFor, ResolveRepository, UnlockWait};
use async_trait::async_trait;
use domain::execution::FailureReason;
use domain::execution::deadline::backup_budget;
use domain::execution::plan_tags::PlanTags;
use domain::execution::queue::RunWaiting;
use domain::execution::{Target, Trigger};
use domain::secrets::SecretWarning;
use domain::{AgentId, OperationId, PlanId};
use http_adapter::{MaintenanceRefusal, Operations, SecretRefusal, StartError};
use platform::SystemClock;
use secrets::SystemSecrets;
use uuid::Uuid;

use super::key_files::KeyFilesLeftOut;
use super::{
    CancelRegistry, maintenance, refusal_redactor, refusal_with, runs, summary, warning_code,
};
use app::ports::config_unusable;

/// Starts the operations the API asks for: a backup of a named plan, run to
/// its end on a task of its own. What happens to the run arrives over the
/// events WebSocket, so the route answers with the operation's ID alone.
#[derive(Clone)]
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
        unlock: Option<UnlockWait>,
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
            let waiting = |source: &domain::secrets::SourceName, why: KeyringWaitsFor| {
                log_keyring_wait(trigger, id, source, why);
            };
            // A run that waits for its keyring to be unlocked waits by the
            // plan's next due run, which the caller arms for that run
            // alone (#1221).
            match unlock {
                Some(wait) => {
                    (*self.resolve)
                        .clone()
                        .waiting_for_unlock(wait)
                        .resolve_saved_unattended(&plan.repository, repository, &waiting)
                        .await
                }
                None => {
                    self.resolve
                        .resolve_saved_unattended(&plan.repository, repository, &waiting)
                        .await
                }
            }
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
                    waits_for_unlock: error.waits_for_unlock(),
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

    /// Starts a backup of `plan` a person or another program asked for,
    /// taking its own place in the queue: it never waits for the keyring,
    /// so its answer is the caller's at once.
    async fn start_plan(&self, id: &PlanId, trigger: Trigger) -> Result<OperationId, Unbuilt> {
        let config = self.config.load().await.map_err(|error| {
            Unbuilt::Refused(config_unusable(error, |code, args, detail| {
                StartError::Unavailable { code, args, detail }
            }))
        })?;
        let place = self
            .queue
            .by_hand(id, config.agent().concurrency)
            .map_err(|RunWaiting| Unbuilt::Refused(StartError::RunWaiting))?;
        let built = self.backup_of(&config, id, &trigger, None).await?;
        Ok(self.run_built(built, trigger, place).await)
    }

    /// Records the run of `built` as a new operation, on a task of its own
    /// under the freeze of the daemon's stop, and returns once the
    /// operation log holds it queued (decision 73).
    async fn run_built(&self, built: BuiltBackup, trigger: Trigger, place: Place) -> OperationId {
        let BuiltBackup {
            job,
            setup,
            warnings,
        } = built;
        // A UUIDv7: the log orders by ID, so the newest run is the highest.
        let operation = OperationId::from_uuid(Uuid::now_v7());
        let cancel = self.cancel_switch(operation);
        let use_case = self.backup_use_case(setup);
        // Recorded as queued before the answer is spent, so a cancel that
        // arrives the instant it is finds the operation in the log. The
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
        operation
    }

    /// The configuration a run of `plan` starts under, or `None`, with the
    /// refusal said in the daemon's log, when it can't be read.
    async fn config_for_a_run(
        &self,
        plan: &PlanId,
        trigger: &Trigger,
    ) -> Option<domain::planning::AgentConfig> {
        match self.config.load().await {
            Ok(config) => Some(config),
            Err(error) => {
                log_not_started(
                    plan,
                    trigger,
                    &config_unusable(error, |code, args, detail| StartError::Unavailable {
                        code,
                        args,
                        detail,
                    }),
                );
                None
            }
        }
    }

    /// A run parked waiting for its keyring to be unlocked, on a task of
    /// its own so the schedule and the queue went on: it resolves again
    /// with that wait armed, is built again from the configuration as it
    /// stands once the keyring is unlocked, runs in a place it takes then,
    /// or is refused as any run of the schedule is (#1221). `refused` is
    /// the refusal that parked it, and `refused_on` the repository it was
    /// refused for, as the configuration it started under named it: a plan
    /// that backs up to another by now doesn't start, and none of its
    /// secrets is resolved to find out (#1247 is whether it should run on
    /// the new one). A plan whose next run is due by the time the parked
    /// task looks stays refused as it was: no read taken after that bound
    /// lets the run go on.
    async fn start_parked(
        &self,
        plan: PlanId,
        trigger: Trigger,
        unlock: UnlockWait,
        refused: SecretUnbuilt,
        refused_on: Option<PlanRepository>,
    ) {
        let Some(config) = self.config_for_a_run(&plan, &trigger).await else {
            return;
        };
        let on = PlanRepository::of(&config, &plan);
        if let (Some(now), Some(then)) = (&on, &refused_on)
            && now.is_other_than(then)
        {
            log_moved(&plan, &trigger);
            return;
        }
        if unlock.has_passed() {
            self.refused_for_secret(&plan, &trigger, refused).await;
            return;
        }
        // The place it waited in was given up, and nothing is taken while
        // it waits: one of its own may have run meanwhile.
        let rebuilt = match self.backup_of(&config, &plan, &trigger, Some(unlock)).await {
            Ok(waited) => {
                self.rebuilt_after_wait(&config, &waited, &plan, &trigger)
                    .await
            }
            Err(unbuilt) => Err(unbuilt),
        };
        match rebuilt {
            Ok(Some((built, limit))) => {
                // A newer run of the plan may be going — running, or
                // waiting for its turn — since this run waited with no
                // place: it gives way to it.
                match self.queue.unless_going(&plan, limit) {
                    Ok(place) => {
                        self.run_built(built, trigger, place).await;
                    }
                    Err(RunWaiting) => log_not_started(&plan, &trigger, &StartError::RunWaiting),
                }
            }
            Ok(None) => log_moved(&plan, &trigger),
            Err(Unbuilt::Secret(secret)) => {
                self.refused_for_secret(&plan, &trigger, *secret).await;
            }
            Err(Unbuilt::Refused(error)) => log_not_started(&plan, &trigger, &error),
        }
    }

    /// The backup of `plan` built again once its wait for the keyring
    /// ended unlocked, from the configuration as it stands now, with the
    /// queue's limit it says: the wait can last hours, and the plan may
    /// have been deleted, changed or moved meanwhile (#1221). `waited` is
    /// the build the wait ended with, from `waited_config`. The keyring
    /// answers at once now, so nothing waits again. `None` when the plan
    /// now backs up to another repository than the one whose keyring it
    /// waited for: that is told before the build, so the other
    /// repository's secrets aren't resolved for a run that won't run.
    async fn rebuilt_after_wait(
        &self,
        waited_config: &domain::planning::AgentConfig,
        waited: &BuiltBackup,
        plan: &PlanId,
        trigger: &Trigger,
    ) -> Result<Option<(BuiltBackup, domain::planning::Concurrency)>, Unbuilt> {
        let fresh = self.config.load().await.map_err(|error| {
            Unbuilt::Refused(config_unusable(error, |code, args, detail| {
                StartError::Unavailable { code, args, detail }
            }))
        })?;
        // The recorded IDs say whether the repository behind the plan's
        // alias is another: an alias can stay while what it names changes.
        // A deleted plan is left to the build, which refuses it.
        if let Some(now) = PlanRepository::of(&fresh, plan)
            && PlanRepository::of(waited_config, plan)
                .as_ref()
                .is_none_or(|then| now.is_other_than(then))
        {
            return Ok(None);
        }
        let built = self.backup_of(&fresh, plan, trigger, None).await?;
        let same_repository = built.job.target().repository == waited.job.target().repository;
        Ok(same_repository.then(|| (built, fresh.agent().concurrency)))
    }

    /// A queued backup parked waiting for its keyring to be unlocked, on a
    /// task of its own so the queue's other leftovers went on: it resolves
    /// again with that wait armed — its operation shows as queued until
    /// the keyring is — is built again from the configuration as it
    /// stands then, and runs in a place it takes once it is, giving way to
    /// a newer run of its plan that is going, as the schedule's parked run
    /// does. Whatever else it answers, or a keyring that never unlocks by
    /// the plan's next kept run, ends it as the queued run it is (#1221).
    /// `cancel` is its cancel switch, parked before it waits: a cancel
    /// while it waits ends it cancelled at once, as a queued backup
    /// waiting for its turn is (§5, decision 75). `refused` is the refusal
    /// that parked it: a plan whose next run is due by the time the
    /// parked task looks stays refused as it was.
    async fn resume_parked(
        &self,
        operation: domain::execution::Operation,
        plan: PlanId,
        trigger: Trigger,
        unlock: UnlockWait,
        refused: SecretUnbuilt,
        cancel: impl Future<Output = ()> + Send + 'static,
    ) {
        let id = operation.id();
        let mut cancel = Box::pin(cancel);
        let waited = tokio::select! {
            biased;
            () = &mut cancel => None,
            waited = self.resumed_after_wait(&operation, &plan, &trigger, unlock, refused) => Some(waited),
        };
        let Some(waited) = waited else {
            self.registry.finish(&id);
            let finished = RunOperation::new(
                Arc::clone(&self.engine),
                Arc::new(SystemClock),
                Arc::clone(&self.store),
            )
            .with_events(Arc::clone(&self.events))
            .cancel_resumed(operation)
            .await;
            log_ended(&finished);
            say_how_it_ended(&self.ends, &finished);
            return;
        };
        match waited {
            Waited::Runs(built, limit) => {
                // A newer run of the plan may be going — running, or
                // waiting for its turn — since this run waited with no
                // place: it gives way to it.
                let Ok(place) = self.queue.unless_going(&plan, limit) else {
                    self.registry.finish(&id);
                    self.end_unrunnable(
                        operation,
                        FailureReason::Superseded,
                        "a newer run of its plan is going",
                    )
                    .await;
                    return;
                };
                self.spawn_resumed(operation, *built, place, cancel);
            }
            Waited::Refused(secret) => {
                self.registry.finish(&id);
                self.refuse_resumed(operation, secret).await;
            }
            Waited::Unrunnable(why) => {
                self.registry.finish(&id);
                self.end_unrunnable(operation, FailureReason::DaemonStopped, &why)
                    .await;
            }
        }
    }

    /// How the wait of `operation`, a queued backup of `plan`, for its
    /// keyring ended, as [`Self::resume_parked`] acts on it.
    async fn resumed_after_wait(
        &self,
        operation: &domain::execution::Operation,
        plan: &PlanId,
        trigger: &Trigger,
        unlock: UnlockWait,
        refused: SecretUnbuilt,
    ) -> Waited {
        let config = match self.config.load().await {
            Ok(config) => config,
            Err(error) => {
                return Waited::Unrunnable(start_refused(&config_unusable(
                    error,
                    |code, args, detail| StartError::Unavailable { code, args, detail },
                )));
            }
        };
        // A plan that backs up to another repository than the one the
        // backup was queued on isn't run there under a record of the old,
        // and none of its secrets is resolved to find that out.
        if let Some(now) = PlanRepository::of(&config, plan)
            && !operation.target().is_on(now.id.as_ref())
        {
            return Waited::Unrunnable("its plan now backs up to another repository".to_owned());
        }
        if unlock.has_passed() {
            return Waited::Refused(refused);
        }
        let rebuilt = match self.backup_of(&config, plan, trigger, Some(unlock)).await {
            Ok(waited) => {
                self.rebuilt_after_wait(&config, &waited, plan, trigger)
                    .await
            }
            Err(unbuilt) => Err(unbuilt),
        };
        match rebuilt {
            Ok(Some((built, limit)))
                if operation
                    .target()
                    .is_on(built.job.target().repository.as_ref()) =>
            {
                Waited::Runs(Box::new(built), limit)
            }
            // The keyring never unlocked within the bound, or another
            // secret of the repository is refused: the queued backup ends
            // failed with the refusal, as a resumed one always did (§5,
            // #490), so retries follow it.
            Err(Unbuilt::Secret(secret))
                if operation.target().is_on(secret.target.repository.as_ref()) =>
            {
                Waited::Refused(*secret)
            }
            // The plan moved to another repository while the run waited:
            // it isn't run there under a record of the old.
            Ok(_) | Err(Unbuilt::Secret(_)) => {
                Waited::Unrunnable("its plan now backs up to another repository".to_owned())
            }
            Err(Unbuilt::Refused(error)) => Waited::Unrunnable(start_refused(&error)),
        }
    }

    /// Runs a queued backup in `place`, on a task of its own under the
    /// freeze: oldest first, as the queue hands them on, each waits for its
    /// turn like any other backup (§5). `cancel` is its cancel switch,
    /// parked already ([`Self::cancel_switch`]).
    fn spawn_resumed(
        &self,
        operation: domain::execution::Operation,
        built: BuiltBackup,
        place: Place,
        cancel: impl Future<Output = ()> + Send + 'static,
    ) {
        let id = operation.id();
        let BuiltBackup {
            job,
            setup,
            warnings,
        } = built;
        let use_case = self.backup_use_case(setup);
        let registry = Arc::clone(&self.registry);
        let ends = self.ends.clone();
        tokio::spawn(self.runs.until_frozen(async move {
            let queued = app::execution::Queued::resumed(operation, job).with_warnings(warnings);
            let finished = use_case.run_in_turn(queued, place, cancel).await;
            registry.finish(&id);
            log_ended(&finished);
            say_how_it_ended(&ends, &finished);
        }));
    }

    /// Ends a queued backup that can't run again as failed with `reason`,
    /// with `why` in the log: `daemon-stopped`, as decision 40 ended every
    /// queued operation, or `superseded` when it gave way to a newer run of
    /// its plan (decision 135). Left queued, it would show as queued with
    /// nothing to run it until the next start (decision 73). An end the log
    /// takes is announced, as every end a start makes is (decision 91).
    async fn end_unrunnable(
        &self,
        mut operation: domain::execution::Operation,
        reason: FailureReason,
        why: &str,
    ) {
        let hyphenated = operation.id().as_uuid().hyphenated();
        log::warn!("the queued backup {hyphenated} can't run again, and ends as failed: {why}");
        if operation.fail(reason, SystemClock.now()).is_err() {
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

    /// Ends `operation`, a queued backup refused for a secret of the
    /// repository its plan still backs up to, failed with the refusal, as
    /// a refused scheduled run does (§5, #490), and is heard as any run's
    /// end, so retries follow it.
    async fn refuse_resumed(&self, operation: domain::execution::Operation, secret: SecretUnbuilt) {
        let SecretUnbuilt {
            refusal, failure, ..
        } = secret;
        log::warn!(
            "the queued backup {} can't run again, and ends as failed: {}",
            operation.id().as_uuid().hyphenated(),
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
    }

    /// Records a run refused for a secret of its repository as a failed
    /// backup that never started, and says how it ended, so the history and
    /// the page say why the schedule's backups stopped (§5, decision 76);
    /// the log keeps its own words, never the refusal's, which only the
    /// history shows, masked (I4, #483). Its place is given up already.
    async fn refused_for_secret(&self, plan: &PlanId, trigger: &Trigger, secret: SecretUnbuilt) {
        let SecretUnbuilt {
            refusal,
            failure,
            target,
            ..
        } = secret;
        let operation = OperationId::from_uuid(Uuid::now_v7());
        log::warn!(
            "{} of plan {} didn't start, and ends as failed operation {}: {}",
            run_named(trigger),
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
        .refused(operation, trigger.clone(), target, failure)
        .await;
        log_recording_errors(&finished);
        say_how_it_ended(&self.ends, &finished);
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

/// How a queued backup's wait for its keyring ended, before anything is
/// recorded of it ([`PlanRunner::resume_parked`]).
enum Waited {
    /// It runs, built from the configuration as it stands now, under the
    /// queue's limit it says. Boxed: a built backup is large beside the
    /// other answers.
    Runs(Box<BuiltBackup>, domain::planning::Concurrency),
    /// Refused for a secret of the repository it was queued on: it ends
    /// failed with that refusal.
    Refused(SecretUnbuilt),
    /// It can't run again, for this reason.
    Unrunnable(String),
}

/// The repository a plan backs up to, as a configuration names it: the
/// alias, and the ID its entry records once the daemon opened it.
struct PlanRepository {
    alias: domain::RepositoryAlias,
    id: Option<domain::RepositoryId>,
}

impl PlanRepository {
    /// The repository `plan` backs up to in `config`; `None` when the
    /// plan, or the repository it names, isn't there.
    fn of(config: &domain::planning::AgentConfig, plan: &PlanId) -> Option<Self> {
        let alias = config.plan(plan)?.repository.clone();
        let id = config.repository(&alias)?.id.clone();
        Some(Self { alias, id })
    }

    /// Whether this is another repository than `then` was. Entries that
    /// both record an ID are told apart by it, since an alias can stay
    /// while the repository behind it changes, and change while it
    /// doesn't; otherwise the alias is all there is to go by.
    fn is_other_than(&self, then: &Self) -> bool {
        match (&self.id, &then.id) {
            (Some(now), Some(then)) => now != then,
            _ => self.alias != then.alias,
        }
    }
}

/// Logs that plan `plan`'s parked run didn't start: the plan backs up to
/// another repository than the one whose keyring it waited for.
fn log_moved(plan: &PlanId, trigger: &Trigger) {
    log::warn!(
        "{} of plan {} didn't start: the plan now backs up to another repository than \
         the one whose keyring it waited for",
        run_named(trigger),
        plan.as_str()
    );
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
    /// Whether the repository's password is in a keyring the service
    /// answered is locked: a run the schedule starts waits for it to be
    /// unlocked, past this refusal (#1221). One whose unlock prompt was
    /// dismissed is refused as locked too, but never waits (#993).
    waits_for_unlock: bool,
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
        StartError::RunWaiting => format!(
            "a run of it is waiting for its turn already ({})",
            wire_name(ErrorCode::PlanRunWaiting)
        ),
    }
}

#[async_trait]
impl PlanRuns for PlanRunner {
    async fn start(
        &self,
        plan: &PlanId,
        trigger: Trigger,
        place: Place,
        unlock: Option<UnlockWait>,
    ) {
        let Some(config) = self.config_for_a_run(plan, &trigger).await else {
            return;
        };
        // A first resolution without the unlock wait answers at once, so
        // the schedule goes on with the other plans' runs; only a locked
        // keyring with the plan's next run to wait within parks the run on
        // a task (#1221).
        match self.backup_of(&config, plan, &trigger, None).await {
            Ok(built) => {
                self.run_built(built, trigger, place).await;
            }
            Err(Unbuilt::Secret(secret)) => match (unlock, secret.waits_for_unlock) {
                (Some(unlock), true) if trigger.is_unattended() => {
                    // The wait can outlast the scheduler's turn and the
                    // run's place in the queue: both go on without it, and
                    // the run takes a new place once the keyring is
                    // unlocked. The daemon's stop ends the wait with the
                    // run, never started.
                    drop(place);
                    let runner = self.clone();
                    let plan = plan.clone();
                    let refused_on = PlanRepository::of(&config, &plan);
                    let frozen = self.runs.clone();
                    tokio::spawn(frozen.until_frozen(async move {
                        runner
                            .start_parked(plan, trigger, unlock, *secret, refused_on)
                            .await;
                    }));
                }
                _ => {
                    // The place is given up before the failed operation is
                    // recorded, as every other refusal's is (§5).
                    drop(place);
                    self.refused_for_secret(plan, &trigger, *secret).await;
                }
            },
            Err(Unbuilt::Refused(error)) => log_not_started(plan, &trigger, &error),
        }
    }

    async fn resume(&self, operation: domain::execution::Operation, unlock: Option<UnlockWait>) {
        let trigger = operation.trigger().clone();
        let built = match operation.target().plan.clone() {
            Some(plan) => match self.config.load().await {
                Ok(config) => self
                    .backup_of(&config, &plan, &trigger, None)
                    .await
                    .map(|built| (built, plan, config.agent().concurrency)),
                Err(error) => Err(Unbuilt::Refused(config_unusable(
                    error,
                    |code, args, detail| StartError::Unavailable { code, args, detail },
                ))),
            },
            None => Err(Unbuilt::Refused(StartError::UnknownPlan)),
        };
        // A queued run the schedule started or caught up on, whose
        // repository password is in a locked keyring with the plan's next
        // kept run to wait within, parks on a task of its own when its plan
        // still backs up to the repository it was recorded on — its
        // operation shows as queued while it waits — so the queue's other
        // leftovers go on (#1221). Its cancel switch is parked first, so a
        // cancel of what shows as queued finds it while it waits (§4.2). A
        // retry is answered at once, as ever: its next try is planned when
        // it ends.
        let built = match (built, operation.target().plan.clone(), unlock) {
            (Err(Unbuilt::Secret(secret)), Some(plan), Some(unlock))
                if matches!(trigger, Trigger::Schedule | Trigger::CatchUp)
                    && secret.waits_for_unlock
                    && operation.target().is_on(secret.target.repository.as_ref()) =>
            {
                let cancel = self.cancel_switch(operation.id());
                let runner = self.clone();
                let frozen = self.runs.clone();
                tokio::spawn(frozen.until_frozen(async move {
                    runner
                        .resume_parked(operation, plan, trigger, unlock, *secret, cancel)
                        .await;
                }));
                return;
            }
            (built, _, _) => built,
        };
        // Refused for a secret of the repository its plan still backs up to,
        // it ends failed with the refusal, as a refused scheduled run does
        // (§5, #490), and is heard as any run's end, so retries follow it.
        let built = match built {
            Err(Unbuilt::Secret(secret))
                if operation.target().is_on(secret.target.repository.as_ref()) =>
            {
                self.refuse_resumed(operation, *secret).await;
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
            Ok((built, plan, limit)) => {
                let place = self.queue.push(&plan, limit);
                let cancel = self.cancel_switch(operation.id());
                self.spawn_resumed(operation, built, place, cancel);
            }
            // The queue can't run it again, as decision 40 ended every
            // queued operation: its plan or repository is gone or changed, its
            // plan moved and the secret is refused, or the configuration can't be read. Left
            // queued, it would show as queued with nothing to run it until
            // the next start (decision 73). An end the log took is announced,
            // as every end a start makes is (decision 91).
            Err(why) => {
                self.end_unrunnable(operation, FailureReason::DaemonStopped, &why)
                    .await;
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
        // too, not only the answer the page shows for a moment (#483). A run
        // of the plan waiting already leaves none: the answer says so, and a
        // client that retries in a loop can't fill the log (decision 75,
        // point 4), as a cancel of an operation that isn't running can't.
        let operation = self
            .start_plan(&id, trigger.clone())
            .await
            .map_err(Unbuilt::into_start_error)
            .inspect_err(|error| {
                if !matches!(error, StartError::RunWaiting) {
                    log_not_started(&id, &trigger, error);
                }
            })?;
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
/// keyring to read the entry `source` names — for the session's service to
/// come, or for the keyring to be unlocked — and how long it may: the
/// entry, never a secret (I4).
fn log_keyring_wait(
    trigger: &Trigger,
    plan: &PlanId,
    source: &domain::secrets::SourceName,
    why: KeyringWaitsFor,
) {
    use domain::secrets::SourceName;
    let name = match source {
        SourceName::Keyring { entry } => entry,
        SourceName::Command { program } => program,
        SourceName::File { path } => path,
        SourceName::Env { name } => name,
    };
    match why {
        KeyringWaitsFor::Service => log::info!(
            "{} of plan {} waits for the session's keyring service to read {name}, \
             until it answers or the daemon has been up for {:#}",
            run_named(trigger),
            plan.as_str(),
            app::secrets::KEYRING_WAIT
        ),
        KeyringWaitsFor::Unlock => log::info!(
            "{} of plan {} waits for the keyring to be unlocked to read {name}, \
             until it is or the plan's next run is due",
            run_named(trigger),
            plan.as_str()
        ),
    }
}
