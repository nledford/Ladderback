//! An agent's reports to its hub (technical design §7.2, §4.2; §16, decision
//! 120, "Signed messages after the handshake"): what the agent's operations
//! did enters its persistent outbox as it happens, whether or not the hub
//! can be reached, and a sync session replays what the hub hasn't
//! acknowledged, in order, once the hub has approved the agent. Delivery is
//! at least once: a report leaves the outbox only when the hub's signed
//! acknowledgement covers it, and the hub takes a report it took before as
//! a duplicate.

use std::sync::Arc;

use api::UiEvent;
use domain::fleet::{AckRefused, AgentReport, Hostname, MachineZone};
use domain::{AgentId, SignedDuration};

use crate::pairing::{ChannelError, ConnectFailure, ConnectToHub};
use crate::ports::{Appended, Clock, Outbox, StoreError};

/// How long the agent waits before it tries its hub again after the first
/// failure; each failure after doubles it, up to [`LONGEST_BACKOFF`].
pub const FIRST_BACKOFF: SignedDuration = SignedDuration::from_secs(1);
/// The longest the agent waits between tries.
pub const LONGEST_BACKOFF: SignedDuration = SignedDuration::from_secs(300);
/// How often a connected agent, or one with nothing to send, looks for new
/// reports in its outbox.
pub const POLL: SignedDuration = SignedDuration::from_secs(2);
/// How long a connected agent with nothing to send keeps its connection
/// before it closes it; it connects again when it has a report.
pub const IDLE_CLOSE: SignedDuration = SignedDuration::from_secs(300);

/// Puts each report the hub needs into the outbox as the agent's operations
/// make it.
pub struct RecordReports {
    outbox: Arc<dyn Outbox>,
}

impl RecordReports {
    pub fn new(outbox: Arc<dyn Outbox>) -> Self {
        Self { outbox }
    }

    /// The report `event` makes: an operation's start or end; none for its
    /// progress, its warnings or anything else the hub doesn't need, nor
    /// for an event whose ID or time doesn't read.
    pub fn report_of(event: &UiEvent) -> Option<AgentReport> {
        todo!()
    }

    /// Puts `event`'s report into the outbox, none for an event that makes
    /// none. A refusal is for the caller to log: it never fails the
    /// operation that made the event.
    pub async fn record(&self, event: &UiEvent) -> Result<Option<Appended>, StoreError> {
        todo!()
    }
}

/// What a sync session came to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Synced {
    /// The hub acknowledged every report the session sent, `sent` of them.
    Replayed { sent: u64 },
    /// The hub holds the agent pending: it sent nothing, and the reports
    /// wait in the outbox for the approval.
    Pending,
}

/// Why a sync session ended before the hub acknowledged what it sent. The
/// reports stay in the outbox for the next session.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SyncFailure {
    Connect(ConnectFailure),
    /// The connection ended, or carried a message the agent acts on none of.
    Channel(ChannelError),
    /// The hub acknowledged what this session didn't send.
    Ack(AckRefused),
    Store(StoreError),
}

/// The agent's side of the sync: one session at a time with the hub it
/// pinned, replaying its outbox.
pub struct SyncWithHub {
    connect: ConnectToHub,
    outbox: Arc<dyn Outbox>,
    clock: Arc<dyn Clock>,
    agent_id: AgentId,
    hostname: Hostname,
    zone: Option<MachineZone>,
}

impl SyncWithHub {
    pub fn new(
        connect: ConnectToHub,
        outbox: Arc<dyn Outbox>,
        clock: Arc<dyn Clock>,
        agent_id: AgentId,
        hostname: Hostname,
        zone: Option<MachineZone>,
    ) -> Self {
        Self {
            connect,
            outbox,
            clock,
            agent_id,
            hostname,
            zone,
        }
    }

    /// One session: connects, sends every report not yet acknowledged, in
    /// order, and returns once the hub acknowledged the last of them.
    pub async fn sync_once(&self) -> Result<Synced, SyncFailure> {
        todo!()
    }

    /// Sessions for as long as the agent runs: each replays the outbox and
    /// keeps the connection while reports come, for up to [`IDLE_CLOSE`]
    /// with none; a failed one is tried again after [`backoff`]. It never
    /// touches the agent's operations, so an agent that can't reach its hub
    /// backs up on schedule all the same.
    pub async fn run(&self) {
        todo!()
    }
}

/// How long to wait after a failed session that followed a wait of
/// `previous`, none after a session that held: [`FIRST_BACKOFF`], doubled
/// each time up to [`LONGEST_BACKOFF`].
pub fn backoff(previous: Option<SignedDuration>) -> SignedDuration {
    todo!()
}

#[cfg(test)]
mod tests;
