//! The hub's commands and the machine's answers between the core's types
//! and their payloads (`api::sync::commands`; §16, decision 176): a
//! command's ID, plan and operation as their text, its time in seconds since
//! the Unix epoch, and the hub's session as its tag.

use api::ErrorCode;
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

/// The kind and the bytes of the hub's command `request`, which it signs.
pub fn command_payload(request: &CommandRequest) -> (WireName, Vec<u8>) {
    let command_id = request.id.as_uuid().hyphenated().to_string();
    let requested_at = request.requested_at.seconds();
    let session = request.asked_by.as_ref().map(|tag| tag.as_str().to_owned());
    match &request.command {
        HubCommand::RunPlan { plan } => (
            run_plan_kind(),
            RunPlanPayload {
                command_id,
                requested_at,
                plan_id: plan.as_str().to_owned(),
                session,
            }
            .to_bytes(),
        ),
        HubCommand::CancelOp { op } => (
            cancel_op_kind(),
            CancelOpPayload {
                command_id,
                requested_at,
                op_id: op.as_uuid().hyphenated().to_string(),
                session,
            }
            .to_bytes(),
        ),
    }
}

/// The machine's answer to the hub's command, as `bytes` of its
/// `CommandResult` say. A refusal's code is one a machine answers with;
/// any other is read as [`ErrorCode::MachineRunRefused`], so a machine
/// can't have the hub say what it doesn't mean.
pub fn read_command_result(bytes: &[u8]) -> Result<(CommandId, CommandAnswer), WireRefused> {
    let payload = CommandResultPayload::from_bytes(bytes).map_err(|_| WireRefused)?;
    let id = CommandId::from_uuid(uuid(&payload.command_id)?);
    let operation = || {
        payload
            .op_id
            .as_deref()
            .ok_or(WireRefused)
            .and_then(uuid)
            .map(OperationId::from_uuid)
    };
    let answer = match payload.result.as_str() {
        "started" => CommandAnswer::Started(operation()?),
        "canceled" => CommandAnswer::Canceled(operation()?),
        "refused" => {
            // Read by its name alone: `refusal_code` keeps only the
            // handshake's refusals, and the list below keeps the machine's.
            let code = payload
                .code
                .and_then(|name| serde_json::from_value(serde_json::Value::String(name)).ok())
                .filter(|code| MACHINE_REFUSALS.contains(code))
                .unwrap_or(ErrorCode::MachineRunRefused);
            let retry_at = payload
                .retry_at
                .filter(|_| code == ErrorCode::CommandRateLimited)
                .and_then(|seconds| i64::try_from(seconds).ok())
                .and_then(|seconds| Timestamp::from_second(seconds).ok());
            CommandAnswer::Refused { code, retry_at }
        }
        _ => return Err(WireRefused),
    };
    Ok((id, answer))
}

/// The codes a machine refuses the hub's command with.
const MACHINE_REFUSALS: [ErrorCode; 11] = [
    ErrorCode::PlanUnknown,
    ErrorCode::OperationUnknown,
    ErrorCode::OperationNotRunning,
    ErrorCode::KeyFilesNotLeftOut,
    ErrorCode::PlanRunWaiting,
    ErrorCode::MachineCommandsOff,
    ErrorCode::CommandExpired,
    ErrorCode::CommandReplayed,
    ErrorCode::CommandRateLimited,
    ErrorCode::MachineRunRefused,
    ErrorCode::CommandBackupsOnly,
];

fn seconds(time: Timestamp) -> Option<u64> {
    EpochSeconds::from_timestamp(time).map(EpochSeconds::seconds)
}
