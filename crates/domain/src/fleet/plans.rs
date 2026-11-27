//! What a machine tells its hub of its plans and of a running operation's
//! progress (technical design §7.2; #1440): a snapshot of its plans, each
//! with its last run and its next, bounded so it fits one sync frame, and
//! the latest progress of an operation it runs, which is sent live and
//! never stored on the machine. The hub keeps the latest of each per
//! machine, and shows a progress only while the operation it measures
//! runs, as the machine's reports tell it.

use super::reported::{ReportedOperation, ReportedOutcome};
use super::{EpochSeconds, WireName};
use crate::execution::OperationStatus;
use crate::{OperationId, PlanId, Timestamp};

/// The most plans one `PlansSnapshot` holds; a hub refuses a snapshot with
/// more. A machine with more says how many it left out, and one whose
/// plans don't all fit one frame of the sync (`api::sync::MAX_FRAME_BYTES`)
/// leaves out its last plans until they do, counted the same way (#1698).
pub const MAX_PLANS_REPORTED: usize = 16;

/// How a plan's last run stands, as the machine's operation log tells it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LastRunOutcome {
    /// Waiting its turn in the machine's queue.
    Queued,
    Running,
    Succeeded,
    SucceededWithWarnings,
    Failed,
    Canceled,
}

impl LastRunOutcome {
    /// Its name on the wire and in the hub's store.
    pub const fn wire_name(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Succeeded => "succeeded",
            Self::SucceededWithWarnings => "succeeded-with-warnings",
            Self::Failed => "failed",
            Self::Canceled => "cancelled",
        }
    }

    /// The outcome `name` names; none for a name this build doesn't know.
    pub fn from_wire_name(name: &str) -> Option<Self> {
        [
            Self::Queued,
            Self::Running,
            Self::Succeeded,
            Self::SucceededWithWarnings,
            Self::Failed,
            Self::Canceled,
        ]
        .into_iter()
        .find(|outcome| outcome.wire_name() == name)
    }

    /// How an operation of `status` stands.
    pub const fn of(status: &OperationStatus) -> Self {
        match status {
            OperationStatus::Queued => Self::Queued,
            OperationStatus::Running => Self::Running,
            OperationStatus::Succeeded => Self::Succeeded,
            OperationStatus::SucceededWithWarnings => Self::SucceededWithWarnings,
            OperationStatus::Failed { .. } => Self::Failed,
            OperationStatus::Canceled => Self::Canceled,
        }
    }
}

/// A plan's last run: when it last changed (its end, else its start, else
/// when it was queued), on the machine's clock, and how it stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LastRun {
    pub at: EpochSeconds,
    pub outcome: LastRunOutcome,
}

impl LastRun {
    /// A plan's last run, of `status`, queued at `queued`, started at
    /// `started` and finished at `finished` when it did: told at its end,
    /// else its start, else when it was queued. None for a time before
    /// 1970, which the wire doesn't hold.
    pub fn of(
        status: &OperationStatus,
        queued: Timestamp,
        started: Option<Timestamp>,
        finished: Option<Timestamp>,
    ) -> Option<Self> {
        Some(Self {
            at: EpochSeconds::from_timestamp(finished.or(started).unwrap_or(queued))?,
            outcome: LastRunOutcome::of(status),
        })
    }
}

/// One plan as its machine reports it: its ID, its last run, none when it
/// never ran, and its next planned run, none when it has no schedule or the
/// scheduler planned none.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReportedPlan {
    pub plan: PlanId,
    pub last: Option<LastRun>,
    pub next_run: Option<EpochSeconds>,
}

/// A machine's plans, in the order its configuration writes them: at most
/// [`MAX_PLANS_REPORTED`], and how many more it has, past that cap or left
/// out to fit the message that carries the report.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlansReport {
    plans: Vec<ReportedPlan>,
    left_out: u64,
}

impl PlansReport {
    /// The report of `plans`: the first [`MAX_PLANS_REPORTED`], and the
    /// count of the rest.
    pub fn of(mut plans: Vec<ReportedPlan>) -> Self {
        let rest = plans.split_off(plans.len().min(MAX_PLANS_REPORTED));
        Self {
            plans,
            left_out: u64::try_from(rest.len()).unwrap_or(u64::MAX),
        }
    }

    /// The report a snapshot holds: none for one with more than
    /// [`MAX_PLANS_REPORTED`] plans, which no machine sends.
    pub fn from_parts(plans: Vec<ReportedPlan>, left_out: u64) -> Option<Self> {
        (plans.len() <= MAX_PLANS_REPORTED).then_some(Self { plans, left_out })
    }

    /// The plans reported, in the machine's order.
    pub fn plans(&self) -> &[ReportedPlan] {
        &self.plans
    }

    /// How many of the machine's plans the report leaves out.
    pub const fn left_out(&self) -> u64 {
        self.left_out
    }

    /// Leaves out the last plan reported, counted with the rest, as when
    /// the report doesn't fit the message that carries it: false, and the
    /// report unchanged, when it reports no plan.
    pub fn leave_out_last(&mut self) -> bool {
        if self.plans.pop().is_none() {
            return false;
        }
        self.left_out = self.left_out.saturating_add(1);
        true
    }
}

/// The latest progress of an operation a machine runs: the step it is in,
/// by its wire name, when the machine names one; what it counts (`bytes`
/// or `count`); how far it came of its total, when it has one; and when the
/// machine heard it, on its own clock.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReportedProgress {
    pub op_id: OperationId,
    pub step: Option<WireName>,
    pub unit: WireName,
    pub current: u64,
    pub total: Option<u64>,
    pub at: EpochSeconds,
}

/// The progress the hub shows of a machine: its latest, only while the
/// operation it measures runs among the machine's latest `operations`, as
/// its reports tell them. A progress whose operation ended, or that the
/// hub knows nothing of, shows nothing.
pub fn progress_shown(
    operations: &[ReportedOperation],
    progress: Option<ReportedProgress>,
) -> Option<ReportedProgress> {
    progress.filter(|progress| {
        operations.iter().any(|operation| {
            operation.op_id == progress.op_id
                && matches!(operation.outcome, ReportedOutcome::Running)
        })
    })
}

/// The kind of the agent's snapshot of its plans.
pub fn plans_kind() -> WireName {
    WireName::known("PlansSnapshot")
}

/// The kind of the hub's answer to a snapshot of plans: it keeps it.
pub fn plans_ack_kind() -> WireName {
    WireName::known("PlansAck")
}

/// The kind of the agent's progress of a running operation.
pub fn progress_kind() -> WireName {
    WireName::known("Progress")
}

#[cfg(test)]
mod tests;
