//! The agent's outbox and the hub's ingest of what it replays (technical
//! design §7.2; §16, decision 120, "Signed messages after the handshake"):
//! an agent keeps each report its hub needs, how one of its operations
//! started and ended, numbered in order as it enters, until the hub
//! acknowledges it; the hub takes each report once, however often it comes.
//! Rules only: the store keeps the reports, and the sync session carries
//! them.

use std::num::NonZeroU64;

use uuid::Uuid;

use super::{EpochSeconds, MAX_EXPIRY_AHEAD, WireName};
use crate::{OperationId, SignedDuration};

/// The most reports an agent's outbox holds. A start makes room by giving
/// up the oldest start; an end is never given up ([`admit`]).
pub const MAX_OUTBOX_REPORTS: u64 = 10_000;

/// The most reports the hub keeps of one agent, its oldest let go first.
pub const MAX_REPORTS_HELD: u64 = 10_000;

/// The most outbox epochs the hub remembers of one agent, the one it took a
/// report of longest ago forgotten first.
pub const MAX_EPOCHS_HELD: u64 = 8;

/// How many places of its outbox the agent sends before it waits for the
/// hub's acknowledgements of them: a window holds the reports of at most
/// that many places, fewer where a full outbox left gaps.
pub const SEND_WINDOW: usize = 32;

/// Whether a hub's message that expires at `expires_at` holds when it
/// arrives `since` after the agent sent the `Hello` the hub answered with a
/// `Welcome` signed at `signed_at`, on the agent's monotonic clock. The
/// hub's time is then at most `signed_at` plus `since`: a message that
/// expired by then, or that expires further ahead of it than a hub signs,
/// is refused, whatever the agent's wall clock says (decision 120, "Expiry
/// is judged on the hub's time").
pub fn fresh_on_hub_time(
    signed_at: EpochSeconds,
    since: SignedDuration,
    expires_at: EpochSeconds,
) -> bool {
    let hub_now_ms = i128::from(signed_at.seconds())
        .saturating_mul(1_000)
        .saturating_add(since.as_millis().max(0));
    let expires_ms = i128::from(expires_at.seconds()).saturating_mul(1_000);
    let furthest_ms = hub_now_ms.saturating_add(i128::from(MAX_EXPIRY_AHEAD) * 1_000);
    hub_now_ms < expires_ms && expires_ms <= furthest_ms
}

/// The kind of the agent's message that carries one report.
pub fn report_kind() -> WireName {
    WireName::known("OpEvent")
}

/// The kind of the hub's acknowledgement of the reports through one.
pub fn ack_kind() -> WireName {
    WireName::known("Ack")
}

/// The kind of the agent's report of every repository it uses (§7.2).
pub fn repos_kind() -> WireName {
    WireName::known("ReposSnapshot")
}

/// The kind of the hub's signed grants, its answer to `ReposSnapshot`.
pub fn grants_kind() -> WireName {
    WireName::known("RepoGrants")
}

/// The kind of the hub's answer to `ReposSnapshot` in recovery mode: it
/// grants and renews nothing, and the machine keeps what it holds until it
/// runs out (§16, decision 137).
pub fn withheld_kind() -> WireName {
    WireName::known("GrantsWithheld")
}

/// The kind of the agent's release of a lease it dropped (§7.3).
pub fn release_kind() -> WireName {
    WireName::known("ReleaseAck")
}

/// The kind of the hub's answer to `ReleaseAck`.
pub fn released_kind() -> WireName {
    WireName::known("Released")
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
        self.0.checked_add(1).map(Self)
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
    /// It was canceled before it ended on its own.
    Canceled,
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
        !matches!(self.happening, Happening::Started)
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
/// `oldest_start`, does with a new report, which would take `next` (none
/// when the numbers ran out). Under [`MAX_OUTBOX_REPORTS`] it is appended;
/// at it, the oldest start gives way, whatever the new report is; full of
/// ends, the report is refused, so no end is ever given up for anything.
pub fn admit(
    held: u64,
    oldest_start: Option<ReportSeq>,
    next: Option<ReportSeq>,
) -> OutboxAdmission {
    if next.is_none() {
        return OutboxAdmission::Refuse;
    }
    if held < MAX_OUTBOX_REPORTS {
        return OutboxAdmission::Append;
    }
    match oldest_start {
        Some(oldest) => OutboxAdmission::EvictThenAppend(oldest),
        None => OutboxAdmission::Refuse,
    }
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
    if ack.epoch != epoch {
        return Err(AckRefused::OtherEpoch);
    }
    match sent_through {
        Some(sent) if ack.seq <= sent => Ok(()),
        _ => Err(AckRefused::NotSent),
    }
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
/// the next. One taken before is [`Ingest::Behind`] when it lies a
/// [`SEND_WINDOW`] or more behind `held`: the agent sends at most that many
/// before their acknowledgements, so no report sent again lies that far.
pub fn ingest(held: Option<ReportSeq>, seq: ReportSeq) -> Ingest {
    match held {
        Some(held) if seq <= held => {
            let behind = held.get().saturating_sub(seq.get());
            if u64::try_from(SEND_WINDOW).is_ok_and(|window| behind >= window) {
                Ingest::Behind
            } else {
                Ingest::Duplicate
            }
        }
        _ => Ingest::New,
    }
}

#[cfg(test)]
mod tests;
