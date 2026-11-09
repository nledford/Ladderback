use async_trait::async_trait;
use domain::execution::{Operation, OperationKind, OperationStatus, Target, Trigger, Warning};
use domain::{OperationId, PlanId, RepositoryId, Timestamp};

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
///   included, so a save that failed is made good by the next one. It
///   refuses one thing: a running maintenance operation that would be a
///   second on its repository (an [`Unavailable`](StoreError::Unavailable)
///   error, with nothing changed).
/// - [`try_start`](Self::try_start) stores an operation that has just
///   started running as `save` does, and enforces I7: maintenance operations
///   on one repository run one at a time. It decides and stores in one
///   atomic step, so of any number of starts racing for a repository, through
///   any number of handles, exactly one wins; the others store nothing and
///   name the winner. A running maintenance operation that names its
///   repository holds it ([`OperationRecord::maintaining`]); nothing else
///   does, so a queued or finished one, a backup, an identify and a
///   diagnostic job are never refused and never make another operation wait.
///   The operation that holds a repository may start again.
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

    /// Stores `operation`, which the caller has just moved to
    /// [`Running`](OperationStatus::Running), as [`save`](Self::save) would,
    /// unless it is a maintenance operation and another one is already
    /// running on its repository (I7): then nothing is stored and the answer
    /// is [`Start::Refused`], naming the one that is. An operation that isn't
    /// running is an [`Unavailable`](StoreError::Unavailable) error, with
    /// nothing stored.
    ///
    /// A use case that runs maintenance calls this before it starts the
    /// engine and fails closed: the engine starts on [`Start::Started`] only,
    /// not on a refusal and not on an error. An operation that names no
    /// repository holds none, so the use case must know the repository's ID
    /// first (an identify reports it).
    async fn try_start(&self, operation: &Operation) -> Result<Start, StoreError>;

    /// The operation with its warnings, or `None` when no such operation was saved.
    async fn get(&self, id: OperationId) -> Result<Option<OperationDetail>, StoreError>;

    /// One page of history, newest first. The page ends at `query.limit`
    /// operations; [`HistoryPage::next`] says whether older ones remain.
    async fn list(&self, query: &HistoryQuery) -> Result<HistoryPage, StoreError>;
}

/// What [`OperationStore::try_start`] decided.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Start {
    /// The operation is stored as running. A maintenance operation now holds
    /// its repository.
    Started,
    /// Another maintenance operation, `running`, is running on the
    /// repository, so nothing was stored.
    Refused { running: OperationId },
}

/// Why the store couldn't do what it was asked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StoreError {
    /// The database couldn't be read or written, or the store refused the
    /// write under I7 (a `save` that would be a second running maintenance
    /// operation, a `try_start` of an operation that isn't running); the
    /// latter can't succeed until the holder ends, so it isn't worth
    /// retrying as is. `detail` is its own message.
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
        Self(requested.clamp(1, MAX_PAGE_SIZE))
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
        Self {
            id: operation.id(),
            kind: operation.kind(),
            trigger: operation.trigger(),
            target: operation.target().clone(),
            status: operation.status().clone(),
            queued_at: to_millisecond(operation.queued_at()),
            started_at: operation.started_at().map(to_millisecond),
            finished_at: operation.finished_at().map(to_millisecond),
            warning_count: operation.warning_count(),
            result: result.cloned(),
        }
    }

    /// The repository this record is running maintenance on: set for a
    /// running maintenance operation that names its repository, and `None`
    /// for everything else. A record that has one holds that repository, and
    /// no other maintenance operation may be running on it (I7).
    pub fn maintaining(&self) -> Option<&RepositoryId> {
        maintained_repository(self.kind, &self.status, &self.target)
    }
}

/// The repository an operation of this kind, status and target holds (I7):
/// the one a running maintenance operation names, and nothing otherwise. It
/// is [`OperationRecord::maintaining`] for a stored record, and lets a store
/// ask the same of an [`Operation`] without copying it into a record.
pub fn maintained_repository<'a>(
    kind: OperationKind,
    status: &OperationStatus,
    target: &'a Target,
) -> Option<&'a RepositoryId> {
    if kind.is_maintenance() && *status == OperationStatus::Running {
        target.repository.as_ref()
    } else {
        None
    }
}

/// `at` cut to the millisecond, never rounded up.
fn to_millisecond(at: Timestamp) -> Timestamp {
    // Every timestamp has a whole-millisecond count that is a timestamp
    // again; the fallback is for a range this can't leave.
    Timestamp::from_millisecond(at.as_millisecond()).unwrap_or(at)
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
