//! The status summary (§3.1, §4.6): "Is everything backed up?" in one
//! reading. For each plan, its last operation, its last successful backup
//! and how long ago, and whether it is overdue for its schedule; for the
//! agent, the operations queued or running and the repositories that couldn't
//! be read at their last attempt. Plans and repositories are named by their
//! IDs and aliases, never by anything a secret could be in (I4).

use std::collections::BTreeMap;
use std::sync::Arc;

use domain::execution::{OperationKind, OperationStatus, RepositoryReading};
use domain::planning::{AgentConfig, Plan};
use domain::{PlanId, RepositoryAlias, SignedDuration, TimeZone, Timestamp};

use crate::ports::{
    Clock, ConfigError, ConfigStore, HistoryQuery, MAX_PAGE_SIZE, OperationRecord, OperationStore,
    PageSize, StoreError,
};

/// How many of the newest operations the summary looks through: those of
/// each plan for its last success, and those of the agent for the queue and
/// each repository's last attempt. The history's largest page.
pub const RECENT_OPERATIONS: usize = MAX_PAGE_SIZE;

/// Reads the status summary.
pub struct ReadStatus {
    config: Arc<dyn ConfigStore>,
    operations: Arc<dyn OperationStore>,
    clock: Arc<dyn Clock>,
    /// The machine's time zone, which a cron schedule that names none is
    /// read in.
    zone: TimeZone,
}

/// The status summary.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Status {
    /// Each configured plan, in the order the configuration writes them.
    pub plans: Vec<PlanStanding>,
    /// The operations queued or running among the newest
    /// [`RECENT_OPERATIONS`], newest first.
    pub queue: Vec<OperationRecord>,
    /// The repositories whose last operation that tells of reading them
    /// couldn't, in the order of their aliases, each with that operation.
    pub unreadable: Vec<UnreadableRepository>,
}

/// One plan as the summary shows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlanStanding {
    pub plan: PlanId,
    /// Its newest operation, whatever its end.
    pub last: Option<OperationRecord>,
    /// Its newest successful backup among its newest
    /// [`RECENT_OPERATIONS`], with or without warnings.
    pub last_success: Option<LastSuccess>,
    /// Whether it is overdue (§3.1): never for a plan without a schedule.
    pub overdue: bool,
}

/// A plan's last successful backup, and how long ago it finished.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LastSuccess {
    pub operation: OperationRecord,
    pub ago: SignedDuration,
}

/// A repository that couldn't be read at its last attempt.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnreadableRepository {
    pub repository: RepositoryAlias,
    /// The operation that couldn't read it.
    pub operation: OperationRecord,
}

/// Why the summary couldn't be read.
#[derive(Debug)]
pub enum StatusError {
    /// The configuration can't be read or used.
    Config(Box<ConfigError>),
    /// The operation log can't be read.
    Store(StoreError),
}

#[cfg(test)]
mod tests;

impl ReadStatus {
    pub fn new(
        config: Arc<dyn ConfigStore>,
        operations: Arc<dyn OperationStore>,
        clock: Arc<dyn Clock>,
        zone: TimeZone,
    ) -> Self {
        Self {
            config,
            operations,
            clock,
            zone,
        }
    }

    /// The summary as it stands now.
    pub async fn read(&self) -> Result<Status, StatusError> {
        let config = self
            .config
            .load()
            .await
            .map_err(|error| StatusError::Config(Box::new(error)))?;
        let now = self.clock.now();
        let recent = self.recent(None).await?;
        let queue = recent
            .iter()
            .filter(|record| {
                matches!(
                    record.status,
                    OperationStatus::Queued | OperationStatus::Running
                )
            })
            .cloned()
            .collect();
        let unreadable = unreadable(&config, &recent);
        let mut plans = Vec::with_capacity(config.plans().len());
        for plan in config.plans() {
            plans.push(self.standing(plan, now).await?);
        }
        Ok(Status {
            plans,
            queue,
            unreadable,
        })
    }

    /// The newest [`RECENT_OPERATIONS`] of `plan`, or of the agent.
    async fn recent(&self, plan: Option<&PlanId>) -> Result<Vec<OperationRecord>, StatusError> {
        let page = self
            .operations
            .list(&HistoryQuery {
                plan: plan.cloned(),
                after: None,
                limit: PageSize::new(RECENT_OPERATIONS),
            })
            .await
            .map_err(StatusError::Store)?;
        Ok(page.operations)
    }

    async fn standing(&self, plan: &Plan, now: Timestamp) -> Result<PlanStanding, StatusError> {
        let recent = self.recent(Some(&plan.id)).await?;
        let last_success = recent
            .iter()
            .find(|record| {
                record.kind == OperationKind::Backup
                    && matches!(
                        record.status,
                        OperationStatus::Succeeded | OperationStatus::SucceededWithWarnings
                    )
            })
            .map(|record| LastSuccess {
                ago: now.duration_since(record.finished_at.unwrap_or(record.queued_at)),
                operation: record.clone(),
            });
        let overdue = plan.schedule.as_ref().is_some_and(|schedule| {
            let started = last_success.as_ref().map(|success| {
                success
                    .operation
                    .started_at
                    .unwrap_or(success.operation.queued_at)
            });
            schedule.is_overdue(&self.zone, started, now)
        });
        Ok(PlanStanding {
            plan: plan.id.clone(),
            last: recent.into_iter().next(),
            last_success,
            overdue,
        })
    }
}

/// The configured repositories whose newest operation among `recent` that
/// tells of reading them couldn't, in alias order. An operation's
/// repository is the alias it recorded, or else its plan's repository as the
/// configuration names it now.
fn unreadable(config: &AgentConfig, recent: &[OperationRecord]) -> Vec<UnreadableRepository> {
    let mut told: BTreeMap<&RepositoryAlias, Option<&OperationRecord>> = BTreeMap::new();
    for record in recent {
        let Some(reading) = record.status.repository_reading() else {
            continue;
        };
        let repository = record.repository_alias.as_ref().or_else(|| {
            let plan = record.target.plan.as_ref()?;
            config
                .plans()
                .iter()
                .find(|configured| &configured.id == plan)
                .map(|configured| &configured.repository)
        });
        let Some(repository) = repository else {
            continue;
        };
        if !config.repositories().contains_key(repository) {
            continue;
        }
        told.entry(repository)
            .or_insert((reading == RepositoryReading::Unreadable).then_some(record));
    }
    told.into_iter()
        .filter_map(|(repository, record)| {
            record.map(|operation| UnreadableRepository {
                repository: repository.clone(),
                operation: operation.clone(),
            })
        })
        .collect()
}
