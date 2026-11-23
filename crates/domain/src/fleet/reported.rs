//! What the hub's dashboard shows of a machine's operations (technical
//! design §4.3, §7.2; #569): the reports the hub took of the machine, read
//! as operations. A report is an operation's start or its end, so an
//! operation is known by both, by its start alone while it runs (or while
//! its end hasn't reached the hub), or by its end alone when the machine's
//! full outbox gave its start up (§16, decision 145).

use super::outbox::{AgentReport, Happening};
use super::{EpochSeconds, WireName};
use crate::OperationId;

/// How many of a machine's latest operations the dashboard shows.
pub const OPERATIONS_SHOWN: usize = 10;

/// How many of a machine's latest reports the hub reads for them: two for
/// each operation shown, a start and its end, and room for operations whose
/// start or end is all the hub took.
pub const REPORTS_READ: usize = 4 * OPERATIONS_SHOWN;

/// How an operation the hub heard of came out, as far as it heard.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReportedOutcome {
    /// Its start, and no end yet.
    Running,
    Succeeded,
    /// It failed with this error code, by its wire name.
    Failed {
        code: WireName,
    },
}

/// One operation of a machine, as its reports tell it: when it started,
/// when it ended, both on the machine's clock, and how it came out.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReportedOperation {
    pub op_id: OperationId,
    /// None when the hub took only its end.
    pub started_at: Option<EpochSeconds>,
    /// None while it runs.
    pub ended_at: Option<EpochSeconds>,
    pub outcome: ReportedOutcome,
}

/// The latest `limit` operations `reports` tell of, newest first: the
/// operation whose report came last first. `reports` are in the order the
/// hub took them. An operation's end settles it: a start heard after its
/// end, as a resend can bring, doesn't set it running again.
pub fn latest_operations(reports: &[AgentReport], limit: usize) -> Vec<ReportedOperation> {
    todo!("{reports:?} {limit}")
}

#[cfg(test)]
mod tests;
