//! The store's SQL. Every statement is a whole static string with bound
//! parameters: the one choice made at run time is which string to run, never
//! a value formatted into one.

/// A `SELECT` of the columns `row::record` reads, from `operations`, followed
/// by `clause`s.
macro_rules! select_operations {
    ($($clause:literal),+ $(,)?) => {
        concat!(
            "SELECT id, kind, plan_id, repo_uuid, \"trigger\", status, queued_at, started_at, ",
            "finished_at, snapshot_id, summary_json, error_code, error_args, error_detail, ",
            "warning_count FROM operations",
            $($clause),+
        )
    };
}

/// Writes an operation, or replaces the one with its ID: every column, so
/// nothing of an earlier save survives. Parameters, in order: `id`, `agent_id`,
/// `kind`, `plan_id`, `repo_uuid`, `trigger`, `status`, `queued_at`,
/// `started_at`, `finished_at`, `snapshot_id`, `summary_json`, `error_code`,
/// `error_args`, `error_detail`, `warning_count`.
pub(crate) const UPSERT_OPERATION: &str = concat!(
    "INSERT INTO operations (id, agent_id, kind, plan_id, repo_uuid, \"trigger\", status, ",
    "queued_at, started_at, finished_at, snapshot_id, summary_json, error_code, error_args, ",
    "error_detail, warning_count) ",
    "VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) ",
    "ON CONFLICT(id) DO UPDATE SET ",
    "agent_id = excluded.agent_id, kind = excluded.kind, plan_id = excluded.plan_id, ",
    "repo_uuid = excluded.repo_uuid, \"trigger\" = excluded.\"trigger\", ",
    "status = excluded.status, queued_at = excluded.queued_at, ",
    "started_at = excluded.started_at, finished_at = excluded.finished_at, ",
    "snapshot_id = excluded.snapshot_id, summary_json = excluded.summary_json, ",
    "error_code = excluded.error_code, error_args = excluded.error_args, ",
    "error_detail = excluded.error_detail, warning_count = excluded.warning_count"
);

/// Parameter: the operation's ID.
pub(crate) const DELETE_WARNINGS: &str = "DELETE FROM op_warnings WHERE op_id = ?";

/// Parameters: the operation's ID, `item`, `during`, `code`, `detail`.
pub(crate) const INSERT_WARNING: &str =
    "INSERT INTO op_warnings (op_id, item, during, code, detail) VALUES (?, ?, ?, ?, ?)";

/// The ID of a running maintenance operation on a repository, other than the
/// operation being started. Parameters: the repository, the operation's own ID.
/// The literals are those of the `one_maintenance_per_repo` index, so it
/// serves the lookup.
pub(crate) const FIND_RUNNING_MAINTENANCE: &str = concat!(
    "SELECT id FROM operations WHERE repo_uuid = ? AND status = 'running' ",
    "AND kind IN ('forget', 'adopt', 'prune', 'check') AND id <> ? LIMIT 1"
);

/// Parameter: the operation's ID.
pub(crate) const GET_OPERATION: &str = select_operations!(" WHERE id = ?");

/// Parameters: the operation's ID, and the most warnings to read.
pub(crate) const GET_WARNINGS: &str =
    "SELECT item, during, code, detail FROM op_warnings WHERE op_id = ? ORDER BY rowid LIMIT ?";

/// Ends every queued or running operation as failed (startup recovery), and
/// nothing else about it: not its times, warnings or target. Parameters:
/// `status`, `error_code`, `error_args`, `error_detail`, then the two statuses
/// it ends, queued and running. A running maintenance row stops being one, so
/// the one-maintenance index stops holding its repository.
pub(crate) const FAIL_UNFINISHED: &str = concat!(
    "UPDATE operations SET status = ?, error_code = ?, error_args = ?, error_detail = ? ",
    "WHERE status IN (?, ?)"
);

// History, newest first, in four statements: with or without a plan, and with
// or without a cursor. The cursor is a term of its own, not an `IS NULL OR`
// alternative, so a later page starts at its place in the index rather than
// reading every newer row first.

/// Parameter: how many rows.
pub(crate) const LIST_FIRST_PAGE: &str = select_operations!(" ORDER BY id DESC LIMIT ?");

/// Parameters: the cursor, how many rows.
pub(crate) const LIST_AFTER: &str = select_operations!(" WHERE id < ? ORDER BY id DESC LIMIT ?");

/// Parameters: the plan, how many rows.
pub(crate) const LIST_PLAN_FIRST_PAGE: &str =
    select_operations!(" WHERE plan_id = ? ORDER BY id DESC LIMIT ?");

/// Parameters: the plan, the cursor, how many rows.
pub(crate) const LIST_PLAN_AFTER: &str =
    select_operations!(" WHERE plan_id = ? AND id < ? ORDER BY id DESC LIMIT ?");
