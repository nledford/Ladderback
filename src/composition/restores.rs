//! The restore routes' side of the wiring (technical design §10.4): the
//! restore use case of `app` over the runner's engine, configuration and
//! operation log, and its refusals as the typed ones the routes answer with.

use std::sync::Arc;

use api::ErrorCode;
use app::access::Access;
use app::execution::RunOperation;
use app::maintenance::MaintenanceError;
use app::ports::ConfigStore;
use app::restore::{RestoreAsk, RestoreError, RestoreSetup, Restores};
use async_trait::async_trait;
use domain::browse::FolderPath;
use domain::restore::{RestoreOrigin, RestoreRefused};
use domain::{OperationId, RepositoryAlias, SnapshotId};
use http_adapter::{MaintenanceRefusal, RestoreRefusal, RestoreStart};
use platform::SystemClock;
use uuid::Uuid;

use super::{CancelRegistry, PlanRunner, maintenance, runs};

/// The restores of the agent: one use case, so one restore runs at a time.
pub(super) struct RestoreService {
    restores: Arc<Restores>,
    config: Arc<dyn ConfigStore>,
    registry: Arc<CancelRegistry>,
    runs: runs::Runs,
}

impl RestoreService {
    /// Restores over `runner`'s parts into `default_folder`, confirmed by
    /// `access` with sign-in on and by the snapshot's short ID without it.
    /// A restore runs no hooks.
    pub(super) fn new(
        runner: &PlanRunner,
        default_folder: String,
        access: Option<Arc<Access>>,
    ) -> Self {
        let clock = Arc::new(SystemClock);
        let restores = Restores::new(
            Arc::clone(&runner.engine),
            clock.clone(),
            Arc::clone(&runner.config),
            runner.resolve.as_ref().clone(),
            RunOperation::new(Arc::clone(&runner.engine), clock, Arc::clone(&runner.store))
                .with_events(Arc::clone(&runner.events)),
            RestoreSetup {
                default_folder,
                zone: domain::TimeZone::system(),
            },
        );
        let restores = match access {
            Some(access) => restores.with_access(access),
            None => restores,
        };
        Self {
            restores: Arc::new(restores),
            config: Arc::clone(&runner.config),
            registry: Arc::clone(&runner.registry),
            runs: runner.runs.clone(),
        }
    }
}

fn alias_of(repository: &str) -> Result<RepositoryAlias, RestoreRefusal> {
    RepositoryAlias::parse(repository)
        .map_err(|_| RestoreRefusal::Repository(MaintenanceRefusal::UnknownRepository))
}

/// Why a restore was refused, as the typed refusal the route answers with.
/// What reading the repository or the configuration refused is told as the
/// maintenance reads tell it.
async fn refusal(
    config: &dyn ConfigStore,
    alias: &RepositoryAlias,
    error: RestoreError,
) -> RestoreRefusal {
    let read = match error {
        RestoreError::StepUpFailed => return RestoreRefusal::StepUpFailed,
        RestoreError::StepUpWait { seconds } => return RestoreRefusal::StepUpWait { seconds },
        RestoreError::StepUpUnavailable(detail) => {
            return RestoreRefusal::StepUpUnavailable(detail);
        }
        RestoreError::Refused(RestoreRefused::InPlaceFromHub) => {
            return RestoreRefusal::InPlaceNotHere;
        }
        RestoreError::Refused(RestoreRefused::HubNotAllowed) => {
            return RestoreRefusal::HubNotAllowed;
        }
        RestoreError::AlreadyRunning(running) => {
            return RestoreRefusal::Running {
                operation: running.as_uuid().hyphenated().to_string(),
            };
        }
        RestoreError::SnapshotUnknown => return RestoreRefusal::SnapshotUnknown,
        RestoreError::UnknownRepository => MaintenanceError::UnknownRepository,
        RestoreError::Config(error) => MaintenanceError::Unavailable(error),
        RestoreError::Resolve(error) => MaintenanceError::Resolve(error),
        RestoreError::Read(error) => error,
    };
    RestoreRefusal::Repository(maintenance::refusal(config, alias, read).await)
}

#[async_trait]
impl http_adapter::Restores for RestoreService {
    async fn preview(
        &self,
        repository: String,
        snapshot: SnapshotId,
        entry: FolderPath,
    ) -> Result<api::RestorePreviewDto, RestoreRefusal> {
        let alias = alias_of(&repository)?;
        let operation = OperationId::from_uuid(Uuid::now_v7());
        match self
            .restores
            .preview_in_place(operation, &alias, snapshot, entry, RestoreOrigin::LocalUi)
            .await
        {
            Ok(plan) => Ok(api::RestorePreviewDto {
                added: plan.files_added,
                overwritten: plan.files_overwritten,
                unchanged: plan.files_unchanged,
                kept: plan.files_kept,
                bytes: plan.bytes_to_write,
            }),
            Err(error) => Err(refusal(self.config.as_ref(), &alias, error).await),
        }
    }

    /// Confirms and starts the restore on a task of its own, as a
    /// maintenance start does: a client that hangs up can't drop the write
    /// of the queued operation halfway, and the route waits for its answer.
    async fn start(
        &self,
        repository: String,
        start: RestoreStart,
    ) -> Result<String, RestoreRefusal> {
        // Counted as a maintenance run is, so a restart waits for it
        // (decision 94); while one waits, no restore starts.
        let Some(maintaining) = self.runs.maintaining() else {
            return Err(RestoreRefusal::Repository(
                MaintenanceRefusal::Unavailable {
                    code: ErrorCode::RestartPending,
                    args: Vec::new(),
                    detail: None,
                },
            ));
        };
        let alias = alias_of(&repository)?;
        let RestoreStart {
            snapshot,
            entry,
            destination,
            address,
            confirmation,
        } = start;
        let ask = RestoreAsk {
            alias: alias.clone(),
            snapshot,
            entry,
            destination,
            origin: RestoreOrigin::LocalUi,
            address,
            confirmation,
        };
        let operation = OperationId::from_uuid(Uuid::now_v7());
        let (started, wait_for_start) = futures::channel::oneshot::channel();
        let restores = Arc::clone(&self.restores);
        let registry = Arc::clone(&self.registry);
        let config = Arc::clone(&self.config);
        tokio::spawn(self.runs.until_frozen(async move {
            let _maintaining = maintaining;
            let prepared = match restores.start(operation, ask).await {
                Ok(prepared) => prepared,
                Err(error) => {
                    let _ = started.send(Err(refusal(config.as_ref(), &alias, error).await));
                    return;
                }
            };
            // Parked before the route answers, so a cancel that arrives the
            // instant it does finds it (§4.2).
            let cancel = registry.register(operation);
            let cancel = async move {
                let _ = cancel.await;
            };
            let _ = started.send(Ok(()));
            let finished = restores.run(prepared, cancel).await;
            registry.finish(&operation);
            super::log_ended(&finished);
        }));
        match wait_for_start.await {
            Ok(answer) => answer.map(|()| operation.as_uuid().hyphenated().to_string()),
            Err(_) => Err(RestoreRefusal::Repository(
                MaintenanceRefusal::Unavailable {
                    code: ErrorCode::OperationFailed,
                    args: Vec::new(),
                    detail: None,
                },
            )),
        }
    }
}
