//! What the hub takes of a machine live (technical design §7.2; #1440):
//! the snapshot of its plans, which it keeps and acknowledges, and the
//! progress of an operation it runs, which it keeps and doesn't answer.
//! Each is kept in place of the one before and told to the browsers.

use api::{MachineProgressDto, OperationStep, ProgressUnit, UiEvent};
use domain::fleet::{EpochSeconds, ReportedProgress, WireName, plans_ack_kind};
use domain::{AgentId, Timestamp};

use super::{Answered, HubPairing, HubPairingError, MessageRefused};
use crate::pairing::wire::{plans_ack_payload, read_plans, read_progress};

impl HubPairing {
    /// Keeps `agent`'s snapshot of its plans in place of the one before,
    /// tells the browsers, and answers with a `PlansAck`.
    pub(super) async fn take_plans(
        &self,
        _agent: AgentId,
        _payload: &[u8],
    ) -> Result<Answered, HubPairingError> {
        todo!()
    }

    /// Keeps `agent`'s latest progress in place of the one before and
    /// tells the browsers; the agent waits for no answer.
    pub(super) async fn take_progress(
        &self,
        _agent: AgentId,
        _payload: &[u8],
    ) -> Result<Answered, HubPairingError> {
        todo!()
    }
}

/// `progress` as the API tells it: a step or unit this build doesn't know
/// as `Unknown`, which a page shows as no step and a plain number.
pub fn progress_dto(_progress: &ReportedProgress) -> MachineProgressDto {
    todo!()
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
