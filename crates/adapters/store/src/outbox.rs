//! The agent's outbox and the hub's reports of its agents (technical design
//! §7.2, migration `0021_sync_outbox`): the `Outbox` and `ReportInbox`
//! ports on the store's own file, built only with the hub (decision 122).

use app::ports::{Appended, Ingested, LatestReports, Outbox, OutboxEntry, ReportInbox, StoreError};
use async_trait::async_trait;
use domain::fleet::{
    AgentReport, EpochSeconds, Happening, Ingest, MAX_EPOCHS_HELD, MAX_REPORTS_HELD,
    OutboxAdmission, OutboxEpoch, PlansReport, ReportSeq, ReportedProgress, WireName, admit,
    ingest,
};
use domain::{AgentId, OperationId, Timestamp, Uuid};
use sqlx::sqlite::SqliteRow;
use sqlx::{Connection, Row, SqliteConnection};

use crate::row::agent_id_text;
use crate::{SqliteStore, unavailable};

const META: &str = "SELECT epoch, last_seq FROM sync_outbox_meta WHERE id = 1";
/// A new outbox's epoch: a random UUID (version 4), which only has to differ
/// from the epoch of any outbox the machine had before.
const NEW_META: &str = "INSERT INTO sync_outbox_meta (id, epoch, last_seq) VALUES (1, \
     lower(hex(randomblob(4))) || '-' || lower(hex(randomblob(2))) || '-4' || \
     substr(lower(hex(randomblob(2))), 2) || '-' || \
     substr('89ab', 1 + abs(random() % 4), 1) || substr(lower(hex(randomblob(2))), 2) || '-' || \
     lower(hex(randomblob(6))), 0)";
const HELD: &str = "SELECT (SELECT COUNT(*) FROM sync_outbox), \
     (SELECT MIN(seq) FROM sync_outbox WHERE kept = 0)";
const EVICT: &str = "DELETE FROM sync_outbox WHERE seq = ?";
const APPEND: &str =
    "INSERT INTO sync_outbox (seq, op_id, happening, code, at, kept) VALUES (?, ?, ?, ?, ?, ?)";
const GIVEN: &str = "UPDATE sync_outbox_meta SET last_seq = ? WHERE id = 1";
const WAITING: &str = "SELECT seq, op_id, happening, code, at FROM sync_outbox \
     WHERE seq > ? ORDER BY seq LIMIT ?";
const ACKNOWLEDGE: &str = "DELETE FROM sync_outbox WHERE seq <= ?";
const WATERMARK: &str = "SELECT seq FROM sync_watermarks WHERE agent_id = ? AND epoch = ?";
const RAISE: &str = "INSERT INTO sync_watermarks (agent_id, epoch, seq, ingested_at) \
     VALUES (?, ?, ?, ?) ON CONFLICT (agent_id, epoch) \
     DO UPDATE SET seq = excluded.seq, ingested_at = excluded.ingested_at";
const TAKE: &str = "INSERT INTO sync_reports (agent_id, epoch, seq, op_id, happening, code, at) \
     VALUES (?, ?, ?, ?, ?, ?, ?)";
/// The agent's newest epochs by when the hub last took one of each, the
/// one just taken always among them, however the hub's clock moved.
const FORGET_EPOCHS: &str = "DELETE FROM sync_watermarks WHERE agent_id = ?1 AND epoch NOT IN \
     (SELECT epoch FROM sync_watermarks WHERE agent_id = ?1 \
      ORDER BY (epoch = ?3) DESC, ingested_at DESC, rowid DESC LIMIT ?2)";
const TALLY: &str = "INSERT INTO sync_report_tallies (agent_id, held, dropped) VALUES (?, 1, 0) \
     ON CONFLICT (agent_id) DO UPDATE SET held = held + 1 RETURNING held";
const FORGET_OLDEST: &str = "DELETE FROM sync_reports WHERE id IN \
     (SELECT id FROM sync_reports WHERE agent_id = ?1 ORDER BY id LIMIT ?2)";
const COUNT_DROPS: &str = "UPDATE sync_report_tallies \
     SET held = held - ?2, dropped = dropped + ?2 WHERE agent_id = ?1 RETURNING dropped";
const REPORTS: &str = "SELECT epoch, seq, op_id, happening, code, at FROM sync_reports \
     WHERE agent_id = ? ORDER BY id";
/// An agent's newest reports, newest first; the caller turns them round.
const LATEST: &str = "SELECT op_id, happening, code, at FROM sync_reports \
     WHERE agent_id = ? ORDER BY id DESC LIMIT ?";
const DROPPED: &str = "SELECT dropped FROM sync_report_tallies WHERE agent_id = ?";

#[async_trait]
impl Outbox for SqliteStore {
    /// One transaction, the write lock taken up front, so two reports at
    /// once take two places.
    async fn append(&self, report: &AgentReport) -> Result<Appended, StoreError> {
        let _call = self.call().await;
        let mut conn = self.pool.acquire().await.map_err(unavailable)?;
        let mut tx = conn
            .begin_with("BEGIN IMMEDIATE")
            .await
            .map_err(unavailable)?;
        let last = match meta(&mut tx).await? {
            Some((_, last)) => last,
            None => {
                sqlx::query(NEW_META)
                    .execute(&mut *tx)
                    .await
                    .map_err(unavailable)?;
                0
            }
        };
        let next = match ReportSeq::new(last) {
            None => Some(ReportSeq::FIRST),
            Some(last) => last.next(),
        }
        .filter(|next| i64::try_from(next.get()).is_ok());
        let held = sqlx::query(HELD)
            .fetch_one(&mut *tx)
            .await
            .map_err(unavailable)?;
        let count: i64 = held.try_get(0).map_err(unavailable)?;
        let oldest: Option<i64> = held.try_get(1).map_err(unavailable)?;
        let oldest_start = oldest.and_then(|seq| ReportSeq::new(u64::try_from(seq).ok()?));
        let admitted = admit(u64::try_from(count).unwrap_or(0), oldest_start, next);
        let Some(next) = next.filter(|_| admitted != OutboxAdmission::Refuse) else {
            return Ok(Appended::Refused);
        };
        let gave_up = match admitted {
            OutboxAdmission::EvictThenAppend(seq) => {
                sqlx::query(EVICT)
                    .bind(place(seq))
                    .execute(&mut *tx)
                    .await
                    .map_err(unavailable)?;
                Some(seq)
            }
            OutboxAdmission::Append | OutboxAdmission::Refuse => None,
        };
        let (happening, code) = happening_text(&report.happening);
        sqlx::query(APPEND)
            .bind(place(next))
            .bind(report.op_id.as_uuid().to_string())
            .bind(happening)
            .bind(code)
            .bind(i64::try_from(report.at.seconds()).unwrap_or(i64::MAX))
            .bind(i64::from(report.kept()))
            .execute(&mut *tx)
            .await
            .map_err(unavailable)?;
        sqlx::query(GIVEN)
            .bind(place(next))
            .execute(&mut *tx)
            .await
            .map_err(unavailable)?;
        tx.commit().await.map_err(unavailable)?;
        Ok(match gave_up {
            None => Appended::Kept(next),
            Some(gave_up) => Appended::KeptGivingUp { seq: next, gave_up },
        })
    }

    async fn unacknowledged(
        &self,
        after: Option<ReportSeq>,
        limit: usize,
    ) -> Result<Vec<OutboxEntry>, StoreError> {
        let _call = self.call().await;
        let mut conn = self.pool.acquire().await.map_err(unavailable)?;
        let Some((epoch, _)) = meta(&mut conn).await? else {
            return Ok(Vec::new());
        };
        let rows = sqlx::query(WAITING)
            .bind(after.map_or(0, place))
            .bind(i64::try_from(limit).unwrap_or(i64::MAX))
            .fetch_all(&mut *conn)
            .await
            .map_err(unavailable)?;
        rows.iter()
            .map(|row| {
                Ok(OutboxEntry {
                    epoch,
                    seq: seq_of(row, "seq")?,
                    report: report_of(row)?,
                })
            })
            .collect()
    }

    async fn acknowledge(&self, epoch: OutboxEpoch, through: ReportSeq) -> Result<(), StoreError> {
        let _call = self.call().await;
        let mut conn = self.pool.acquire().await.map_err(unavailable)?;
        let mut tx = conn
            .begin_with("BEGIN IMMEDIATE")
            .await
            .map_err(unavailable)?;
        if meta(&mut tx).await?.is_some_and(|(held, _)| held == epoch) {
            sqlx::query(ACKNOWLEDGE)
                .bind(place(through))
                .execute(&mut *tx)
                .await
                .map_err(unavailable)?;
        }
        tx.commit().await.map_err(unavailable)
    }
}

#[async_trait]
impl ReportInbox for SqliteStore {
    /// One transaction, the write lock taken up front, so of two deliveries
    /// of one report at once, the second finds the first's watermark.
    async fn ingest(
        &self,
        agent: AgentId,
        entry: &OutboxEntry,
        received_at: Timestamp,
    ) -> Result<Ingested, StoreError> {
        let agent = agent_id_text(agent);
        let epoch = entry.epoch.as_uuid().to_string();
        let _call = self.call().await;
        let mut conn = self.pool.acquire().await.map_err(unavailable)?;
        let mut tx = conn
            .begin_with("BEGIN IMMEDIATE")
            .await
            .map_err(unavailable)?;
        let held = sqlx::query(WATERMARK)
            .bind(&agent)
            .bind(&epoch)
            .fetch_optional(&mut *tx)
            .await
            .map_err(unavailable)?
            .map(|row| seq_of(&row, "seq"))
            .transpose()?;
        let verdict = ingest(held, entry.seq);
        if verdict != Ingest::New {
            return Ok(Ingested {
                ingest: verdict,
                dropped: None,
            });
        }
        sqlx::query(RAISE)
            .bind(&agent)
            .bind(&epoch)
            .bind(place(entry.seq))
            .bind(received_at.as_millisecond())
            .execute(&mut *tx)
            .await
            .map_err(unavailable)?;
        let (happening, code) = happening_text(&entry.report.happening);
        sqlx::query(TAKE)
            .bind(&agent)
            .bind(&epoch)
            .bind(place(entry.seq))
            .bind(entry.report.op_id.as_uuid().to_string())
            .bind(happening)
            .bind(code)
            .bind(i64::try_from(entry.report.at.seconds()).unwrap_or(i64::MAX))
            .execute(&mut *tx)
            .await
            .map_err(unavailable)?;
        sqlx::query(FORGET_EPOCHS)
            .bind(&agent)
            .bind(i64::try_from(MAX_EPOCHS_HELD).unwrap_or(i64::MAX))
            .bind(&epoch)
            .execute(&mut *tx)
            .await
            .map_err(unavailable)?;
        let held: i64 = sqlx::query(TALLY)
            .bind(&agent)
            .fetch_one(&mut *tx)
            .await
            .map_err(unavailable)?
            .try_get("held")
            .map_err(unavailable)?;
        let over = held.saturating_sub(i64::try_from(MAX_REPORTS_HELD).unwrap_or(i64::MAX));
        let mut dropped = None;
        if over > 0 {
            let let_go = sqlx::query(FORGET_OLDEST)
                .bind(&agent)
                .bind(over)
                .execute(&mut *tx)
                .await
                .map_err(unavailable)?
                .rows_affected();
            let in_all: i64 = sqlx::query(COUNT_DROPS)
                .bind(&agent)
                .bind(i64::try_from(let_go).unwrap_or(i64::MAX))
                .fetch_one(&mut *tx)
                .await
                .map_err(unavailable)?
                .try_get("dropped")
                .map_err(unavailable)?;
            dropped = Some(u64::try_from(in_all).map_err(|_| corrupt("dropped"))?);
        }
        tx.commit().await.map_err(unavailable)?;
        Ok(Ingested {
            ingest: Ingest::New,
            dropped,
        })
    }

    async fn reports(&self, agent: AgentId) -> Result<Vec<OutboxEntry>, StoreError> {
        let _call = self.call().await;
        let mut conn = self.pool.acquire().await.map_err(unavailable)?;
        let rows = sqlx::query(REPORTS)
            .bind(agent_id_text(agent))
            .fetch_all(&mut *conn)
            .await
            .map_err(unavailable)?;
        rows.iter()
            .map(|row| {
                Ok(OutboxEntry {
                    epoch: epoch_of(row, "epoch")?,
                    seq: seq_of(row, "seq")?,
                    report: report_of(row)?,
                })
            })
            .collect()
    }

    async fn latest(&self, agent: AgentId, limit: usize) -> Result<LatestReports, StoreError> {
        let agent = agent_id_text(agent);
        let _call = self.call().await;
        let mut conn = self.pool.acquire().await.map_err(unavailable)?;
        let rows = sqlx::query(LATEST)
            .bind(&agent)
            .bind(i64::try_from(limit).unwrap_or(i64::MAX))
            .fetch_all(&mut *conn)
            .await
            .map_err(unavailable)?;
        let mut reports = rows.iter().map(report_of).collect::<Result<Vec<_>, _>>()?;
        reports.reverse();
        let dropped: Option<i64> = sqlx::query(DROPPED)
            .bind(&agent)
            .fetch_optional(&mut *conn)
            .await
            .map_err(unavailable)?
            .map(|row| row.try_get("dropped"))
            .transpose()
            .map_err(unavailable)?;
        Ok(LatestReports {
            reports,
            dropped: u64::try_from(dropped.unwrap_or(0)).map_err(|_| corrupt("dropped"))?,
        })
    }

    async fn keep_plans(&self, agent: AgentId, plans: &PlansReport) -> Result<(), StoreError> {
        live::keep_plans(self, agent, plans).await
    }

    async fn plans(&self, agent: AgentId) -> Result<Option<PlansReport>, StoreError> {
        live::plans(self, agent).await
    }

    async fn keep_progress(
        &self,
        agent: AgentId,
        progress: &ReportedProgress,
    ) -> Result<(), StoreError> {
        live::keep_progress(self, agent, progress).await
    }

    async fn progress(&self, agent: AgentId) -> Result<Option<ReportedProgress>, StoreError> {
        live::progress(self, agent).await
    }
}

mod live;

/// The outbox's epoch and the last place it gave, none before its first
/// report.
async fn meta(conn: &mut SqliteConnection) -> Result<Option<(OutboxEpoch, u64)>, StoreError> {
    let Some(row) = sqlx::query(META)
        .fetch_optional(&mut *conn)
        .await
        .map_err(unavailable)?
    else {
        return Ok(None);
    };
    let last: i64 = row.try_get("last_seq").map_err(unavailable)?;
    let last = u64::try_from(last).map_err(|_| corrupt("last_seq"))?;
    Ok(Some((epoch_of(&row, "epoch")?, last)))
}

/// A place as the store keeps it: every place the outbox gives fits, and
/// the hub reads none that doesn't (`read_report`).
fn place(seq: ReportSeq) -> i64 {
    i64::try_from(seq.get()).unwrap_or(i64::MAX)
}

fn happening_text(happening: &Happening) -> (&'static str, Option<&str>) {
    match happening {
        Happening::Started => ("started", None),
        Happening::Succeeded => ("succeeded", None),
        Happening::Failed { code } => ("failed", Some(code.as_str())),
        Happening::Cancelled => ("cancelled", None),
    }
}

fn corrupt(column: &str) -> StoreError {
    StoreError::Corrupt {
        detail: format!("sync outbox: column {column} doesn't read"),
    }
}

fn seq_of(row: &SqliteRow, column: &str) -> Result<ReportSeq, StoreError> {
    let seq: i64 = row.try_get(column).map_err(unavailable)?;
    u64::try_from(seq)
        .ok()
        .and_then(ReportSeq::new)
        .ok_or_else(|| corrupt(column))
}

fn epoch_of(row: &SqliteRow, column: &str) -> Result<OutboxEpoch, StoreError> {
    let text: String = row.try_get(column).map_err(unavailable)?;
    Uuid::try_parse(&text)
        .map(OutboxEpoch::from_uuid)
        .map_err(|_| corrupt(column))
}

fn report_of(row: &SqliteRow) -> Result<AgentReport, StoreError> {
    let op_id: String = row.try_get("op_id").map_err(unavailable)?;
    let happening: String = row.try_get("happening").map_err(unavailable)?;
    let code: Option<String> = row.try_get("code").map_err(unavailable)?;
    let at: i64 = row.try_get("at").map_err(unavailable)?;
    let happening = match (happening.as_str(), code) {
        ("started", None) => Happening::Started,
        ("succeeded", None) => Happening::Succeeded,
        ("failed", Some(code)) => Happening::Failed {
            code: WireName::parse(&code).map_err(|_| corrupt("code"))?,
        },
        ("cancelled", None) => Happening::Cancelled,
        _ => return Err(corrupt("happening")),
    };
    Ok(AgentReport {
        op_id: OperationId::from_uuid(Uuid::try_parse(&op_id).map_err(|_| corrupt("op_id"))?),
        happening,
        at: EpochSeconds::from_seconds(u64::try_from(at).map_err(|_| corrupt("at"))?),
    })
}
