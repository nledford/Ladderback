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
use domain::{OperationId, PlanId, RepositoryAlias, SignedDuration, TimeZone, Timestamp};

use crate::ports::{
    Clock, ConfigError, ConfigStore, HistoryPage, HistoryQuery, MAX_PAGE_SIZE, OperationRecord,
    OperationStore, PageSize, StoreError,
};

/// How many operations the summary reads from the log at a time: the
/// history's largest page. The queue is looked for in the agent's newest
/// page only; a plan's last success and each repository's last reading are
/// looked for further back, a page at a time, until they are found.
pub const PAGE: usize = MAX_PAGE_SIZE;

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
    /// The operations queued or running among the agent's newest [`PAGE`],
    /// newest first.
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
    /// Its newest successful backup, with or without warnings, however far
    /// back the log holds it.
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
        let newest = self.page(None, None).await?;
        let queue = newest
            .operations
            .iter()
            .filter(|record| {
                matches!(
                    record.status,
                    OperationStatus::Queued | OperationStatus::Running
                )
            })
            .cloned()
            .collect();
        let unreadable = self.unreadable(&config, newest).await?;
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

    /// One page, [`PAGE`] operations, of `plan`'s operations or the agent's,
    /// newest first, those older than `after` when it is set.
    async fn page(
        &self,
        plan: Option<&PlanId>,
        after: Option<OperationId>,
    ) -> Result<HistoryPage, StatusError> {
        self.operations
            .list(&HistoryQuery {
                plan: plan.cloned(),
                after,
                limit: PageSize::new(PAGE),
            })
            .await
            .map_err(StatusError::Store)
    }

    async fn standing(&self, plan: &Plan, now: Timestamp) -> Result<PlanStanding, StatusError> {
        let mut page = self.page(Some(&plan.id), None).await?;
        let last = page.operations.first().cloned();
        // Back through the plan's history, a page at a time, until a
        // successful backup: a run of failures however long doesn't hide it.
        let found = loop {
            if let Some(record) = page.operations.iter().find(|record| is_success(record)) {
                break Some(record.clone());
            }
            let Some(next) = page.next else { break None };
            page = self.page(Some(&plan.id), Some(next)).await?;
        };
        let last_success = found.map(|record| LastSuccess {
            ago: now.duration_since(record.finished_at.unwrap_or(record.queued_at)),
            operation: record,
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
            last,
            last_success,
            overdue,
        })
    }

    /// The configured repositories whose newest operation that tells of
    /// reading them couldn't, in alias order. It starts from `newest`, the
    /// agent's newest page, and goes back through the log a page at a time
    /// until every configured repository has been told of, or the log ends.
    async fn unreadable(
        &self,
        config: &AgentConfig,
        newest: HistoryPage,
    ) -> Result<Vec<UnreadableRepository>, StatusError> {
        let mut told = Told::new(config);
        let mut page = newest;
        loop {
            told.take(&page.operations);
            let Some(next) = page.next.filter(|_| !told.complete()) else {
                break;
            };
            page = self.page(None, Some(next)).await?;
        }
        Ok(told.unreadable())
    }
}

/// Whether `record` is a successful backup, with warnings or without.
fn is_success(record: &OperationRecord) -> bool {
    record.kind == OperationKind::Backup
        && matches!(
            record.status,
            OperationStatus::Succeeded | OperationStatus::SucceededWithWarnings
        )
}

/// What the operations read so far tell of each configured repository: the
/// newest operation that tells of reading it, and whether it couldn't. An
/// operation's repository is the alias it recorded, or else its plan's
/// repository as the configuration names it now.
struct Told<'a> {
    config: &'a AgentConfig,
    told: BTreeMap<RepositoryAlias, Option<OperationRecord>>,
}

impl<'a> Told<'a> {
    fn new(config: &'a AgentConfig) -> Self {
        Self {
            config,
            told: BTreeMap::new(),
        }
    }

    /// Reads `records`, newest first, and older than any read before.
    fn take(&mut self, records: &[OperationRecord]) {
        for record in records {
            let Some(reading) = record.status.repository_reading() else {
                continue;
            };
            let repository = record.repository_alias.as_ref().or_else(|| {
                let plan = record.target.plan.as_ref()?;
                self.config
                    .plans()
                    .iter()
                    .find(|configured| &configured.id == plan)
                    .map(|configured| &configured.repository)
            });
            let Some(repository) = repository else {
                continue;
            };
            if !self.config.repositories().contains_key(repository) {
                continue;
            }
            self.told
                .entry(repository.clone())
                .or_insert((reading == RepositoryReading::Unreadable).then(|| record.clone()));
        }
    }

    /// Whether every configured repository has been told of.
    fn complete(&self) -> bool {
        self.told.len() == self.config.repositories().len()
    }

    fn unreadable(self) -> Vec<UnreadableRepository> {
        self.told
            .into_iter()
            .filter_map(|(repository, operation)| {
                operation.map(|operation| UnreadableRepository {
                    repository,
                    operation,
                })
            })
            .collect()
    }
}
