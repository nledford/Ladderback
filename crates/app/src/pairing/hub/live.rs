//! What the hub takes of a machine live (technical design §7.2; #1440):
//! the snapshot of its plans, which it keeps and acknowledges, the
//! progress of an operation it runs, which it keeps and doesn't answer,
//! and its heartbeat, which says the backups it runs and which the hub
//! answers with the prunes it holds for it (decisions 159 and 166). Each
//! snapshot and progress is kept in place of the one before and told to
//! the browsers.

use std::sync::PoisonError;

use api::{MachineProgressDto, OperationStep, ProgressUnit, UiEvent};
use domain::fleet::{EpochSeconds, ReportedProgress, WireName, heartbeat_ack_kind, plans_ack_kind};
use domain::maintenance::{RunningBackup, held_prunes};
use domain::{AgentId, RepositoryId, Timestamp};

use super::{Answered, HubPairing, HubPairingError, MessageRefused};
use crate::pairing::wire::{
    heartbeat_ack_payload, plans_ack_payload, read_heartbeat, read_plans, read_progress,
};

impl HubPairing {
    /// Keeps what `agent`'s heartbeat `payload` says it runs, and answers
    /// with a `HeartbeatAck` naming the prunes the hub holds for it.
    pub(super) fn take_heartbeat(&self, agent: AgentId, payload: &[u8]) -> Answered {
        let Ok(running) = read_heartbeat(payload) else {
            return Err(MessageRefused::Unexpected);
        };
        self.heard_running(agent, running);
        let held = self.prunes_held_for(agent);
        Ok(Some((
            heartbeat_ack_kind(),
            heartbeat_ack_payload(held.as_deref()),
        )))
    }

    /// Keeps `running`, the backups `agent`'s latest heartbeat says it
    /// runs, in place of what the one before said: `None` when it didn't
    /// say, which holds every other machine's prune while `agent` is
    /// online (§16, decision 166).
    pub fn heard_running(&self, agent: AgentId, running: Option<Vec<RunningBackup>>) {
        let mut kept = self
            .presence
            .running
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        match running {
            Some(running) => {
                kept.insert(agent, running);
            }
            None => {
                kept.remove(&agent);
            }
        }
    }

    /// The repositories whose prune the hub holds for `agent`, now: those
    /// another machine it holds a connection of is backing up to, or
    /// `None` when it can't tell which, which holds every prune
    /// (`domain::maintenance::held_prunes`). A machine offline holds
    /// nothing.
    pub fn prunes_held_for(&self, agent: AgentId) -> Option<Vec<RepositoryId>> {
        let others = self.online_except(agent);
        let running = self
            .presence
            .running
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        held_prunes(
            others
                .iter()
                .map(|other| running.get(other).map(Vec::as_slice)),
        )
    }

    /// Keeps `agent`'s snapshot of its plans in place of the one before,
    /// tells the browsers, and answers with a `PlansAck`.
    pub(super) async fn take_plans(
        &self,
        agent: AgentId,
        payload: &[u8],
    ) -> Result<Answered, HubPairingError> {
        let (Some(inbox), Ok(plans)) = (&self.inbox, read_plans(payload)) else {
            return Ok(Err(MessageRefused::Unexpected));
        };
        inbox.keep_plans(agent, &plans).await?;
        self.tell(UiEvent::MachinePlans {
            machine: agent.as_uuid().to_string(),
        })
        .await;
        Ok(Ok(Some((plans_ack_kind(), plans_ack_payload()))))
    }

    /// Keeps `agent`'s latest progress in place of the one before and
    /// tells the browsers; the agent waits for no answer.
    pub(super) async fn take_progress(
        &self,
        agent: AgentId,
        payload: &[u8],
    ) -> Result<Answered, HubPairingError> {
        let (Some(inbox), Ok(progress)) = (&self.inbox, read_progress(payload)) else {
            return Ok(Err(MessageRefused::Unexpected));
        };
        inbox.keep_progress(agent, &progress).await?;
        self.tell(UiEvent::MachineProgress {
            machine: agent.as_uuid().to_string(),
            progress: progress_dto(&progress),
        })
        .await;
        Ok(Ok(None))
    }
}

/// `progress` as the API tells it: a step or unit this build doesn't know
/// as `Unknown`, which a page shows as no step and a plain number.
pub fn progress_dto(progress: &ReportedProgress) -> MachineProgressDto {
    MachineProgressDto {
        operation: progress.op_id.as_uuid().to_string(),
        step: progress
            .step
            .as_ref()
            .map(|step| known(step).unwrap_or(OperationStep::Unknown)),
        unit: known(&progress.unit).unwrap_or(ProgressUnit::Unknown),
        current: progress.current,
        total: progress.total,
        at: time_of(progress.at).unwrap_or_default(),
    }
}

/// The step or unit `name` names, read as the API reads its kebab-case
/// name; none for a name this build doesn't know.
fn known<T: serde::de::DeserializeOwned>(name: &WireName) -> Option<T> {
    serde_json::from_value(serde_json::Value::String(name.as_str().to_owned())).ok()
}

/// A reported time as the API writes times; none past what a timestamp
/// holds, which the hub never keeps (`wire::read_progress`).
pub fn time_of(at: EpochSeconds) -> Option<String> {
    let seconds = i64::try_from(at.seconds()).ok()?;
    Timestamp::from_second(seconds)
        .ok()
        .map(|time| time.to_string())
}
