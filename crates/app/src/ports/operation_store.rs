use async_trait::async_trait;
use domain::execution::{Operation, OperationKind, OperationStatus, Target, Trigger, Warning};
use domain::{OperationId, PlanId, Timestamp};

use crate::ports::EngineResult;

/// The most operations one page of history holds.
pub const MAX_PAGE_SIZE: usize = 200;

/// How many operations a page of history holds when the caller doesn't say.
pub const DEFAULT_PAGE_SIZE: usize = 50;

/// Keeps the operation log: every operation as it progresses, and the history
/// read from it (technical design §3.2 and §9).
///
/// Backups never depend on the log, so a store that fails never fails an
/// operation: [`RunOperation`](crate::execution::RunOperation) reports the
/// failure and carries on.
///
/// Contract (checked by `contract::operation_store`):
/// - [`save`](Self::save) stores an operation under its ID: the first call
///   adds it, and every later call replaces what was stored, warnings
///   included, so a save that failed is made good by the next one.
/// - [`get`](Self::get) returns exactly what was last saved, as
///   [`OperationRecord::of`] describes it, with the warnings in the order the
///   operation recorded them. Times are kept to the millisecond.
/// - [`list`](Self::list) returns operations newest first, which is highest
///   ID first, because IDs are UUIDv7 and so ordered by time.
#[async_trait]
pub trait OperationStore: Send + Sync {
    /// Stores `operation`, and `result` when the engine reported one.
    async fn save(
        &self,
        operation: &Operation,
        result: Option<&EngineResult>,
    ) -> Result<(), StoreError>;

    /// The operation with its warnings, or `None` when no such operation was saved.
    async fn get(&self, id: OperationId) -> Result<Option<OperationDetail>, StoreError>;

    /// One page of history, newest first. The page ends at `query.limit`
    /// operations; [`HistoryPage::next`] says whether older ones remain.
    async fn list(&self, query: &HistoryQuery) -> Result<HistoryPage, StoreError>;
}

/// Why the store couldn't do what it was asked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StoreError {
    /// The database couldn't be read or written. `detail` is its own message.
    Unavailable { detail: String },
    /// A stored row isn't something this version wrote, so it can't be read
    /// back as an operation. `detail` names what was wrong with it.
    Corrupt { detail: String },
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(self, f)
    }
}

impl std::error::Error for StoreError {}

/// How many operations a page of history holds: always between 1 and
/// [`MAX_PAGE_SIZE`], whatever a caller asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PageSize(usize);

impl PageSize {
    pub const DEFAULT: Self = Self(DEFAULT_PAGE_SIZE);

    /// The page size asked for, raised to 1 or cut to [`MAX_PAGE_SIZE`] when
    /// it is outside that range.
    pub fn new(requested: usize) -> Self {
        todo!()
    }

    pub fn get(self) -> usize {
        self.0
    }
}

impl Default for PageSize {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// Which operations to list, and how many.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HistoryQuery {
    /// Only this plan's operations; `None` lists them all.
    pub plan: Option<PlanId>,
    /// Only operations older than this one (IDs below it). The `next` of the
    /// previous page; it needn't name an operation that still exists.
    pub after: Option<OperationId>,
    pub limit: PageSize,
}

/// One page of history.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HistoryPage {
    /// Newest first.
    pub operations: Vec<OperationRecord>,
    /// Set when older operations remain: the last one of this page, to pass
    /// as [`HistoryQuery::after`].
    pub next: Option<OperationId>,
}

/// An operation as the log keeps it: what history lists.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OperationRecord {
    pub id: OperationId,
    pub kind: OperationKind,
    pub trigger: Trigger,
    pub target: Target,
    pub status: OperationStatus,
    /// Times are kept to the millisecond.
    pub queued_at: Timestamp,
    pub started_at: Option<Timestamp>,
    pub finished_at: Option<Timestamp>,
    /// Every warning the operation recorded, kept or not.
    pub warning_count: u64,
    /// What the engine reported when the operation succeeded.
    pub result: Option<EngineResult>,
}

impl OperationRecord {
    /// How the log describes `operation` and the engine's `result`: its
    /// times cut to the millisecond, the precision the log keeps.
    pub fn of(operation: &Operation, result: Option<&EngineResult>) -> Self {
        todo!()
    }
}

/// One operation in full: its record and the warnings it kept.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OperationDetail {
    pub record: OperationRecord,
    /// In the order they were recorded. Only the first
    /// [`MAX_STORED_WARNINGS`](domain::execution::MAX_STORED_WARNINGS) are
    /// kept: `record.warning_count` has the total.
    pub warnings: Vec<Warning>,
}
