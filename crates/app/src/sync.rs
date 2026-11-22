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
use domain::fleet::{
    AckRefused, AgentReport, Approval, EpochSeconds, Happening, Hostname, MAX_OUTBOX_REPORTS,
    MachineZone, SEND_WINDOW, WireName, check_ack,
};
use domain::{AgentId, OperationId, SignedDuration, Timestamp, Uuid};

use crate::pairing::{ChannelError, ConnectFailure, ConnectToHub};
use crate::ports::{Appended, Clock, Outbox, StoreError};

/// How long the agent waits before it tries its hub again after the first
/// failure; each failure after doubles it, up to [`LONGEST_BACKOFF`].
pub const FIRST_BACKOFF: SignedDuration = SignedDuration::from_secs(1);
/// The longest the agent waits between tries.
pub const LONGEST_BACKOFF: SignedDuration = SignedDuration::from_secs(300);
/// How often the agent looks for reports in its outbox while it has
/// nothing to send, and how long it waits after a session that sent them.
pub const POLL: SignedDuration = SignedDuration::from_secs(2);

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
        let (operation, happening, at) = match event {
            UiEvent::OperationStarted {
                operation,
                started_at,
                ..
            } => (operation, Happening::Started, started_at),
            UiEvent::OperationSucceeded {
                operation,
                finished_at,
            } => (operation, Happening::Succeeded, finished_at),
            UiEvent::OperationFailed {
                operation,
                code,
                finished_at,
                ..
            } => (
                operation,
                Happening::Failed {
                    code: WireName::parse(&api::sync::code_name(*code)).ok()?,
                },
                finished_at,
            ),
            _ => return None,
        };
        Some(AgentReport {
            op_id: OperationId::from_uuid(Uuid::try_parse(operation).ok()?),
            happening,
            at: EpochSeconds::from_timestamp(at.parse::<Timestamp>().ok()?)?,
        })
    }

    /// Puts `event`'s report into the outbox, none for an event that makes
    /// none. A full outbox loses no report silently: the start it gives up
    /// for this one, or this one when it refuses it, is logged as a
    /// warning. Neither fails the operation that made the event.
    pub async fn record(&self, event: &UiEvent) -> Result<Option<Appended>, StoreError> {
        let Some(report) = Self::report_of(event) else {
            return Ok(None);
        };
        let appended = self.outbox.append(&report).await?;
        let operation = report.op_id.as_uuid();
        match appended {
            Appended::Kept(_) => {}
            Appended::KeptGivingUp { gave_up, .. } => log::warn!(
                "the outbox holds {MAX_OUTBOX_REPORTS} reports the hub hasn't acknowledged: \
                 it gave up its oldest start, at place {}, to keep a report of operation \
                 {operation}",
                gave_up.get(),
            ),
            Appended::Refused => log::warn!(
                "the outbox holds {MAX_OUTBOX_REPORTS} ends the hub hasn't acknowledged, or \
                 ran out of places: a report of operation {operation} wasn't kept"
            ),
        }
        Ok(Some(appended))
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
    /// order, at most [`SEND_WINDOW`] ahead of the hub's acknowledgements,
    /// and returns once the hub acknowledged the last of them. A pending
    /// agent sends nothing.
    pub async fn sync_once(&self) -> Result<Synced, SyncFailure> {
        let (connected, mut channel) = self
            .connect
            .connect_channel(self.agent_id, self.hostname.clone(), self.zone.clone())
            .await
            .map_err(SyncFailure::Connect)?;
        if connected.approval == Approval::Pending {
            return Ok(Synced::Pending);
        }
        let mut sent = 0;
        // Each window starts after the last place acknowledged, so a session
        // goes through the outbox once even if acknowledging kept nothing.
        let mut after = None;
        loop {
            let window = self
                .outbox
                .unacknowledged(after, SEND_WINDOW)
                .await
                .map_err(SyncFailure::Store)?;
            let (Some(first), Some(last)) = (window.first(), window.last()) else {
                return Ok(Synced::Replayed { sent });
            };
            let (epoch, through) = (first.epoch, last.seq);
            after = Some(through);
            for entry in &window {
                channel
                    .send_report(entry)
                    .await
                    .map_err(SyncFailure::Channel)?;
                sent += 1;
            }
            loop {
                let ack = channel.receive_ack().await.map_err(SyncFailure::Channel)?;
                check_ack(epoch, Some(through), ack).map_err(SyncFailure::Ack)?;
                self.outbox
                    .acknowledge(ack.epoch, ack.seq)
                    .await
                    .map_err(SyncFailure::Store)?;
                if ack.seq >= through {
                    break;
                }
            }
        }
    }

    /// Sessions for as long as the agent runs: whenever its outbox holds a
    /// report the hub hasn't acknowledged, a session replays it, and the
    /// outbox is looked at again after [`POLL`]; a failed session, or one
    /// the hub holds pending, is tried again after [`backoff`]. It never
    /// touches the agent's operations, so an agent that can't reach its hub
    /// backs up on schedule all the same.
    pub async fn run(&self) {
        let mut wait = None;
        // Each streak of failures, and of unreadable outboxes, is logged
        // once, at its first.
        let mut failing = false;
        let mut unreadable = false;
        loop {
            let waiting = match self.outbox.unacknowledged(None, 1).await {
                Ok(waiting) => {
                    unreadable = false;
                    !waiting.is_empty()
                }
                Err(error) => {
                    if !unreadable {
                        log::warn!("the outbox couldn't be read: {error:?}");
                    }
                    unreadable = true;
                    false
                }
            };
            if !waiting {
                self.clock.sleep(POLL).await;
                continue;
            }
            let failure = match self.sync_once().await {
                Ok(Synced::Replayed { .. }) => {
                    wait = None;
                    failing = false;
                    // Never straight into the next session: an outbox that
                    // kept what the hub acknowledged would otherwise have
                    // the loop replay it without pause.
                    self.clock.sleep(POLL).await;
                    continue;
                }
                Ok(Synced::Pending) => None,
                Err(failure) => Some(failure),
            };
            if !failing && let Some(failure) = &failure {
                log::warn!("the agent's reports didn't reach its hub: {failure:?}");
            }
            failing = failure.is_some();
            let next = backoff(wait);
            wait = Some(next);
            self.clock.sleep(next).await;
        }
    }
}

/// How long to wait after a failed session that followed a wait of
/// `previous`, none after a session that held: [`FIRST_BACKOFF`], doubled
/// each time up to [`LONGEST_BACKOFF`].
pub fn backoff(previous: Option<SignedDuration>) -> SignedDuration {
    previous.map_or(FIRST_BACKOFF, |previous| {
        previous
            .checked_mul(2)
            .map_or(LONGEST_BACKOFF, |doubled| doubled.min(LONGEST_BACKOFF))
    })
}

#[cfg(test)]
mod tests;
