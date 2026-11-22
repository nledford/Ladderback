//! The agent's outbox and the hub's ingest of what it replays (technical
//! design §7.2; §16, decision 120, "Signed messages after the handshake"):
//! an agent keeps each report its hub needs, how one of its operations
//! started and ended, numbered in order as it enters, until the hub
//! acknowledges it; the hub takes each report once, however often it comes.
//! Rules only: the store keeps the reports, and the sync session carries
//! them.

use std::num::NonZeroU64;

use uuid::Uuid;

use super::{EpochSeconds, WireName};
use crate::OperationId;

/// The most reports an agent's outbox holds. A start makes room by giving
/// up the oldest start; an end is never given up ([`admit`]).
pub const MAX_OUTBOX_REPORTS: u64 = 10_000;

/// The most reports the hub keeps of one agent, its oldest let go first.
pub const MAX_REPORTS_HELD: u64 = 10_000;

/// The most outbox epochs the hub remembers of one agent, the one it took a
/// report of longest ago forgotten first.
pub const MAX_EPOCHS_HELD: u64 = 8;

/// The most reports the agent sends before it waits for the hub's
/// acknowledgement of the first of them.
pub const SEND_WINDOW: usize = 32;

/// The kind of the agent's message that carries one report.
pub fn report_kind() -> WireName {
    WireName::known("OpEvent")
}

/// The kind of the hub's acknowledgement of the reports through one.
pub fn ack_kind() -> WireName {
    WireName::known("Ack")
}

/// An outbox's epoch: a random ID made with the outbox, so the reports of a
/// rebuilt database are never taken for ones the hub already acknowledged.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct OutboxEpoch(Uuid);

impl OutboxEpoch {
    pub const fn from_uuid(uuid: Uuid) -> Self {
        Self(uuid)
    }

    pub const fn as_uuid(self) -> Uuid {
        self.0
    }
}

/// A report's place in its epoch: 1 for the first, and each next one above,
/// never reused once given, even after the report it was given to is gone.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ReportSeq(NonZeroU64);

impl ReportSeq {
    /// The first report's place.
    pub const FIRST: Self = Self(NonZeroU64::MIN);

    /// The place `number` names; none for 0.
    pub const fn new(number: u64) -> Option<Self> {
        match NonZeroU64::new(number) {
            Some(number) => Some(Self(number)),
            None => None,
        }
    }

    pub const fn get(self) -> u64 {
        self.0.get()
    }

    /// The place after this one; none past the last a number can hold, so
    /// an outbox that ran out of numbers refuses a report rather than reuse
    /// one.
    pub fn next(self) -> Option<Self> {
        todo!()
    }
}

/// What a report says happened to an operation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Happening {
    Started,
    Succeeded,
    /// It failed with this error code, by its wire name: a code, never the
    /// engine's words, which can name a file (I4 holds of the rest).
    Failed {
        code: WireName,
    },
}

/// One report of the agent's to its hub.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentReport {
    pub op_id: OperationId,
    pub happening: Happening,
    /// When it happened, on the agent's clock.
    pub at: EpochSeconds,
}

impl AgentReport {
    /// Whether the outbox keeps this report however full it is: an end,
    /// which says how an operation came out, unlike a start, which the end
    /// that follows makes up for.
    pub const fn kept(&self) -> bool {
        todo!()
    }
}

/// What the outbox does with a new report.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutboxAdmission {
    /// It has room: the report is appended.
    Append,
    /// It is full: the start at this place gives way, and the report is
    /// appended.
    EvictThenAppend(ReportSeq),
    /// It is full of ends, or of numbers: the report is refused, and the
    /// caller says so in the log.
    Refuse,
}

/// What an outbox that holds `held` reports, the oldest start among them at
/// `oldest_start`, does with `report`, the next of which would take `next`
/// (none when the numbers ran out). Under [`MAX_OUTBOX_REPORTS`] it is
/// appended; at it, the oldest start gives way; full of ends, the report is
/// refused, so no end is ever given up for anything.
pub fn admit(
    held: u64,
    oldest_start: Option<ReportSeq>,
    next: Option<ReportSeq>,
    report: &AgentReport,
) -> OutboxAdmission {
    todo!()
}

/// The hub's acknowledgement of every report through `seq` of `epoch`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ack {
    pub epoch: OutboxEpoch,
    pub seq: ReportSeq,
}

/// Why the agent acts on no acknowledgement. Each closes the connection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AckRefused {
    /// Of an epoch that isn't the outbox's.
    OtherEpoch,
    /// Of a report this connection hasn't sent.
    NotSent,
}

/// Whether the agent, whose outbox's epoch is `epoch` and which sent on
/// this connection the reports through `sent_through`, takes `ack`.
pub fn check_ack(
    epoch: OutboxEpoch,
    sent_through: Option<ReportSeq>,
    ack: Ack,
) -> Result<(), AckRefused> {
    todo!()
}

/// What the hub does with a report.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ingest {
    /// It hasn't taken it: it keeps it and acknowledges it.
    New,
    /// It took it before: it acknowledges it again and keeps nothing.
    Duplicate,
    /// It took it before, and a report of the epoch at least a
    /// [`SEND_WINDOW`] further on: further behind than a report sent again
    /// lies, so the agent numbers its reports again from an older place, as
    /// one restored from an older database does (#1387). It acknowledges
    /// it, as a duplicate, and keeps nothing.
    Behind,
}

/// What the hub does with the report at `seq` of an epoch whose reports it
/// took through `held`, none when it took none of that epoch. A report the
/// agent let go of to make room leaves a gap, which is no reason to refuse
/// the next.
pub fn ingest(held: Option<ReportSeq>, seq: ReportSeq) -> Ingest {
    todo!()
}

#[cfg(test)]
mod tests;
