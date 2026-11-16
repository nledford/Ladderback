//! The download routes' side of the wiring (technical design §10.4): the
//! download use case of `app` over the runner's engine, configuration and
//! operation log, its refusals as the typed ones the routes answer with, and
//! the sweep that gives up the downloads nobody fetched in time.

use std::sync::{Arc, Weak};
use std::time::Duration;

use app::download::{Download, DownloadAsk, DownloadError, Downloads};
use app::maintenance::MaintenanceError;
use app::ports::{ConfigStore, Random};
use async_trait::async_trait;
use domain::restore::download::DownloadTicket;
use domain::{OperationId, RepositoryAlias};
use http_adapter::{DownloadRefusal, DownloadStart, Handover, MaintenanceRefusal, SignIn};
use platform::SystemClock;
use uuid::Uuid;

use super::{PlanRunner, maintenance, runs};

/// How often the downloads whose minute ran out are given up, when no
/// other download asks first: their engine jobs stop and their turns are
/// free again (§10.4).
const SWEEP_EVERY: Duration = Duration::from_secs(10);

/// The downloads of the agent: one use case, so two at a time in all.
pub(super) struct DownloadService {
    downloads: Arc<Downloads>,
    config: Arc<dyn ConfigStore>,
    runs: runs::Runs,
}

impl DownloadService {
    /// Downloads over `runner`'s parts, confirmed as `sign_in` says: by the
    /// admin password with sign-in on, by the snapshot's short ID with it
    /// off, their tickets drawn from `random`. Starts the sweep, which ends
    /// once the service is dropped.
    pub(super) fn new(runner: &PlanRunner, sign_in: &SignIn, random: Arc<dyn Random>) -> Self {
        let downloads = Downloads::new(
            Arc::clone(&runner.engine),
            Arc::new(SystemClock),
            Arc::clone(&runner.config),
            runner.resolve.as_ref().clone(),
            random,
            Arc::clone(&runner.store),
        );
        let downloads = Arc::new(match sign_in {
            SignIn::On(access) => downloads.with_access(Arc::clone(access)),
            SignIn::Off { .. } => downloads,
        });
        tokio::spawn(sweep(Arc::downgrade(&downloads)));
        Self {
            downloads,
            config: Arc::clone(&runner.config),
            runs: runner.runs.clone(),
        }
    }
}

/// Gives up the expired downloads every [`SWEEP_EVERY`] while `downloads`
/// lives.
async fn sweep(downloads: Weak<Downloads>) {
    let mut every = tokio::time::interval(SWEEP_EVERY);
    every.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        every.tick().await;
        let Some(downloads) = downloads.upgrade() else {
            return;
        };
        downloads.give_up_expired().await;
    }
}

/// Why a download was refused, as the typed refusal the route answers
/// with. What reading the repository or the configuration refused is told
/// as the maintenance reads tell it.
async fn refusal(
    config: &dyn ConfigStore,
    alias: &RepositoryAlias,
    error: DownloadError,
) -> DownloadRefusal {
    let read = match error {
        DownloadError::StepUpFailed => return DownloadRefusal::StepUpFailed,
        DownloadError::StepUpWait { seconds } => return DownloadRefusal::StepUpWait { seconds },
        DownloadError::StepUpUnavailable(detail) => {
            return DownloadRefusal::StepUpUnavailable(detail);
        }
        DownloadError::RandomUnavailable => {
            return DownloadRefusal::TicketUnavailable(
                "the platform gave no random bytes for the download's ticket".to_owned(),
            );
        }
        DownloadError::Busy => return DownloadRefusal::Busy,
        DownloadError::SnapshotUnknown => return DownloadRefusal::SnapshotUnknown,
        DownloadError::EntryUnknown => return DownloadRefusal::EntryUnknown,
        // A start never answers `TicketUnknown`: only a fetch does.
        DownloadError::TicketUnknown | DownloadError::UnknownRepository => {
            MaintenanceError::UnknownRepository
        }
        DownloadError::Config(error) => MaintenanceError::Unavailable(error),
        DownloadError::Resolve(error) => MaintenanceError::Resolve(error),
        DownloadError::Read(error) => error,
    };
    DownloadRefusal::Repository(maintenance::refusal(config, alias, read).await)
}

#[async_trait]
impl http_adapter::Downloads for DownloadService {
    /// Confirms the download on a task of its own, as a restore's start
    /// does: a client that hangs up can't drop the record of the download
    /// halfway, and the route waits for its answer. While a restart waits
    /// for the running operations, none is confirmed, as no restore starts
    /// then; one being handed over doesn't hold the restart off, which cuts
    /// it short.
    async fn start(
        &self,
        repository: String,
        start: DownloadStart,
    ) -> Result<DownloadTicket, DownloadRefusal> {
        if self.runs.held() {
            return Err(DownloadRefusal::Repository(
                MaintenanceRefusal::Unavailable {
                    code: api::ErrorCode::RestartPending,
                    args: Vec::new(),
                    detail: None,
                },
            ));
        }
        let alias = RepositoryAlias::parse(&repository)
            .map_err(|_| DownloadRefusal::Repository(MaintenanceRefusal::UnknownRepository))?;
        let DownloadStart {
            snapshot,
            entry,
            format,
            address,
            confirmation,
        } = start;
        let ask = DownloadAsk {
            alias: alias.clone(),
            snapshot,
            entry,
            format,
            address,
            confirmation,
        };
        let operation = OperationId::from_uuid(Uuid::now_v7());
        let downloads = Arc::clone(&self.downloads);
        let config = Arc::clone(&self.config);
        let started = tokio::spawn(async move {
            match downloads.start(operation, ask).await {
                Ok(ticket) => Ok(ticket),
                Err(error) => Err(refusal(config.as_ref(), &alias, error).await),
            }
        });
        started.await.unwrap_or_else(|_| {
            Err(DownloadRefusal::Repository(
                MaintenanceRefusal::Unavailable {
                    code: api::ErrorCode::OperationFailed,
                    args: Vec::new(),
                    detail: None,
                },
            ))
        })
    }

    /// The download under `ticket`; how it ends is written to the log on a
    /// task of its own, once its pieces end or are dropped. Downloads whose
    /// minute ran out are given up by the sweep, never by this request, so
    /// a browser that hangs up can't leave one running in the log.
    async fn fetch(&self, ticket: DownloadTicket) -> Option<Handover> {
        let Download {
            file_name,
            shape,
            pieces,
            recorded,
        } = self.downloads.fetch(&ticket).ok()?;
        tokio::spawn(recorded);
        Some(Handover {
            file_name,
            shape,
            pieces,
        })
    }
}
