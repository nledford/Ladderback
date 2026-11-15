//! The restore use case (technical design §10.4): one entry of a snapshot
//! restored into a new folder under the agent's restore folder, or in place
//! after a dry run said what it would change. Every restore is confirmed by
//! step-up first, runs as an operation with a backup's deadline, one at a
//! time on the agent, and runs no hooks, as a manual maintenance run doesn't.

use std::net::IpAddr;
use std::sync::{Arc, Mutex};

use domain::browse::FolderPath;
use domain::restore::{Destination, RestoreOrigin, RestoreRefused};
use domain::{OperationId, RepositoryAlias, SnapshotId, TimeZone};

use crate::access::Access;
use crate::execution::{Finished, Queued, RunOperation};
use crate::maintenance::MaintenanceError;
use crate::ports::{Clock, ConfigError, ConfigStore, Engine, RestorePlanSummary};
use crate::secrets::{ResolveError, ResolveRepository};

/// What a restore needs that the configuration doesn't say.
#[derive(Clone, Debug)]
pub struct RestoreSetup {
    /// The restore folder when `[agent] restore-folder` is unset: an
    /// absolute path.
    pub default_folder: String,
    /// The machine's time zone, in which a new folder's name gives the time.
    pub zone: TimeZone,
}

/// One restore a person asked for, as it reached the agent.
#[derive(Clone, Debug)]
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
    /// on, the snapshot's short ID otherwise.
    pub confirmation: String,
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
    Config(ConfigError),
    /// The repository's secrets couldn't be resolved.
    Resolve(ResolveError),
    /// Reading the repository failed, or wasn't answered in time.
    Read(MaintenanceError),
}

/// A restore that has been confirmed, recorded as queued and holds the
/// agent's one restore slot until it is run and ends, or is dropped.
#[must_use = "a prepared restore that is never run stays queued in the log"]
pub struct PreparedRestore {
    queued: Queued,
    operation: OperationId,
    _slot: Slot,
}

impl PreparedRestore {
    /// The operation the restore runs as.
    pub fn operation(&self) -> OperationId {
        self.operation
    }
}

/// The agent's one restore slot, held by the restore that runs.
struct Slot {
    running: Arc<Mutex<Option<OperationId>>>,
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
        _op: OperationId,
        _alias: &RepositoryAlias,
        _snapshot: SnapshotId,
        _entry: FolderPath,
        _origin: RestoreOrigin,
    ) -> Result<RestorePlanSummary, RestoreError> {
        todo!("Restores::preview_in_place")
    }

    /// Confirms `ask` by step-up, admits it by the agent's rules, takes the
    /// agent's restore slot and records the restore as queued: nothing is
    /// recorded and the engine isn't asked when any of them refuses.
    pub async fn start(
        &self,
        _op: OperationId,
        _ask: RestoreAsk,
    ) -> Result<PreparedRestore, RestoreError> {
        todo!("Restores::start")
    }

    /// Runs `prepared` until it ends, or `cancel` completes and it is
    /// cancelled; the slot is free again once it has ended.
    pub async fn run(
        &self,
        _prepared: PreparedRestore,
        _cancel: impl Future<Output = ()> + Send,
    ) -> Finished {
        todo!("Restores::run")
    }

    /// The restore that runs on the agent, if one does.
    pub fn running(&self) -> Option<OperationId> {
        todo!("Restores::running")
    }
}
