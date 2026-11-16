//! Reading rows back into the app's read model.
//!
//! A value this version didn't write is a [`StoreError::Corrupt`] naming the
//! operation and the column, never a default: a row that can't be read must be
//! seen, not shown as something else. Every column is read as an `Option`,
//! because SQLx reads a `NULL` as `""` or `0` when asked for a plain `String`
//! or `i64`, and a missing value would pass for one.

use std::fmt::Display;

use app::ports::{
    EngineResult, OperationRecord, PendingRetry, PlannedRun, ReleaseCheck, StoreError,
    StoredProposal,
};
use domain::access::api_token::{
    ApiToken, ApiTokenDigest, ApiTokenId, ApiTokenLabel, Scope, Scopes,
};
use domain::access::{KeptSession, PasswordFingerprint, SessionDigest, SessionLength};
use domain::execution::locations::LocationsRecord;
use domain::execution::retry::RetryScheduled;
use domain::execution::{OperationStatus, Target, Trigger, Warning, WarningKind};
use domain::maintenance::{RepositoryStats, StatsReading};
use domain::planning::{
    ConfigRevision, ProposalId, ProposalStatus, Proposer, Rationale, RejectReason,
};
use domain::updates::Version;
use domain::{OperationId, PlanId, RepositoryAlias, RepositoryId, SnapshotId, Timestamp, Uuid};
use sqlx::Row;
use sqlx::sqlite::SqliteRow;

use crate::codec::{self, BadColumn};
use crate::locations;
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

    fn blob(&self, column: &str) -> Result<Option<Vec<u8>>, StoreError> {
        self.row
            .try_get::<Option<Vec<u8>>, _>(column)
            .map_err(|error| self.corrupt(column, unreadable(&error)))
    }

    /// What started the operation: a trigger word, and for an API token the
    /// token's ID and label beside it, which no other trigger leaves set.
    fn trigger(&self) -> Result<Trigger, StoreError> {
        let word = self.required_text("trigger")?;
        let id = self.text("token_id")?;
        let label = self.text("token_label")?;
        if word == codec::API_TOKEN {
            let id = id.ok_or_else(|| self.corrupt("token_id", "is NULL"))?;
            let id = ApiTokenId::parse(&id).ok_or_else(|| {
                self.corrupt("token_id", format!("{} is not an API token ID", shown(&id)))
            })?;
            let label = label.ok_or_else(|| self.corrupt("token_label", "is NULL"))?;
            let label = ApiTokenLabel::new(label)
                .map_err(|_| self.corrupt("token_label", "is not an API token label"))?;
            return Ok(Trigger::ApiToken { id, label });
        }
        let trigger = codec::trigger_from_text(&word)
            .ok_or_else(|| self.corrupt("trigger", format!("{} is not a trigger", shown(&word))))?;
        for (column, stray) in [("token_id", &id), ("token_label", &label)] {
            if stray.is_some() {
                return Err(self.corrupt(column, "is set on an operation no API token started"));
            }
        }
        Ok(trigger)
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

    /// The run's choice of location, which must agree with the position of
    /// the one it used: both columns are set, or neither is when the run
    /// used none.
    fn locations(&self) -> Result<Option<LocationsRecord>, StoreError> {
        let used = self.integer("location_no")?;
        let Some(text) = self.text("locations_json")? else {
            return match used {
                None => Ok(None),
                Some(_) => Err(self.corrupt("location_no", "is set, but no location is recorded")),
            };
        };
        let record = locations::decode(&text).map_err(|why| self.corrupt("locations_json", why))?;
        if used != record.used().map(i64::from) {
            return Err(self.corrupt(
                "location_no",
                "is not the position of the location the run used",
            ));
        }
        Ok(Some(record))
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

/// A row of `retry_state`: a plan's pending retry. Its attempt counts from
/// 1, as the store writes it.
pub(crate) fn pending_retry(row: &SqliteRow) -> Result<PendingRetry, StoreError> {
    let reader = Reader {
        row,
        scope: "a pending retry".to_owned(),
    };
    let plan = reader
        .id_of("plan_id", "plan ID", PlanId::parse)?
        .ok_or_else(|| reader.corrupt("plan_id", "is NULL"))?;
    let reader = Reader {
        row,
        scope: format!("the pending retry of {}", plan.as_str()),
    };
    let counted = reader
        .integer("attempt")?
        .ok_or_else(|| reader.corrupt("attempt", "is NULL"))?;
    let attempt = u32::try_from(counted)
        .ok()
        .filter(|attempt| *attempt >= 1)
        .ok_or_else(|| reader.corrupt("attempt", format!("{counted} is not an attempt")))?;
    Ok(PendingRetry {
        plan,
        retry: RetryScheduled {
            attempt,
            at: reader.required_time("retry_at")?,
            until: reader.time("until")?,
        },
    })
}

/// The row of `release_check`. A version this version couldn't have written
/// is [`StoreError::Corrupt`]; the next check writes the row anew.
pub(crate) fn release_check(row: &SqliteRow) -> Result<ReleaseCheck, StoreError> {
    let reader = Reader {
        row,
        scope: "the release check".to_owned(),
    };
    let update = reader
        .text("update_version")?
        .map(|text| {
            Version::parse(&text).map_err(|_| {
                reader.corrupt("update_version", format!("{} is no version", shown(&text)))
            })
        })
        .transpose()?;
    Ok(ReleaseCheck {
        at: reader.required_time("checked_at")?,
        update,
    })
}

/// A row of `api_tokens`. A token whose label, scopes or digest this
/// version couldn't have written is [`StoreError::Corrupt`], never a token
/// that lets more or less than it was given.
pub(crate) fn api_token(row: &SqliteRow) -> Result<ApiToken, StoreError> {
    let mut reader = Reader {
        row,
        scope: "an API token".to_owned(),
    };
    let id_text = reader.required_text("id")?;
    let id = ApiTokenId::parse(&id_text).ok_or_else(|| {
        reader.corrupt("id", format!("{} is not an API token ID", shown(&id_text)))
    })?;
    reader.scope = format!("API token {}", id.as_str());
    let label = ApiTokenLabel::new(reader.required_text("label")?)
        .map_err(|_| reader.corrupt("label", "is not an API token label"))?;
    let scopes_text = reader.required_text("scopes")?;
    let scopes = scopes_text
        .split(',')
        .map(|name| {
            Scope::parse(name)
                .ok_or_else(|| reader.corrupt("scopes", format!("{} is not a scope", shown(name))))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let scopes = Scopes::new(scopes).map_err(|_| reader.corrupt("scopes", "names no scope"))?;
    let digest = reader
        .blob("digest")?
        .ok_or_else(|| reader.corrupt("digest", "is NULL"))?;
    let digest = <[u8; 32]>::try_from(digest.as_slice())
        .map_err(|_| reader.corrupt("digest", format!("is {} bytes, not 32", digest.len())))?;
    let password = reader
        .integer("password")?
        .ok_or_else(|| reader.corrupt("password", "is NULL"))?;
    Ok(ApiToken {
        id,
        label,
        scopes,
        digest: ApiTokenDigest::from_bytes(digest),
        password: PasswordFingerprint::new(password.cast_unsigned()),
        created_at: reader.required_time("created_at")?,
        last_used_at: reader.time("last_used_at")?,
    })
}

/// The session length as `sql::SESSION_LENGTH` reads it. A name this version
/// didn't write is [`StoreError::Corrupt`], never the default length.
pub(crate) fn session_length(row: &SqliteRow) -> Result<SessionLength, StoreError> {
    let reader = Reader {
        row,
        scope: "the sign-in settings".to_owned(),
    };
    let name = reader.required_text("session_length")?;
    SessionLength::parse(&name).ok_or_else(|| {
        reader.corrupt(
            "session_length",
            format!("{} is no session length", shown(&name)),
        )
    })
}

/// A session as `sql::ALL_SESSIONS` reads it. A report names it by its row,
/// never by its digest.
pub(crate) fn kept_session(row: &SqliteRow) -> Result<KeptSession, StoreError> {
    let reader = Reader {
        row,
        scope: "a session".to_owned(),
    };
    let digest = reader
        .blob("digest")?
        .ok_or_else(|| reader.corrupt("digest", "is NULL"))?;
    let digest = <[u8; 32]>::try_from(digest.as_slice())
        .map_err(|_| reader.corrupt("digest", format!("is {} bytes, not 32", digest.len())))?;
    let password = reader
        .integer("password")?
        .ok_or_else(|| reader.corrupt("password", "is NULL"))?;
    Ok(KeptSession {
        digest: SessionDigest::from_bytes(digest),
        password: PasswordFingerprint::new(password.cast_unsigned()),
        started: reader.required_time("started_at")?,
        last_used: reader.required_time("last_used_at")?,
        keep_signed_in: flag(&reader, "keep_signed_in")?,
        secure: flag(&reader, "secure")?,
    })
}

/// A reading of a repository's stats as `sql::LATEST_STATS` and
/// `sql::STATS_HISTORY` read it. A report names it by its row.
pub(crate) fn stats_reading(row: &SqliteRow) -> Result<StatsReading, StoreError> {
    let mut reader = Reader {
        row,
        scope: "a stats reading".to_owned(),
    };
    if let Some(id) = reader.integer("id")? {
        reader.scope = format!("stats reading {id}");
    }
    let alias = reader
        .id_of("repo_alias", "repository alias", RepositoryAlias::parse)?
        .ok_or_else(|| reader.corrupt("repo_alias", "is NULL"))?;
    Ok(StatsReading {
        alias,
        repository: reader.id_of("repo_uuid", "repository ID", RepositoryId::parse)?,
        taken_at: reader.required_time("taken_at")?,
        stats: RepositoryStats {
            snapshots: figure(&reader, "snapshots")?,
            total_size: figure(&reader, "total_bytes")?,
            stored_size: figure(&reader, "stored_bytes")?,
            raw_size: figure(&reader, "raw_bytes")?,
        },
    })
}

/// A count or a size, which is never negative.
fn figure(reader: &Reader<'_>, column: &str) -> Result<u64, StoreError> {
    let value = reader
        .integer(column)?
        .ok_or_else(|| reader.corrupt(column, "is NULL"))?;
    u64::try_from(value).map_err(|_| reader.corrupt(column, format!("{value} is below 0")))
}

/// A column that holds 1 for yes and 0 for no; anything else is corrupt.
fn flag(reader: &Reader<'_>, column: &str) -> Result<bool, StoreError> {
    match reader.integer(column)? {
        Some(0) => Ok(false),
        Some(1) => Ok(true),
        Some(other) => Err(reader.corrupt(column, format!("is {other}, not 0 or 1"))),
        None => Err(reader.corrupt(column, "is NULL")),
    }
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
    let trigger = reader.trigger()?;
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
    let locations = reader.locations()?;

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
        warning_kinds: Vec::new(),
        result,
        repository_alias,
        locations,
        entry: None,
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

/// One row of a page's warning codes: the operation's ID as stored, and the
/// kind of one of its kept warnings.
pub(crate) fn warning_kind(row: &SqliteRow) -> Result<(String, WarningKind), StoreError> {
    let page = Reader {
        row,
        scope: "the warning codes of a page of history".to_owned(),
    };
    let operation = page.required_text("op_id")?;
    let reader = Reader {
        row,
        scope: format!("operation {}, its warning codes", shown(&operation)),
    };
    let code = reader.required_text("code")?;
    Ok((operation, codec::warning_kind_from_text(&code)))
}

/// A proposal as `sql::GET_PROPOSAL` and `sql::ALL_PROPOSALS` read it. Its
/// proposer is a token's ID and label, both set, or neither for a client
/// with no token.
pub(crate) fn proposal(row: &SqliteRow) -> Result<StoredProposal, StoreError> {
    let mut reader = Reader {
        row,
        scope: "a proposal".to_owned(),
    };
    let id_text = reader.required_text("id")?;
    let id = ProposalId::parse(&id_text)
        .ok_or_else(|| reader.corrupt("id", format!("{} is not a proposal ID", shown(&id_text))))?;
    reader.scope = format!("proposal {}", id.as_str());
    let proposer = match (reader.text("token_id")?, reader.text("token_label")?) {
        (None, None) => Proposer::NoToken,
        (Some(token), Some(label)) => Proposer::Token {
            id: ApiTokenId::parse(&token).ok_or_else(|| {
                reader.corrupt(
                    "token_id",
                    format!("{} is not an API token ID", shown(&token)),
                )
            })?,
            label: ApiTokenLabel::new(label)
                .map_err(|_| reader.corrupt("token_label", "is not an API token label"))?,
        },
        (Some(_), None) => return Err(reader.corrupt("token_label", "is NULL")),
        (None, Some(_)) => return Err(reader.corrupt("token_id", "is NULL")),
    };
    let rationale = Rationale::parse(reader.required_text("rationale")?)
        .map_err(|_| reader.corrupt("rationale", "is too long"))?;
    let status_text = reader.required_text("status")?;
    let status = ProposalStatus::parse(&status_text).ok_or_else(|| {
        reader.corrupt("status", format!("{} is not a status", shown(&status_text)))
    })?;
    let base_text = reader.required_text("base")?;
    let base = ConfigRevision::parse(&base_text).ok_or_else(|| {
        reader.corrupt("base", format!("{} is not a revision", shown(&base_text)))
    })?;
    Ok(StoredProposal {
        id,
        proposer,
        base,
        changes: reader.required_text("changes")?,
        rationale,
        status,
        created_at: reader.required_time("created_at")?,
        expires_at: reader.required_time("expires_at")?,
        decided_at: reader.time("decided_at")?,
        reasons: proposal_reasons(&reader)?,
        approved_from: reader.id_of("approved_from", "network address", str::parse)?,
        applied_as: reader.text("applied_as")?,
    })
}

/// A rejection's reasons, kept as a JSON array of strings; none when the
/// column is NULL.
fn proposal_reasons(reader: &Reader<'_>) -> Result<Vec<RejectReason>, StoreError> {
    let Some(text) = reader.text("reasons")? else {
        return Ok(Vec::new());
    };
    let reasons: Vec<String> = serde_json::from_str(&text)
        .map_err(|_| reader.corrupt("reasons", "is not a JSON array of strings"))?;
    reasons
        .into_iter()
        .map(|reason| {
            RejectReason::parse(reason)
                .ok_or_else(|| reader.corrupt("reasons", "holds one too long"))
        })
        .collect()
}
