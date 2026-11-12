//! Reading rows back into the app's read model.
//!
//! A value this version didn't write is a [`StoreError::Corrupt`] naming the
//! operation and the column, never a default: a row that can't be read must be
//! seen, not shown as something else. Every column is read as an `Option`,
//! because SQLx reads a `NULL` as `""` or `0` when asked for a plain `String`
//! or `i64`, and a missing value would pass for one.

use std::fmt::Display;

use app::ports::{EngineResult, OperationRecord, PlannedRun, StoreError};
use domain::execution::{OperationStatus, Target, Warning};
use domain::{OperationId, PlanId, RepositoryAlias, RepositoryId, SnapshotId, Timestamp, Uuid};
use sqlx::Row;
use sqlx::sqlite::SqliteRow;

use crate::codec::{self, BadColumn};
use crate::report::{cap, shown};
use crate::summary::Summary;

/// One row, and what a report calls it.
struct Reader<'r> {
    row: &'r SqliteRow,
    scope: String,
}

impl Reader<'_> {
    fn corrupt(&self, column: &str, why: impl Display) -> StoreError {
        StoreError::Corrupt {
            detail: cap(format!("{}: column {column}: {why}", self.scope)),
        }
    }

    fn bad(&self, bad: BadColumn) -> StoreError {
        self.corrupt(bad.column, bad.why)
    }

    fn text(&self, column: &str) -> Result<Option<String>, StoreError> {
        self.row
            .try_get::<Option<String>, _>(column)
            .map_err(|error| self.corrupt(column, unreadable(&error)))
    }

    fn required_text(&self, column: &str) -> Result<String, StoreError> {
        self.text(column)?
            .ok_or_else(|| self.corrupt(column, "is NULL"))
    }

    fn integer(&self, column: &str) -> Result<Option<i64>, StoreError> {
        self.row
            .try_get::<Option<i64>, _>(column)
            .map_err(|error| self.corrupt(column, unreadable(&error)))
    }

    fn time(&self, column: &str) -> Result<Option<Timestamp>, StoreError> {
        let Some(millisecond) = self.integer(column)? else {
            return Ok(None);
        };
        Timestamp::from_millisecond(millisecond)
            .map(Some)
            .map_err(|_| self.corrupt(column, format!("{millisecond} is not a time")))
    }

    fn required_time(&self, column: &str) -> Result<Timestamp, StoreError> {
        self.time(column)?
            .ok_or_else(|| self.corrupt(column, "is NULL"))
    }

    /// An ID of the kind `parse` reads, which the column may leave out.
    fn id_of<T, E>(
        &self,
        column: &str,
        what: &str,
        parse: impl Fn(&str) -> Result<T, E>,
    ) -> Result<Option<T>, StoreError> {
        let Some(text) = self.text(column)? else {
            return Ok(None);
        };
        parse(&text)
            .map(Some)
            .map_err(|_| self.corrupt(column, format!("{} is not a {what}", shown(&text))))
    }

    /// The operation's ID, which must be the exact text the store writes: a
    /// row under another spelling of it could never be found by `get`.
    fn operation_id(&self, text: &str) -> Result<OperationId, StoreError> {
        let uuid = Uuid::parse_str(text).map_err(|_| self.corrupt("id", "is not a UUID"))?;
        let mut buffer = Uuid::encode_buffer();
        let canonical: &str = uuid.hyphenated().encode_lower(&mut buffer);
        if canonical != text {
            return Err(self.corrupt("id", "is not in lowercase hyphenated form"));
        }
        Ok(OperationId::from_uuid(uuid))
    }

    fn result(&self) -> Result<Option<EngineResult>, StoreError> {
        let Some(text) = self.text("summary_json")? else {
            return Ok(None);
        };
        let summary: Summary =
            serde_json::from_str(&text).map_err(|error| self.corrupt("summary_json", error))?;
        Ok(Some(summary.into()))
    }

    /// How the operation ended up, from its status and its error columns.
    fn status(&self) -> Result<OperationStatus, StoreError> {
        let word = self.required_text("status")?;
        let code = self.text("error_code")?;
        let args = self.text("error_args")?;
        let detail = self.text("error_detail")?;
        if word == codec::FAILED {
            let reason = codec::decode_failure(code.as_deref(), args.as_deref(), detail.as_deref())
                .map_err(|bad| self.bad(bad))?;
            return Ok(OperationStatus::Failed { reason });
        }
        let status = codec::unfailed_status_from_text(&word)
            .ok_or_else(|| self.corrupt("status", format!("{} is not a status", shown(&word))))?;
        for (column, stray) in [
            ("error_code", &code),
            ("error_args", &args),
            ("error_detail", &detail),
        ] {
            if stray.is_some() {
                return Err(self.corrupt(column, "is set on an operation that did not fail"));
            }
        }
        Ok(status)
    }
}

/// A row of `schedule_state`: a plan's planned run.
pub(crate) fn planned_run(row: &SqliteRow) -> Result<PlannedRun, StoreError> {
    let reader = Reader {
        row,
        scope: "a planned run".to_owned(),
    };
    let plan = reader
        .id_of("plan_id", "plan ID", PlanId::parse)?
        .ok_or_else(|| reader.corrupt("plan_id", "is NULL"))?;
    let reader = Reader {
        row,
        scope: format!("the planned run of {}", plan.as_str()),
    };
    Ok(PlannedRun {
        plan,
        at: reader.required_time("next_run")?,
    })
}

/// What a column that couldn't be read says: for a value of the wrong type,
/// SQLx's account of the types, without its repeating the column.
fn unreadable(error: &sqlx::Error) -> String {
    match error {
        sqlx::Error::ColumnDecode { source, .. } => source.to_string(),
        other => other.to_string(),
    }
}

/// The ID in a row that holds only an `id`, as the running-maintenance lookup
/// reads it. A row that holds a bad one is [`StoreError::Corrupt`].
pub(crate) fn operation_id(row: &SqliteRow) -> Result<OperationId, StoreError> {
    let mut reader = Reader {
        row,
        scope: "an operation".to_owned(),
    };
    let id_text = reader.required_text("id")?;
    reader.scope = format!("operation {}", shown(&id_text));
    reader.operation_id(&id_text)
}

/// An operation's row, as history lists it.
pub(crate) fn record(row: &SqliteRow) -> Result<OperationRecord, StoreError> {
    let mut reader = Reader {
        row,
        scope: "an operation".to_owned(),
    };
    let id_text = reader.required_text("id")?;
    reader.scope = format!("operation {}", shown(&id_text));
    let id = reader.operation_id(&id_text)?;

    let kind = reader.required_text("kind")?;
    let kind = codec::kind_from_text(&kind)
        .ok_or_else(|| reader.corrupt("kind", format!("{} is not a kind", shown(&kind))))?;
    let trigger = reader.required_text("trigger")?;
    let trigger = codec::trigger_from_text(&trigger).ok_or_else(|| {
        reader.corrupt("trigger", format!("{} is not a trigger", shown(&trigger)))
    })?;
    let target = Target {
        plan: reader.id_of("plan_id", "plan ID", PlanId::parse)?,
        repository: reader.id_of("repo_uuid", "repository ID", RepositoryId::parse)?,
    };
    let status = reader.status()?;
    let queued_at = reader.required_time("queued_at")?;
    let started_at = reader.time("started_at")?;
    let finished_at = reader.time("finished_at")?;
    let result = reader.result()?;

    // The snapshot has a column of its own for lookups; it must agree with the result.
    let snapshot = reader.id_of("snapshot_id", "snapshot ID", SnapshotId::parse)?;
    let results_snapshot = match &result {
        Some(EngineResult::Backup(backup)) => Some(&backup.snapshot),
        _ => None,
    };
    match (&snapshot, results_snapshot) {
        (None, None) => {}
        (Some(stored), Some(reported)) if stored == reported => {}
        (Some(_), Some(_)) => {
            return Err(reader.corrupt("snapshot_id", "is not the snapshot of the backup result"));
        }
        (Some(_), None) => {
            return Err(reader.corrupt("snapshot_id", "is set, but there is no backup result"));
        }
        (None, Some(_)) => {
            return Err(reader.corrupt("snapshot_id", "is NULL, but there is a backup result"));
        }
    }

    let warning_count = reader.integer("warning_count")?;
    let warning_count = warning_count
        .ok_or_else(|| reader.corrupt("warning_count", "is NULL"))
        .and_then(|count| {
            u64::try_from(count)
                .map_err(|_| reader.corrupt("warning_count", format!("{count} is negative")))
        })?;
    let repository_alias =
        reader.id_of("repo_alias", "repository alias", RepositoryAlias::parse)?;

    Ok(OperationRecord {
        id,
        kind,
        trigger,
        target,
        status,
        queued_at,
        started_at,
        finished_at,
        warning_count,
        result,
        repository_alias,
    })
}

/// One of an operation's warnings. `operation` is the operation's ID text and
/// `position` the warning's place among its warnings, for the report.
pub(crate) fn warning(
    operation: &str,
    position: usize,
    row: &SqliteRow,
) -> Result<Warning, StoreError> {
    let reader = Reader {
        row,
        scope: format!("operation {}, warning {position}", shown(operation)),
    };
    let code = reader.required_text("code")?;
    Ok(Warning {
        kind: codec::warning_kind_from_text(&code),
        item: reader.text("item")?,
        during: reader.required_text("during")?,
        detail: reader.required_text("detail")?,
    })
}
