//! A paired machine's ledger of its hub's commands (§16, decisions 120 and
//! 176; tables `hub_commands` and `hub_runs`): the `CommandLedger` port on
//! the store's own file. Each call is one transaction, so two commands
//! taken at once can't both find room or both be new. An ID is forgotten
//! once it lies a whole life from the time given, either way: one taken
//! while the clock ran ahead goes once the clock is set back, rather than
//! waiting for the clock to catch up.

use app::ports::{CommandLedger, Remembered, StoreError};
use async_trait::async_trait;
use domain::fleet::{COMMANDS_REMEMBERED_FOR, CommandId, HubRun, MAX_COMMANDS_REMEMBERED};
use domain::{PlanId, SignedDuration, Timestamp};
use sqlx::{Connection, Row};

use crate::{SqliteStore, unavailable};

/// Parameters: the times at or before which, and at or after which, an ID
/// is forgotten.
const FORGET_COMMANDS: &str = "DELETE FROM hub_commands WHERE taken_at <= ?1 OR taken_at >= ?2";

const COMMAND_KEPT: &str = "SELECT EXISTS (SELECT 1 FROM hub_commands WHERE command_id = ?1)";

const COMMANDS_KEPT: &str = "SELECT COUNT(*) AS kept, MIN(taken_at) AS oldest FROM hub_commands";

const KEEP_COMMAND: &str = "INSERT INTO hub_commands (command_id, taken_at) VALUES (?1, ?2)";

/// Parameter: the time before which, or at which, a run is forgotten.
const FORGET_RUNS: &str = "DELETE FROM hub_runs WHERE at <= ?1";

const KEEP_RUN: &str = "INSERT INTO hub_runs (plan_id, at) VALUES (?1, ?2)";

const RUNS: &str = "SELECT plan_id, at FROM hub_runs ORDER BY at, rowid";

/// How long a run the hub started counts against its limit.
const RUNS_KEPT_FOR: SignedDuration = SignedDuration::from_hours(1);

#[async_trait]
impl CommandLedger for SqliteStore {
    async fn remember(&self, id: CommandId, now: Timestamp) -> Result<Remembered, StoreError> {
        let id = id.as_uuid().to_string();
        let life = i64::try_from(COMMANDS_REMEMBERED_FOR.as_millis()).unwrap_or(i64::MAX);
        let _call = self.call().await;
        let mut conn = self.pool.acquire().await.map_err(unavailable)?;
        let mut tx = conn.begin().await.map_err(unavailable)?;
        sqlx::query(FORGET_COMMANDS)
            .bind(now.as_millisecond().saturating_sub(life))
            .bind(now.as_millisecond().saturating_add(life))
            .execute(&mut *tx)
            .await
            .map_err(unavailable)?;
        let seen: bool = sqlx::query_scalar(COMMAND_KEPT)
            .bind(id.as_str())
            .fetch_one(&mut *tx)
            .await
            .map_err(unavailable)?;
        if seen {
            return Ok(Remembered::Seen);
        }
        let row = sqlx::query(COMMANDS_KEPT)
            .fetch_one(&mut *tx)
            .await
            .map_err(unavailable)?;
        let kept: i64 = row.try_get("kept").map_err(unavailable)?;
        let oldest: Option<i64> = row.try_get("oldest").map_err(unavailable)?;
        if usize::try_from(kept).unwrap_or(usize::MAX) >= MAX_COMMANDS_REMEMBERED
            && let Some(oldest) = oldest
        {
            let oldest = Timestamp::from_millisecond(oldest).map_err(|_| corrupt("taken_at"))?;
            return Ok(Remembered::Full { oldest });
        }
        sqlx::query(KEEP_COMMAND)
            .bind(id.as_str())
            .bind(now.as_millisecond())
            .execute(&mut *tx)
            .await
            .map_err(unavailable)?;
        tx.commit().await.map_err(unavailable)?;
        Ok(Remembered::New)
    }

    async fn ran(&self, run: &HubRun) -> Result<(), StoreError> {
        let life = i64::try_from(RUNS_KEPT_FOR.as_millis()).unwrap_or(i64::MAX);
        let _call = self.call().await;
        let mut conn = self.pool.acquire().await.map_err(unavailable)?;
        let mut tx = conn.begin().await.map_err(unavailable)?;
        sqlx::query(FORGET_RUNS)
            .bind(run.at.as_millisecond().saturating_sub(life))
            .execute(&mut *tx)
            .await
            .map_err(unavailable)?;
        sqlx::query(KEEP_RUN)
            .bind(run.plan.as_str())
            .bind(run.at.as_millisecond())
            .execute(&mut *tx)
            .await
            .map_err(unavailable)?;
        tx.commit().await.map_err(unavailable)
    }

    async fn runs(&self) -> Result<Vec<HubRun>, StoreError> {
        let _call = self.call().await;
        sqlx::query(RUNS)
            .fetch_all(&self.pool)
            .await
            .map_err(unavailable)?
            .iter()
            .map(|row| {
                let plan: String = row.try_get("plan_id").map_err(unavailable)?;
                let at: i64 = row.try_get("at").map_err(unavailable)?;
                Ok(HubRun {
                    plan: PlanId::parse(&plan).map_err(|_| corrupt("plan_id"))?,
                    at: Timestamp::from_millisecond(at).map_err(|_| corrupt("at"))?,
                })
            })
            .collect()
    }
}

fn corrupt(name: &str) -> StoreError {
    StoreError::Corrupt {
        detail: format!("hub commands: column {name} doesn't read"),
    }
}
