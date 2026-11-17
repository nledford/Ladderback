//! The restore use case (technical design §10.4): one entry of a snapshot
//! restored into a new folder under the agent's restore folder, or in place
//! after a dry run said what it would change. Every restore is confirmed by
//! step-up first, runs as an operation with a backup's deadline, one at a
//! time on the agent, and runs no hooks, as a manual maintenance run doesn't.

use std::net::IpAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use domain::access::step_up::{StepUpConfirmation, StepUpFor};
use domain::browse::FolderPath;
use domain::execution::Trigger;
use domain::execution::deadline::backup_budget;
use domain::planning::{AgentConfig, RepositoryConfig, RestoreFolder};
use domain::restore::{
    Destination, RestoreOrigin, RestorePolicy, RestoreRefused, RestoreRequest, restore_folder_name,
};
use domain::secrets::SecretWarning;
use domain::{OperationId, RepositoryAlias, SnapshotId, TimeZone};

use crate::access::{Access, StepUpRefused, confirm_step_up};
use crate::execution::{Finished, Queued, RunOperation};
use crate::maintenance::{MaintenanceError, run_read};
use crate::ports::{
    Clock, ConfigError, ConfigStore, Engine, EngineResult, Job, RepositoryAccess, RestoreJob,
    RestorePlanSummary, RestoreTarget,
};
use crate::secrets::{ResolveError, ResolveRepository, ResolvedRepository};

/// What a restore needs that the configuration doesn't say.
#[derive(Clone, Debug)]
pub struct RestoreSetup {
    /// The restore folder when `[agent] restore-folder` is unset:
    /// `ladderback-restore` in the home folder, none on a machine without
    /// one, where a restore into a new folder is refused.
    pub default_folder: Option<RestoreFolder>,
    /// The machine's time zone, in which a new folder's name gives the time.
    pub zone: TimeZone,
}

/// One restore a person asked for, as it reached the agent.
#[derive(Debug)]
pub struct RestoreAsk {
    pub alias: RepositoryAlias,
    pub snapshot: SnapshotId,
    /// The entry, by its keys from the snapshot's top.
    pub entry: FolderPath,
    pub destination: Destination,
    pub origin: RestoreOrigin,
    /// The address the request came from, whose sign-in waits a wrong
    /// step-up password counts against.
    pub address: IpAddr,
    /// What the person typed to confirm: the admin password with sign-in
    /// on, the snapshot's short ID otherwise. Never shown (I4).
    pub confirmation: StepUpConfirmation,
}

/// Why a restore didn't start. A typed code, never a sentence (§8.4).
#[derive(Debug)]
pub enum RestoreError {
    /// The step-up confirmation wasn't the admin password, or the
    /// snapshot's ID.
    StepUpFailed,
    /// The address's wait after failed sign-ins has `seconds` left; the
    /// password wasn't checked.
    StepUpWait { seconds: u64 },
    /// The admin password couldn't be checked: the platform's own text.
    StepUpUnavailable(String),
    /// The agent's rules don't take this restore.
    Refused(RestoreRefused),
    /// A restore runs on the agent already: this one.
    AlreadyRunning(OperationId),
    /// No repository is configured under the alias.
    UnknownRepository,
    /// The repository holds no snapshot by that ID.
    SnapshotUnknown,
    /// The configuration couldn't be read.
    Config(Box<ConfigError>),
    /// The repository's secrets couldn't be resolved.
    Resolve(ResolveError),
    /// Reading the repository failed, or wasn't answered in time.
    Read(MaintenanceError),
}

fn config_error(error: ConfigError) -> RestoreError {
    RestoreError::Config(Box::new(error))
}

/// A restore that has been confirmed, recorded as queued and holds the
/// agent's one restore slot until it is run and ends, or is dropped.
#[must_use = "a prepared restore that is never run stays queued in the log"]
pub struct PreparedRestore {
    queued: Queued,
    operation: OperationId,
    folder: Option<String>,
    slot: Slot,
}

impl PreparedRestore {
    /// The operation the restore runs as.
    pub fn operation(&self) -> OperationId {
        self.operation
    }

    /// The new folder the restore writes into, so the person can be told
    /// where its files are; none when it restores over the original paths.
    pub fn folder(&self) -> Option<&str> {
        self.folder.as_deref()
    }
}

/// The agent's one restore slot, held by the restore that runs: given up
/// when it is dropped.
struct Slot {
    running: Arc<Mutex<Option<OperationId>>>,
}

impl Drop for Slot {
    fn drop(&mut self) {
        *lock(&self.running) = None;
    }
}

fn lock(running: &Mutex<Option<OperationId>>) -> MutexGuard<'_, Option<OperationId>> {
    running.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Restores entries of snapshots.
pub struct Restores {
    engine: Arc<dyn Engine>,
    clock: Arc<dyn Clock>,
    config: Arc<dyn ConfigStore>,
    resolve: ResolveRepository,
    run: RunOperation,
    setup: RestoreSetup,
    access: Option<Arc<Access>>,
    running: Arc<Mutex<Option<OperationId>>>,
}

impl Restores {
    /// Restores that run through `run`, which carries no hooks.
    pub fn new(
        engine: Arc<dyn Engine>,
        clock: Arc<dyn Clock>,
        config: Arc<dyn ConfigStore>,
        resolve: ResolveRepository,
        run: RunOperation,
        setup: RestoreSetup,
    ) -> Self {
        Self {
            engine,
            clock,
            config,
            resolve,
            run,
            setup,
            access: None,
            running: Arc::new(Mutex::new(None)),
        }
    }

    /// With sign-in on: step-up asks for the admin password, checked by
    /// `access` in the client's sign-in waits.
    pub fn with_access(self, access: Arc<Access>) -> Self {
        Self {
            access: Some(access),
            ..self
        }
    }

    /// What restoring `entry` of `snapshot` in place would do, from a dry
    /// run that writes nothing: a read, never recorded as an operation.
    pub async fn preview_in_place(
        &self,
        op: OperationId,
        alias: &RepositoryAlias,
        snapshot: SnapshotId,
        entry: FolderPath,
        origin: RestoreOrigin,
    ) -> Result<RestorePlanSummary, RestoreError> {
        let config = self.config.load().await.map_err(config_error)?;
        let request = self.admit(&config, snapshot, entry, Destination::InPlace, origin)?;
        let repository = repository(&config, alias)?;
        let ResolvedRepository { access, .. } = self.resolved(alias, repository).await?;
        let job = Job::RestorePlan(RestoreJob {
            repository: access,
            snapshot: request.snapshot().clone(),
            entry: request.entry().clone(),
            target: RestoreTarget::InPlace,
            deadline_budget: backup_budget(None, None),
        });
        match run_read(self.engine.as_ref(), self.clock.as_ref(), op, job).await {
            Ok(EngineResult::RestorePlan(plan)) => Ok(plan),
            Ok(_) => Err(RestoreError::Read(MaintenanceError::Failed(
                domain::execution::FailureReason::EngineExited { exit_code: None },
            ))),
            Err(error) => Err(RestoreError::Read(error)),
        }
    }

    /// Confirms `ask` by step-up, admits it by the agent's rules, takes the
    /// agent's restore slot and records the restore as queued: nothing is
    /// recorded and the engine isn't asked when any of them refuses.
    pub async fn start(
        &self,
        op: OperationId,
        ask: RestoreAsk,
    ) -> Result<PreparedRestore, RestoreError> {
        self.step_up(&ask).await?;
        let config = self.config.load().await.map_err(config_error)?;
        let request = self.admit(
            &config,
            ask.snapshot,
            ask.entry,
            ask.destination,
            ask.origin,
        )?;
        let slot = self.take_slot(op)?;
        let repository = repository(&config, &ask.alias)?;
        let ResolvedRepository { access, warnings } = self.resolved(&ask.alias, repository).await?;
        let target = match request.folder() {
            Some(parent) => {
                let host = self.host_of(op, access.clone(), request.snapshot()).await?;
                let at = self
                    .clock
                    .now()
                    .to_zoned(self.setup.zone.clone())
                    .datetime();
                let mut folder = PathBuf::from(parent.as_str());
                folder.push(restore_folder_name(&host, request.snapshot(), at));
                RestoreTarget::NewFolder(folder.to_string_lossy().into_owned())
            }
            None => RestoreTarget::InPlace,
        };
        let folder = match &target {
            RestoreTarget::NewFolder(folder) => Some(folder.clone()),
            RestoreTarget::InPlace => None,
        };
        let job = Job::Restore(RestoreJob {
            repository: access,
            snapshot: request.snapshot().clone(),
            entry: request.entry().clone(),
            target,
            deadline_budget: backup_budget(None, None),
        });
        let trigger = match request.origin() {
            RestoreOrigin::LocalUi => Trigger::Manual,
            RestoreOrigin::Hub => Trigger::Hub,
        };
        let queued = self
            .run
            .queue(op, trigger, job)
            .await
            .with_warnings(warnings.iter().map(SecretWarning::warning).collect());
        Ok(PreparedRestore {
            queued,
            operation: op,
            folder,
            slot,
        })
    }

    /// Runs `prepared` until it ends, or `cancel` completes and it is
    /// cancelled; the slot is free again once it has ended.
    pub async fn run(
        &self,
        prepared: PreparedRestore,
        cancel: impl Future<Output = ()> + Send,
    ) -> Finished {
        let PreparedRestore { queued, slot, .. } = prepared;
        let finished = self.run.run_queued(queued, cancel).await;
        drop(slot);
        finished
    }

    /// The step-up confirmation of `ask`: the admin password, in the
    /// address's sign-in waits, with sign-in on; the snapshot's ID
    /// otherwise.
    async fn step_up(&self, ask: &RestoreAsk) -> Result<(), RestoreError> {
        confirm_step_up(
            self.access.as_deref(),
            ask.address,
            &ask.confirmation,
            StepUpFor::Snapshot(&ask.snapshot),
        )
        .await
        .map_err(|refused| match refused {
            StepUpRefused::Failed => RestoreError::StepUpFailed,
            StepUpRefused::Wait { seconds } => RestoreError::StepUpWait { seconds },
            StepUpRefused::Unavailable(text) => RestoreError::StepUpUnavailable(text),
        })
    }

    /// The agent's restore slot, for `op`, unless a restore holds it.
    fn take_slot(&self, op: OperationId) -> Result<Slot, RestoreError> {
        let mut running = lock(&self.running);
        if let Some(other) = *running {
            return Err(RestoreError::AlreadyRunning(other));
        }
        *running = Some(op);
        Ok(Slot {
            running: self.running.clone(),
        })
    }

    async fn resolved(
        &self,
        alias: &RepositoryAlias,
        repository: &RepositoryConfig,
    ) -> Result<ResolvedRepository, RestoreError> {
        self.resolve
            .resolve_saved(alias, repository)
            .await
            .map_err(RestoreError::Resolve)
    }

    /// The host `snapshot` records, read from the repository's listing,
    /// never from the request: it names the new folder.
    async fn host_of(
        &self,
        op: OperationId,
        access: RepositoryAccess,
        snapshot: &SnapshotId,
    ) -> Result<String, RestoreError> {
        let listed = run_read(
            self.engine.as_ref(),
            self.clock.as_ref(),
            op,
            Job::Ls(access),
        )
        .await
        .map_err(RestoreError::Read)?;
        let EngineResult::Snapshots(snapshots) = listed else {
            return Err(RestoreError::Read(MaintenanceError::Failed(
                domain::execution::FailureReason::EngineExited { exit_code: None },
            )));
        };
        snapshots
            .into_iter()
            .find(|listed| &listed.id == snapshot)
            .map(|listed| listed.hostname)
            .ok_or(RestoreError::SnapshotUnknown)
    }

    /// The request, if the agent's configured rules take it: one into a
    /// new folder goes under the agent's `restore-folder`, else the default
    /// folder.
    fn admit(
        &self,
        config: &AgentConfig,
        snapshot: SnapshotId,
        entry: FolderPath,
        destination: Destination,
        origin: RestoreOrigin,
    ) -> Result<RestoreRequest, RestoreError> {
        let agent = config.agent();
        let policy = RestorePolicy {
            allow_hub_restore: agent.allow_hub_restore,
            folder: agent
                .restore_folder
                .clone()
                .or_else(|| self.setup.default_folder.clone()),
        };
        RestoreRequest::admit(policy, snapshot, entry, destination, origin)
            .map_err(RestoreError::Refused)
    }
}

fn repository<'a>(
    config: &'a AgentConfig,
    alias: &RepositoryAlias,
) -> Result<&'a RepositoryConfig, RestoreError> {
    config
        .repositories()
        .get(alias)
        .ok_or(RestoreError::UnknownRepository)
}
