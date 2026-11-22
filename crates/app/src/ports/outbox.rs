use async_trait::async_trait;
use domain::fleet::{AgentReport, Ingest, OutboxEpoch, ReportSeq};
use domain::{AgentId, Timestamp};

use super::StoreError;

/// One report in an outbox, at its place in its epoch.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OutboxEntry {
    pub epoch: OutboxEpoch,
    pub seq: ReportSeq,
    pub report: AgentReport,
}

/// What became of a report given to the outbox.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Appended {
    /// Kept at this place.
    Kept(ReportSeq),
    /// Kept at `seq` after the oldest start not yet acknowledged, at
    /// `gave_up`, gave way: the outbox was full
    /// ([`MAX_OUTBOX_REPORTS`](domain::fleet::MAX_OUTBOX_REPORTS)).
    KeptGivingUp { seq: ReportSeq, gave_up: ReportSeq },
    /// Refused: the outbox is full of ends, or out of numbers
    /// (`domain::fleet::admit`).
    Refused,
}

/// The agent's persistent outbox (technical design §7.2): the reports its
/// hub needs, each numbered in its epoch as it enters and kept until the
/// hub acknowledges it, across restarts. The epoch is made with the outbox
/// and lasts as long as its database.
#[async_trait]
pub trait Outbox: Send + Sync {
    /// Gives `report` its place, after the last one given, and keeps it,
    /// deciding by `domain::fleet::admit` in the same step, so appends that
    /// race each get a place of their own.
    async fn append(&self, report: &AgentReport) -> Result<Appended, StoreError>;

    /// At most `limit` reports not yet acknowledged, oldest first, after
    /// `after` (from the first when `None`).
    async fn unacknowledged(
        &self,
        after: Option<ReportSeq>,
        limit: usize,
    ) -> Result<Vec<OutboxEntry>, StoreError>;

    /// Lets go of every report of `epoch` through `through`. An epoch that
    /// isn't the outbox's lets go of nothing.
    async fn acknowledge(&self, epoch: OutboxEpoch, through: ReportSeq) -> Result<(), StoreError>;
}

/// What the hub's reports did with one an agent sent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ingested {
    pub ingest: Ingest,
    /// How many of the agent's oldest reports were let go of to keep its
    /// last [`MAX_REPORTS_HELD`](domain::fleet::MAX_REPORTS_HELD): none for
    /// a duplicate.
    pub dropped: u64,
}

/// The hub's reports of its agents (technical design §7.2): each agent's
/// last [`MAX_REPORTS_HELD`](domain::fleet::MAX_REPORTS_HELD) reports, how
/// many it let go of past that, and for each of its last epochs the place
/// it took reports through.
#[async_trait]
pub trait ReportInbox: Send + Sync {
    /// Takes `entry` from `agent`, received at `received_at`, deciding by
    /// `domain::fleet::ingest` in the same step: a report it took before is
    /// [`Ingest::Duplicate`] and changes nothing, so deliveries that race
    /// keep it once. A new report past the cap lets go of the agent's
    /// oldest and counts it.
    async fn ingest(
        &self,
        agent: AgentId,
        entry: &OutboxEntry,
        received_at: Timestamp,
    ) -> Result<Ingested, StoreError>;

    /// The reports kept of `agent`, in the order it took them.
    async fn reports(&self, agent: AgentId) -> Result<Vec<OutboxEntry>, StoreError>;

    /// How many of `agent`'s reports were let go of past the cap, ever.
    async fn dropped(&self, agent: AgentId) -> Result<u64, StoreError>;
}
