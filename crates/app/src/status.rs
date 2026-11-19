//! The status summary (§3.1, §4.6): "Is everything backed up?" in one
//! reading. For each plan, its last operation, its last successful backup
//! and how long ago, and whether it is overdue for its schedule; for the
//! agent, the operations queued or running and the repositories that couldn't
//! be read at their last attempt. Plans and repositories are named by their
//! IDs and aliases, never by anything a secret could be in (I4).

use std::sync::Arc;

use domain::execution::OperationKind;
use domain::{PlanId, RepositoryAlias, SignedDuration, TimeZone};

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
        let _ = (
            &self.config,
            &self.operations,
            &self.clock,
            &self.zone,
            OperationKind::Backup,
            HistoryQuery {
                plan: None,
                after: None,
                limit: PageSize::new(RECENT_OPERATIONS),
            },
        );
        todo!()
    }
}
