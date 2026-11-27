//! The hub's commands and the machine's answers between the core's types
//! and their payloads (`api::sync::commands`; §16, decision 176): a
//! command's ID, plan and operation as their text, its time in seconds since
//! the Unix epoch, and the hub's session as its tag.

use api::sync::{CancelOpPayload, CommandResultPayload, RunPlanPayload, code_name};
use domain::access::SessionTag;
use domain::fleet::{
    CommandId, CommandRequest, EpochSeconds, HubCommand, WireName, cancel_op_kind, run_plan_kind,
};
use domain::{OperationId, PlanId, Timestamp};

use super::{WireRefused, uuid};
use crate::hub_commands::CommandAnswer;

/// The command a signed message of `kind` holding `bytes` asks for; none
/// when `kind` isn't a command's.
pub fn read_command(kind: &WireName, bytes: &[u8]) -> Result<Option<CommandRequest>, WireRefused> {
    let session = |tag: Option<String>| {
        tag.map(|tag| SessionTag::parse(&tag).map_err(|_| WireRefused))
            .transpose()
    };
    let request = if *kind == run_plan_kind() {
        let payload = RunPlanPayload::from_bytes(bytes).map_err(|_| WireRefused)?;
        CommandRequest {
            id: CommandId::from_uuid(uuid(&payload.command_id)?),
            requested_at: EpochSeconds::from_seconds(payload.requested_at),
            asked_by: session(payload.session)?,
            command: HubCommand::RunPlan {
                plan: PlanId::parse(&payload.plan_id).map_err(|_| WireRefused)?,
            },
        }
    } else if *kind == cancel_op_kind() {
        let payload = CancelOpPayload::from_bytes(bytes).map_err(|_| WireRefused)?;
        CommandRequest {
            id: CommandId::from_uuid(uuid(&payload.command_id)?),
            requested_at: EpochSeconds::from_seconds(payload.requested_at),
            asked_by: session(payload.session)?,
            command: HubCommand::CancelOp {
                op: OperationId::from_uuid(uuid(&payload.op_id)?),
            },
        }
    } else {
        return Ok(None);
    };
    Ok(Some(request))
}

/// The bytes of the machine's `answer` to the command `id`, which it signs.
pub fn command_result_payload(id: CommandId, answer: &CommandAnswer) -> Vec<u8> {
    let text = |operation: &OperationId| Some(operation.as_uuid().hyphenated().to_string());
    let (result, op_id, code, retry_at) = match answer {
        CommandAnswer::Started(operation) => ("started", text(operation), None, None),
        CommandAnswer::Canceled(operation) => ("canceled", text(operation), None, None),
        CommandAnswer::Refused { code, retry_at } => (
            "refused",
            None,
            Some(code_name(*code)),
            retry_at.and_then(seconds),
        ),
    };
    CommandResultPayload {
        command_id: id.as_uuid().hyphenated().to_string(),
        result: result.to_owned(),
        op_id,
        code,
        retry_at,
    }
    .to_bytes()
}

fn seconds(time: Timestamp) -> Option<u64> {
    EpochSeconds::from_timestamp(time).map(EpochSeconds::seconds)
}
