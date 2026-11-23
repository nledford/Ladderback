//! What the agent tells its hub live (§7.2, #1440) between the core's types
//! and their signed payloads: a snapshot of its plans, the hub's answer to
//! it, and a running operation's progress. A field that doesn't hold what
//! it must refuses the whole payload.

use api::sync::{LastRunPayload, PlanPayload, PlansAckPayload, PlansPayload, ProgressPayload};
use domain::fleet::{
    EpochSeconds, LastRun, LastRunOutcome, PlansReport, ReportedPlan, ReportedProgress, WireName,
};
use domain::{OperationId, PlanId};

use super::{WireRefused, uuid};

/// The bytes of `report`'s payload, which the agent signs.
pub fn plans_payload(_report: &PlansReport) -> Vec<u8> {
    todo!()
}

/// The snapshot a payload's bytes hold: more plans than a snapshot holds, a
/// plan ID that isn't one, an outcome this build doesn't know or a number
/// past `i64::MAX`, which the hub's store can't hold, refuses it.
pub fn read_plans(_bytes: &[u8]) -> Result<PlansReport, WireRefused> {
    todo!()
}

/// The bytes of the hub's `PlansAck`, which it signs.
pub fn plans_ack_payload() -> Vec<u8> {
    todo!()
}

/// Whether a payload's bytes hold a `PlansAck`.
pub fn read_plans_ack(_bytes: &[u8]) -> Result<(), WireRefused> {
    todo!()
}

/// The bytes of `progress`'s payload, which the agent signs.
pub fn progress_payload(_progress: &ReportedProgress) -> Vec<u8> {
    todo!()
}

/// The progress a payload's bytes hold: an operation ID that isn't a UUID,
/// a step or unit that isn't a wire name, or a number past `i64::MAX`
/// refuses it.
pub fn read_progress(_bytes: &[u8]) -> Result<ReportedProgress, WireRefused> {
    todo!()
}

/// `number`, when the hub's store can hold it.
fn held(number: u64) -> Result<u64, WireRefused> {
    i64::try_from(number)
        .map(|_| number)
        .map_err(|_| WireRefused)
}

#[cfg(test)]
mod tests;
