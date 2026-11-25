//! What the agent tells its hub live (§7.2, #1440) between the core's types
//! and their signed payloads: a snapshot of its plans, the hub's answer to
//! it, a running operation's progress, and the heartbeat that holds its
//! connection open and the hub's answer to it (decision 159). A field that
//! doesn't hold what it must refuses the whole payload.

use api::sync::{
    HeartbeatAckPayload, HeartbeatPayload, LastRunPayload, PlanPayload, PlansAckPayload,
    PlansPayload, ProgressPayload,
};
use domain::fleet::{
    EpochSeconds, LastRun, LastRunOutcome, PlansReport, ReportedPlan, ReportedProgress, WireName,
};
use domain::{OperationId, PlanId, Timestamp};

use super::{WireRefused, uuid};

/// The bytes of `report`'s payload, which the agent signs.
pub fn plans_payload(report: &PlansReport) -> Vec<u8> {
    PlansPayload {
        plans: report
            .plans()
            .iter()
            .map(|plan| PlanPayload {
                plan: plan.plan.as_str().to_owned(),
                last: plan.last.map(|last| LastRunPayload {
                    at: last.at.seconds(),
                    outcome: last.outcome.wire_name().to_owned(),
                }),
                next: plan.next_run.map(EpochSeconds::seconds),
            })
            .collect(),
        left_out: report.left_out(),
    }
    .to_bytes()
}

/// The snapshot a payload's bytes hold: more plans than a snapshot holds, a
/// plan ID that isn't one, an outcome this build doesn't know, a number
/// past `i64::MAX`, which the hub's store can't hold, or a time past year
/// 9999, which no timestamp holds, refuses it.
pub fn read_plans(bytes: &[u8]) -> Result<PlansReport, WireRefused> {
    let payload = PlansPayload::from_bytes(bytes).map_err(|_| WireRefused)?;
    let plans = payload
        .plans
        .into_iter()
        .map(|plan| {
            Ok(ReportedPlan {
                plan: PlanId::parse(&plan.plan).map_err(|_| WireRefused)?,
                last: plan
                    .last
                    .map(|last| {
                        Ok(LastRun {
                            at: time(last.at)?,
                            outcome: LastRunOutcome::from_wire_name(&last.outcome)
                                .ok_or(WireRefused)?,
                        })
                    })
                    .transpose()?,
                next_run: plan.next.map(time).transpose()?,
            })
        })
        .collect::<Result<Vec<_>, WireRefused>>()?;
    PlansReport::from_parts(plans, held(payload.left_out)?).ok_or(WireRefused)
}

/// The bytes of the hub's `PlansAck`, which it signs.
pub fn plans_ack_payload() -> Vec<u8> {
    PlansAckPayload {}.to_bytes()
}

/// Whether a payload's bytes hold a `PlansAck`.
pub fn read_plans_ack(bytes: &[u8]) -> Result<(), WireRefused> {
    PlansAckPayload::from_bytes(bytes)
        .map(|_| ())
        .map_err(|_| WireRefused)
}

/// The bytes of the agent's `Heartbeat`, which it signs.
pub fn heartbeat_payload() -> Vec<u8> {
    HeartbeatPayload {}.to_bytes()
}

/// Whether a payload's bytes hold a `Heartbeat`.
pub fn read_heartbeat(bytes: &[u8]) -> Result<(), WireRefused> {
    HeartbeatPayload::from_bytes(bytes)
        .map(|_| ())
        .map_err(|_| WireRefused)
}

/// The bytes of the hub's `HeartbeatAck`, which it signs.
pub fn heartbeat_ack_payload() -> Vec<u8> {
    HeartbeatAckPayload {}.to_bytes()
}

/// Whether a payload's bytes hold a `HeartbeatAck`.
pub fn read_heartbeat_ack(bytes: &[u8]) -> Result<(), WireRefused> {
    HeartbeatAckPayload::from_bytes(bytes)
        .map(|_| ())
        .map_err(|_| WireRefused)
}

/// The bytes of `progress`'s payload, which the agent signs.
pub fn progress_payload(progress: &ReportedProgress) -> Vec<u8> {
    ProgressPayload {
        op_id: progress.op_id.as_uuid().to_string(),
        step: progress.step.as_ref().map(|step| step.as_str().to_owned()),
        unit: progress.unit.as_str().to_owned(),
        current: progress.current,
        total: progress.total,
        at: progress.at.seconds(),
    }
    .to_bytes()
}

/// The progress a payload's bytes hold: an operation ID that isn't a UUID,
/// a step or unit that isn't a wire name, a number past `i64::MAX` or a
/// time past year 9999 refuses it.
pub fn read_progress(bytes: &[u8]) -> Result<ReportedProgress, WireRefused> {
    let payload = ProgressPayload::from_bytes(bytes).map_err(|_| WireRefused)?;
    Ok(ReportedProgress {
        op_id: OperationId::from_uuid(uuid(&payload.op_id)?),
        step: payload
            .step
            .map(|step| WireName::parse(&step).map_err(|_| WireRefused))
            .transpose()?,
        unit: WireName::parse(&payload.unit).map_err(|_| WireRefused)?,
        current: held(payload.current)?,
        total: payload.total.map(held).transpose()?,
        at: time(payload.at)?,
    })
}

/// `number`, when the hub's store can hold it.
fn held(number: u64) -> Result<u64, WireRefused> {
    i64::try_from(number)
        .map(|_| number)
        .map_err(|_| WireRefused)
}

/// `seconds` since the Unix epoch, when they are a timestamp's: the hub
/// shows each time it keeps.
fn time(seconds: u64) -> Result<EpochSeconds, WireRefused> {
    i64::try_from(seconds)
        .ok()
        .filter(|seconds| Timestamp::from_second(*seconds).is_ok())
        .map(|_| EpochSeconds::from_seconds(seconds))
        .ok_or(WireRefused)
}

#[cfg(test)]
mod tests;
