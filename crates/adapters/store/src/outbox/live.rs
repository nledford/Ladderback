//! The hub's latest plans and progress of each agent (technical design
//! §7.2, migration `0025_sync_plans_progress`): each kept in place of the
//! one before.

use app::ports::StoreError;
use domain::fleet::{
    EpochSeconds, LastRun, LastRunOutcome, PlansReport, ReportedPlan, ReportedProgress, WireName,
};
use domain::{AgentId, OperationId, PlanId, Uuid};
use sqlx::sqlite::SqliteRow;
use sqlx::{Connection, Row};

use super::corrupt;
use crate::row::agent_id_text;
use crate::{SqliteStore, unavailable};

const FORGET_PLANS: &str = "DELETE FROM sync_plans WHERE agent_id = ?";
const KEEP_PLAN: &str = "INSERT INTO sync_plans \
     (agent_id, place, plan_id, last_at, last_outcome, next_run) VALUES (?, ?, ?, ?, ?, ?)";
const KEEP_SNAPSHOT: &str = "INSERT INTO sync_plan_snapshots (agent_id, left_out) VALUES (?, ?) \
     ON CONFLICT (agent_id) DO UPDATE SET left_out = excluded.left_out";
const SNAPSHOT: &str = "SELECT left_out FROM sync_plan_snapshots WHERE agent_id = ?";
const PLANS: &str = "SELECT plan_id, last_at, last_outcome, next_run FROM sync_plans \
     WHERE agent_id = ? ORDER BY place";
const KEEP_PROGRESS: &str = "INSERT INTO sync_progress \
     (agent_id, op_id, step, unit, current, total, at) VALUES (?, ?, ?, ?, ?, ?, ?) \
     ON CONFLICT (agent_id) DO UPDATE SET op_id = excluded.op_id, step = excluded.step, \
     unit = excluded.unit, current = excluded.current, total = excluded.total, at = excluded.at";
const PROGRESS: &str =
    "SELECT op_id, step, unit, current, total, at FROM sync_progress WHERE agent_id = ?";

/// One transaction, so a reader finds the snapshot before or after, whole.
pub(super) async fn keep_plans(
    _store: &SqliteStore,
    _agent: AgentId,
    _plans: &PlansReport,
) -> Result<(), StoreError> {
    todo!()
}

pub(super) async fn plans(
    _store: &SqliteStore,
    _agent: AgentId,
) -> Result<Option<PlansReport>, StoreError> {
    Ok(None)
}

pub(super) async fn keep_progress(
    _store: &SqliteStore,
    _agent: AgentId,
    _progress: &ReportedProgress,
) -> Result<(), StoreError> {
    todo!()
}

pub(super) async fn progress(
    _store: &SqliteStore,
    _agent: AgentId,
) -> Result<Option<ReportedProgress>, StoreError> {
    Ok(None)
}

/// A number as the store keeps it: the hub reads none past `i64::MAX`
/// (`app::pairing::wire::read_plans`, `read_progress`).
fn number(value: u64) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

fn held(row: &SqliteRow, column: &str) -> Result<u64, StoreError> {
    let value: i64 = row.try_get(column).map_err(unavailable)?;
    u64::try_from(value).map_err(|_| corrupt(column))
}

fn held_if_any(row: &SqliteRow, column: &str) -> Result<Option<u64>, StoreError> {
    let value: Option<i64> = row.try_get(column).map_err(unavailable)?;
    value
        .map(|value| u64::try_from(value).map_err(|_| corrupt(column)))
        .transpose()
}

fn wire_name(text: &str, column: &str) -> Result<WireName, StoreError> {
    WireName::parse(text).map_err(|_| corrupt(column))
}

fn plan_of(row: &SqliteRow) -> Result<ReportedPlan, StoreError> {
    let plan: String = row.try_get("plan_id").map_err(unavailable)?;
    let outcome: Option<String> = row.try_get("last_outcome").map_err(unavailable)?;
    let last = match (held_if_any(row, "last_at")?, outcome) {
        (Some(at), Some(outcome)) => Some(LastRun {
            at: EpochSeconds::from_seconds(at),
            outcome: LastRunOutcome::from_wire_name(&outcome)
                .ok_or_else(|| corrupt("last_outcome"))?,
        }),
        (None, None) => None,
        _ => return Err(corrupt("last_at")),
    };
    Ok(ReportedPlan {
        plan: PlanId::parse(&plan).map_err(|_| corrupt("plan_id"))?,
        last,
        next_run: held_if_any(row, "next_run")?.map(EpochSeconds::from_seconds),
    })
}

fn progress_of(row: &SqliteRow) -> Result<ReportedProgress, StoreError> {
    let op_id: String = row.try_get("op_id").map_err(unavailable)?;
    let step: Option<String> = row.try_get("step").map_err(unavailable)?;
    let unit: String = row.try_get("unit").map_err(unavailable)?;
    Ok(ReportedProgress {
        op_id: OperationId::from_uuid(Uuid::try_parse(&op_id).map_err(|_| corrupt("op_id"))?),
        step: step.map(|step| wire_name(&step, "step")).transpose()?,
        unit: wire_name(&unit, "unit")?,
        current: held(row, "current")?,
        total: held_if_any(row, "total")?,
        at: EpochSeconds::from_seconds(held(row, "at")?),
    })
}
