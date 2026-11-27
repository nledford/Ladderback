use std::collections::VecDeque;
use std::sync::Arc;

use api::ErrorCode;
use api::sync::HubFrame;
use domain::fleet::{
    Ack, AgentEnvelope, Binding, CommandId, EpochSeconds, FIRST_SEQUENCE, HubEnvelope,
    MAX_COMMANDS_IN_FLIGHT, Nonce, PayloadDigest, PlansReport, PublicKey, ReportedProgress,
    SignedMessage, SigningSeed, WireName, ack_kind, command_result_kind, fresh_on_hub_time,
    grants_kind, heartbeat_ack_kind, heartbeat_kind, plans_ack_kind, plans_kind, progress_kind,
    release_kind, released_kind, report_kind, repos_kind, withheld_kind,
};
use domain::maintenance::{LeaseEpoch, RepoGrants, RepositoryReports, RunningBackup};
use domain::{AgentId, RepositoryId, SignedDuration};

use super::wire::{
    HubMessage, agent_envelope_frame, command_result_payload, heartbeat_payload, hub_message,
    plans_payload, progress_payload, read_ack, read_command, read_grants, read_heartbeat_ack,
    read_plans_ack, read_released, read_withheld, release_payload, report_payload, repos_payload,
};
use crate::hub_commands::{CommandAnswer, ReceivedCommand};
use crate::ports::{Clock, HubLink, LinkError, OutboxEntry, Signing, SigningError};

/// Why a connection after the handshake went on no further. None carries a
/// seed (I4).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChannelError {
    /// The connection ended or didn't carry the message.
    Link(LinkError),
    /// A message the hub didn't sign on this connection, or signed out of
    /// sequence: nothing in it is acted on.
    Unsigned,
    /// A message whose expiry has passed on the hub's time, or lies further
    /// ahead than a hub signs.
    Expired,
    /// A message of a kind the agent doesn't take there, or whose payload
    /// isn't the kind's.
    OutOfTurn,
    /// The agent's count of its own messages ran out.
    SequenceSpent,
    Signing(SigningError),
}

impl From<LinkError> for ChannelError {
    fn from(error: LinkError) -> Self {
        Self::Link(error)
    }
}

impl From<SigningError> for ChannelError {
    fn from(error: SigningError) -> Self {
        Self::Signing(error)
    }
}

/// The connection to the hub after its `Welcome` (decision 120, "Signed
/// messages after the handshake"): each message the agent sends is signed
/// for this agent and connection with the next sequence number, and each
/// the hub sends is acted on only once its signature, its sequence number
/// and its expiry, on the hub's time, hold.
pub struct AgentChannel {
    link: Box<dyn HubLink>,
    signing: Arc<dyn Signing>,
    clock: Arc<dyn Clock>,
    seed: SigningSeed,
    bound: Bound,
    /// The sequence number of the agent's next message.
    next_sent: u64,
    /// The sequence number the hub's next message must carry.
    next_received: u64,
    /// When the hub signed its `Welcome`, on its clock.
    welcomed_at: EpochSeconds,
    /// When the agent sent `Hello`, on its monotonic clock.
    hello_sent: SignedDuration,
    /// The hub's commands taken while the agent waited for another message,
    /// which it hasn't answered, at most [`MAX_COMMANDS_IN_FLIGHT`].
    commands: VecDeque<ReceivedCommand>,
    /// The commands dropped unacted to make room for a newer one, which
    /// the agent answers expired, at most [`MAX_COMMANDS_IN_FLIGHT`].
    dropped: VecDeque<CommandId>,
}

/// What every signed message on one connection is bound to.
#[derive(Clone, Copy)]
pub(super) struct Bound {
    pub(super) hub_key: PublicKey,
    pub(super) agent_id: AgentId,
    pub(super) agent_nonce: Nonce,
    pub(super) hub_nonce: Nonce,
    pub(super) binding: Binding,
}

impl std::fmt::Debug for AgentChannel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AgentChannel")
            .field("next_sent", &self.next_sent)
            .field("next_received", &self.next_received)
            .finish_non_exhaustive()
    }
}

impl AgentChannel {
    /// The connection whose `Hello`, sent at `hello_sent`, the hub answered
    /// with a `Welcome` signed at `welcomed_at`; both carried sequence
    /// number 1.
    pub(super) fn after_welcome(
        link: Box<dyn HubLink>,
        signing: Arc<dyn Signing>,
        clock: Arc<dyn Clock>,
        seed: SigningSeed,
        bound: Bound,
        welcomed_at: EpochSeconds,
        hello_sent: SignedDuration,
    ) -> Self {
        let next = FIRST_SEQUENCE.saturating_add(1);
        Self {
            link,
            signing,
            clock,
            seed,
            bound,
            next_sent: next,
            next_received: next,
            welcomed_at,
            hello_sent,
            commands: VecDeque::new(),
            dropped: VecDeque::new(),
        }
    }

    /// Sends the report `entry`, signed with the next sequence number.
    pub async fn send_report(&mut self, entry: &OutboxEntry) -> Result<(), ChannelError> {
        self.send(report_kind(), report_payload(entry)).await
    }

    /// The hub's next acknowledgement, once it holds. A signed message of
    /// a kind this build doesn't take is counted and passed over.
    pub async fn receive_ack(&mut self) -> Result<Ack, ChannelError> {
        loop {
            let envelope = self.receive().await?;
            if envelope.kind == ack_kind() {
                return read_ack(&envelope.payload).map_err(|_| ChannelError::OutOfTurn);
            }
        }
    }

    /// Sends `reports` as every repository the agent uses
    /// (`ReposSnapshot`), signed with the next sequence number.
    pub async fn send_repos(&mut self, reports: &RepositoryReports) -> Result<(), ChannelError> {
        self.send(repos_kind(), repos_payload(reports)).await
    }

    /// The hub's next grants, once they hold; nothing when the hub withholds
    /// them in recovery mode (`GrantsWithheld`), so the machine keeps what
    /// it holds. A signed message of another kind is counted and passed
    /// over.
    pub async fn receive_grants(&mut self) -> Result<Option<RepoGrants>, ChannelError> {
        loop {
            let envelope = self.receive().await?;
            if envelope.kind == grants_kind() {
                return read_grants(&envelope.payload)
                    .map(Some)
                    .map_err(|_| ChannelError::OutOfTurn);
            }
            if envelope.kind == withheld_kind() {
                return read_withheld(&envelope.payload)
                    .map(|()| None)
                    .map_err(|_| ChannelError::OutOfTurn);
            }
        }
    }

    /// Sends the snapshot of the agent's `plans`, signed with the next
    /// sequence number.
    pub async fn send_plans(&mut self, plans: &PlansReport) -> Result<(), ChannelError> {
        self.send(plans_kind(), plans_payload(plans)).await
    }

    /// The hub's next `PlansAck`, once it holds. A signed message of a kind
    /// this build doesn't take is counted and passed over.
    pub async fn receive_plans_ack(&mut self) -> Result<(), ChannelError> {
        loop {
            let envelope = self.receive().await?;
            if envelope.kind == plans_ack_kind() {
                return read_plans_ack(&envelope.payload).map_err(|_| ChannelError::OutOfTurn);
            }
        }
    }

    /// Sends the release of the agent's lease on `repository` at `epoch`
    /// (`ReleaseAck`), signed with the next sequence number.
    pub async fn send_release(
        &mut self,
        repository: &RepositoryId,
        epoch: LeaseEpoch,
    ) -> Result<(), ChannelError> {
        self.send(release_kind(), release_payload(repository, epoch))
            .await
    }

    /// The release the hub next says it took, once its answer holds. A
    /// signed message of another kind is counted and passed over.
    pub async fn receive_released(&mut self) -> Result<(RepositoryId, LeaseEpoch), ChannelError> {
        loop {
            let envelope = self.receive().await?;
            if envelope.kind == released_kind() {
                return read_released(&envelope.payload).map_err(|_| ChannelError::OutOfTurn);
            }
        }
    }

    /// Sends `progress`, signed with the next sequence number; the hub
    /// answers it with nothing.
    pub async fn send_progress(&mut self, progress: &ReportedProgress) -> Result<(), ChannelError> {
        self.send(progress_kind(), progress_payload(progress)).await
    }

    /// Sends a heartbeat naming `running`, the backups the machine runs, or
    /// saying it can't tell, signed with the next sequence number (§16,
    /// decisions 159 and 166).
    pub async fn send_heartbeat(
        &mut self,
        running: Option<&[RunningBackup]>,
    ) -> Result<(), ChannelError> {
        self.send(heartbeat_kind(), heartbeat_payload(running))
            .await
    }

    /// The hub's next `HeartbeatAck`, once it holds: the repositories
    /// whose prune the hub holds, or `None` when it can't tell, as for a
    /// list this build can't read, which holds every prune and keeps the
    /// connection. A signed message of a kind this build doesn't take is
    /// counted and passed over.
    pub async fn receive_heartbeat_ack(
        &mut self,
    ) -> Result<Option<Vec<RepositoryId>>, ChannelError> {
        loop {
            let envelope = self.receive().await?;
            if envelope.kind == heartbeat_ack_kind() {
                return Ok(read_heartbeat_ack(&envelope.payload).unwrap_or(None));
            }
        }
    }

    /// The next command the hub sent that the agent hasn't answered: one
    /// it took while it waited for another message, or else the hub's
    /// next, whenever it comes. Cancel-safe: a listen given up loses no
    /// command. A signed message of a kind this build doesn't take is
    /// counted and passed over.
    pub async fn listen_command(&mut self) -> Result<ReceivedCommand, ChannelError> {
        loop {
            if let Some(command) = self.commands.pop_front() {
                return Ok(command);
            }
            let frame = self.link.listen().await?;
            // A message that isn't a command, unasked, is passed over.
            let _ = self.take(&frame)?;
        }
    }

    /// The next command the hub sent while the agent waited for another
    /// message, which it hasn't answered; none when there is none.
    pub fn buffered_command(&mut self) -> Option<ReceivedCommand> {
        self.commands.pop_front()
    }

    /// Answers each command the agent dropped unacted to make room for a
    /// newer one: it came too long before the agent could act on it, so
    /// its answer is [`ErrorCode::CommandExpired`].
    pub async fn answer_dropped(&mut self) -> Result<(), ChannelError> {
        while let Some(id) = self.dropped.pop_front() {
            let expired = CommandAnswer::Refused {
                code: ErrorCode::CommandExpired,
                retry_at: None,
            };
            self.send_command_result(id, &expired).await?;
        }
        Ok(())
    }

    /// Sends `answer` to the hub's command `id` (`CommandResult`), signed
    /// with the next sequence number.
    pub async fn send_command_result(
        &mut self,
        id: CommandId,
        answer: &CommandAnswer,
    ) -> Result<(), ChannelError> {
        self.send(command_result_kind(), command_result_payload(id, answer))
            .await
    }

    /// Sends a message of `kind` holding `payload`, signed with the next
    /// sequence number.
    async fn send(&mut self, kind: WireName, payload: Vec<u8>) -> Result<(), ChannelError> {
        let sequence = self.next_sent;
        let next = sequence.checked_add(1).ok_or(ChannelError::SequenceSpent)?;
        let bound = self.bound;
        let said = SignedMessage::AgentMessage {
            hub_key: bound.hub_key,
            agent_id: bound.agent_id,
            agent_nonce: bound.agent_nonce,
            hub_nonce: bound.hub_nonce,
            binding: bound.binding,
            sequence,
            kind: &kind,
            payload: PayloadDigest::of(&payload),
        };
        let signature = self.signing.sign(&self.seed, &said.to_bytes())?;
        self.next_sent = next;
        self.link
            .send(&agent_envelope_frame(&AgentEnvelope {
                kind,
                seq: sequence,
                payload,
                signature,
            }))
            .await?;
        Ok(())
    }

    /// The hub's next signed message other than a command, once it holds;
    /// each command before it is kept to be answered.
    async fn receive(&mut self) -> Result<HubEnvelope, ChannelError> {
        loop {
            let frame = self.link.receive().await?;
            if let Some(envelope) = self.take(&frame)? {
                return Ok(envelope);
            }
        }
    }

    /// The signed message `frame` holds, once it holds; none when it is a
    /// command, which is kept to be answered, with the hub's time it came
    /// at. One that finds [`MAX_COMMANDS_IN_FLIGHT`] held unanswered makes
    /// room by dropping the oldest, which the hub has stopped waiting for:
    /// it waits on no more at once.
    fn take(&mut self, frame: &HubFrame) -> Result<Option<HubEnvelope>, ChannelError> {
        let since = self.clock.monotonic().saturating_sub(self.hello_sent);
        let Ok(HubMessage::Signed(envelope)) = hub_message(frame) else {
            return Err(ChannelError::OutOfTurn);
        };
        let bound = self.bound;
        let signed = SignedMessage::HubMessage {
            hub_key: bound.hub_key,
            agent_id: bound.agent_id,
            agent_nonce: bound.agent_nonce,
            hub_nonce: bound.hub_nonce,
            binding: bound.binding,
            sequence: envelope.seq,
            expires_at: envelope.expires_at,
            kind: &envelope.kind,
            payload: PayloadDigest::of(&envelope.payload),
        };
        if envelope.seq != self.next_received
            || !self
                .signing
                .verify(&bound.hub_key, &signed.to_bytes(), &envelope.signature)
        {
            return Err(ChannelError::Unsigned);
        }
        self.next_received = self.next_received.saturating_add(1);
        if !fresh_on_hub_time(self.welcomed_at, since, envelope.expires_at) {
            return Err(ChannelError::Expired);
        }
        let request =
            read_command(&envelope.kind, &envelope.payload).map_err(|_| ChannelError::OutOfTurn)?;
        let Some(request) = request else {
            return Ok(Some(envelope));
        };
        if self.commands.len() >= MAX_COMMANDS_IN_FLIGHT
            && let Some(oldest) = self.commands.pop_front()
        {
            if self.dropped.len() >= MAX_COMMANDS_IN_FLIGHT {
                // Older still: no one waits for its answer.
                self.dropped.pop_front();
            }
            self.dropped.push_back(oldest.request.id);
        }
        self.commands.push_back(ReceivedCommand {
            request,
            welcomed_at: self.welcomed_at,
            since,
            hello_sent: self.hello_sent,
        });
        Ok(None)
    }
}
