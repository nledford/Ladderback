//! What an agent tells its hub besides its reports (technical design §7.2;
//! #1440): a snapshot of its plans, each with its last run and its next,
//! sent when it changed, and the latest progress of the operation it runs,
//! sent at most every [`PROGRESS_EVERY`] and never kept on the machine.

use std::sync::{Arc, Mutex, PoisonError};

use api::UiEvent;
use domain::fleet::{EpochSeconds, LastRun, PlansReport, ReportedPlan, ReportedProgress, WireName};
use domain::{OperationId, SignedDuration, Timestamp, Uuid};

use crate::ports::ScheduleStore;
use crate::status::{ReadStatus, StatusError};

/// How often the agent reads its plans again to see whether they changed
/// since it last told its hub: a next run planned anew, a run queued.
pub const PLANS_CHECK: SignedDuration = SignedDuration::from_secs(60);
/// The shortest time between two progresses the agent sends its hub.
pub const PROGRESS_EVERY: SignedDuration = SignedDuration::from_secs(5);

/// Reads the agent's plans as its hub is told them: each plan's last run,
/// as the status summary tells it, and its next planned run.
pub struct ReadPlans {
    status: ReadStatus,
    schedules: Arc<dyn ScheduleStore>,
}

impl ReadPlans {
    pub fn new(status: ReadStatus, schedules: Arc<dyn ScheduleStore>) -> Self {
        Self { status, schedules }
    }

    /// The agent's plans as they stand now, in its configuration's order.
    pub async fn read(&self) -> Result<PlansReport, StatusError> {
        let status = self.status.read().await?;
        let planned = self
            .schedules
            .planned_runs()
            .await
            .map_err(StatusError::Store)?;
        let plans = status
            .plans
            .into_iter()
            .map(|standing| ReportedPlan {
                last: standing.last.as_ref().and_then(|last| {
                    LastRun::of(
                        &last.status,
                        last.queued_at,
                        last.started_at,
                        last.finished_at,
                    )
                }),
                last_success: standing.last_success.as_ref().and_then(|success| {
                    let run = &success.operation;
                    EpochSeconds::from_timestamp(
                        run.finished_at.or(run.started_at).unwrap_or(run.queued_at),
                    )
                }),
                next_run: planned
                    .iter()
                    .find(|run| run.plan == standing.plan)
                    .and_then(|run| EpochSeconds::from_timestamp(run.at)),
                plan: standing.plan,
            })
            .collect();
        Ok(PlansReport::of(plans))
    }
}

/// The latest progress of the operation the agent runs, as its events tell
/// it: kept from its progress events, and let go of when it ends.
#[derive(Debug, Default)]
pub struct LiveProgress {
    latest: Mutex<Option<ReportedProgress>>,
}

impl LiveProgress {
    pub fn new() -> Self {
        Self::default()
    }

    /// Keeps the progress `event` tells, in place of the one before; lets
    /// it go when `event` ends its operation. An event whose ID, time,
    /// step or unit doesn't read changes nothing.
    pub fn hear(&self, event: &UiEvent) {
        let mut latest = self.latest.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(progress) = progress_of(event) {
            *latest = Some(progress);
        } else if let UiEvent::OperationSucceeded { operation, .. }
        | UiEvent::OperationFailed { operation, .. }
        | UiEvent::OperationCanceled { operation, .. } = event
            && latest
                .as_ref()
                .is_some_and(|progress| Some(progress.op_id) == operation_id(operation))
        {
            *latest = None;
        }
    }

    /// The progress kept now, none between operations.
    pub fn latest(&self) -> Option<ReportedProgress> {
        self.latest
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

/// The progress an `OperationProgress` event tells.
fn progress_of(event: &UiEvent) -> Option<ReportedProgress> {
    let UiEvent::OperationProgress {
        operation,
        step,
        unit,
        current,
        total,
        at,
    } = event
    else {
        return None;
    };
    Some(ReportedProgress {
        op_id: operation_id(operation)?,
        step: match step {
            Some(step) => Some(wire_name(step)?),
            None => None,
        },
        unit: wire_name(unit)?,
        current: *current,
        total: *total,
        at: EpochSeconds::from_timestamp(at.as_deref()?.parse::<Timestamp>().ok()?)?,
    })
}

fn operation_id(operation: &str) -> Option<OperationId> {
    Uuid::try_parse(operation).ok().map(OperationId::from_uuid)
}

/// The kebab-case name `value` has on the API's wire.
fn wire_name<T: serde::Serialize>(value: &T) -> Option<WireName> {
    match serde_json::to_value(value).ok()? {
        serde_json::Value::String(name) => WireName::parse(&name).ok(),
        _ => None,
    }
}

#[cfg(test)]
mod tests;
