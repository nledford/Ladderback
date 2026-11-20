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
            "warning_count, repo_alias, location_no, locations_json, token_id, token_label, ",
            "entry ",
            "FROM operations",
            $($clause),+
        )
    };
}

/// Writes an operation, or replaces the one with its ID: every column, so
/// nothing of an earlier save survives. Parameters, in order: `id`, `agent_id`,
/// `kind`, `plan_id`, `repo_uuid`, `trigger`, `status`, `queued_at`,
/// `started_at`, `finished_at`, `snapshot_id`, `summary_json`, `error_code`,
/// `error_args`, `error_detail`, `warning_count`, `repo_alias`, `location_no`,
/// `locations_json`, `token_id`, `token_label`, `entry`.
pub(crate) const UPSERT_OPERATION: &str = concat!(
    "INSERT INTO operations (id, agent_id, kind, plan_id, repo_uuid, \"trigger\", status, ",
    "queued_at, started_at, finished_at, snapshot_id, summary_json, error_code, error_args, ",
    "error_detail, warning_count, repo_alias, location_no, locations_json, token_id, ",
    "token_label, entry) ",
    "VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) ",
    "ON CONFLICT(id) DO UPDATE SET ",
    "agent_id = excluded.agent_id, kind = excluded.kind, plan_id = excluded.plan_id, ",
    "repo_uuid = excluded.repo_uuid, \"trigger\" = excluded.\"trigger\", ",
    "status = excluded.status, queued_at = excluded.queued_at, ",
    "started_at = excluded.started_at, finished_at = excluded.finished_at, ",
    "snapshot_id = excluded.snapshot_id, summary_json = excluded.summary_json, ",
    "error_code = excluded.error_code, error_args = excluded.error_args, ",
    "error_detail = excluded.error_detail, warning_count = excluded.warning_count, ",
    "repo_alias = excluded.repo_alias, location_no = excluded.location_no, ",
    "locations_json = excluded.locations_json, token_id = excluded.token_id, ",
    "token_label = excluded.token_label, entry = excluded.entry"
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

/// The ID of the maintenance operation running on a repository, the one that
/// holds its I7 slot. Parameter: the repository. The literals are those of
/// the `one_maintenance_per_repo` index, so it serves the lookup.
pub(crate) const FIND_MAINTENANCE_RUNNING_ON: &str = concat!(
    "SELECT id FROM operations WHERE repo_uuid = ? AND status = 'running' ",
    "AND kind IN ('forget', 'adopt', 'prune', 'check') LIMIT 1"
);

/// The ID of a maintenance operation running on any repository that names
/// one, as the slot is held only by those.
pub(crate) const FIND_ANY_MAINTENANCE_RUNNING: &str = concat!(
    "SELECT id FROM operations WHERE repo_uuid IS NOT NULL AND status = 'running' ",
    "AND kind IN ('forget', 'adopt', 'prune', 'check') LIMIT 1"
);

/// Parameter: the operation's ID.
pub(crate) const GET_OPERATION: &str = select_operations!(" WHERE id = ?");

/// Parameters: the operation's ID, and the most warnings to read.
pub(crate) const GET_WARNINGS: &str =
    "SELECT item, during, code, detail FROM op_warnings WHERE op_id = ? ORDER BY rowid LIMIT ?";

/// The warning codes of a page of operations, each code of each operation
/// once, in the order the first warning of each was recorded (#508): the
/// kept warnings are the stored ones. Parameter: the operations' IDs as a
/// JSON array.
pub(crate) const LIST_WARNING_KINDS: &str = concat!(
    "SELECT op_id, code, MIN(rowid) AS first FROM op_warnings ",
    "WHERE op_id IN (SELECT value FROM json_each(?)) ",
    "GROUP BY op_id, code ORDER BY first"
);

/// Ends every running operation as failed (startup recovery, and the
/// daemon's orderly stop), and nothing else about it but its finish time:
/// not its start, warnings or target, and returns the ID of each it ended,
/// in no set order. Parameters: `status`, `error_code`, `error_args`,
/// `error_detail`, `finished_at` (NULL for recovery, which doesn't know when
/// the daemon stopped), then the status it ends, running. A
/// running maintenance row stops being one, so the one-maintenance index
/// stops holding its repository.
pub(crate) const FAIL_RUNNING: &str = concat!(
    "UPDATE operations SET status = ?, error_code = ?, error_args = ?, error_detail = ?, ",
    "finished_at = ? WHERE status = ? RETURNING id"
);

/// The persistent queue (§5), oldest first: the queued operations by ID,
/// which is time order. The status is a literal, so SQLite matches the
/// statement to the partial index `ops_queue` when it prepares it, and the
/// read doesn't grow with the history.
pub(crate) const LIST_QUEUED: &str = select_operations!(" WHERE status = 'queued' ORDER BY id");

/// Every plan's planned run, in plan order.
pub(crate) const PLANNED_RUNS: &str =
    "SELECT plan_id, next_run FROM schedule_state ORDER BY plan_id";

/// Keeps a plan's planned run, replacing the one kept before. Parameters:
/// `plan_id`, `next_run`.
pub(crate) const PLAN_RUN: &str = concat!(
    "INSERT INTO schedule_state (plan_id, next_run) VALUES (?, ?) ",
    "ON CONFLICT(plan_id) DO UPDATE SET next_run = excluded.next_run"
);

/// Forgets a plan's planned run. Parameter: `plan_id`.
pub(crate) const UNPLAN: &str = "DELETE FROM schedule_state WHERE plan_id = ?";

/// Every plan's pending retry, in the order of the plans' IDs.
pub(crate) const PENDING_RETRIES: &str =
    "SELECT plan_id, attempt, retry_at, until FROM retry_state ORDER BY plan_id";

/// Keeps a plan's pending retry, replacing the one kept before. Parameters:
/// `plan_id`, `attempt`, `retry_at`, `until`.
pub(crate) const KEEP_RETRY: &str = concat!(
    "INSERT INTO retry_state (plan_id, attempt, retry_at, until) VALUES (?, ?, ?, ?) ",
    "ON CONFLICT(plan_id) DO UPDATE SET attempt = excluded.attempt, ",
    "retry_at = excluded.retry_at, until = excluded.until"
);

/// Forgets a plan's pending retry. Parameter: `plan_id`.
pub(crate) const DROP_RETRY: &str = "DELETE FROM retry_state WHERE plan_id = ?";

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

/// Whether any operation of a plan succeeded, with warnings or without.
/// Parameter: the plan. The statuses are literals, not parameters, so SQLite
/// can see when it prepares the statement that they are those of the partial
/// index `ops_succeeded_by_plan`, and answers from it: one probe, however
/// many operations the plan has failed.
pub(crate) const EVER_SUCCEEDED: &str = concat!(
    "SELECT EXISTS (SELECT 1 FROM operations ",
    "WHERE plan_id = ? AND status IN ('succeeded', 'succeeded_with_warnings'))"
);

/// The columns `row::api_token` reads.
macro_rules! select_api_tokens {
    ($($clause:literal),+ $(,)?) => {
        concat!(
            "SELECT id, label, digest, permissions, password, created_at, last_used_at ",
            "FROM api_tokens",
            $($clause),+
        )
    };
}

/// How many API tokens are kept.
pub(crate) const COUNT_API_TOKENS: &str = "SELECT COUNT(*) FROM api_tokens";

/// Keeps a new API token. Parameters: `id`, `label`, `digest`,
/// `permissions`, `password`, `created_at`, `last_used_at`.
pub(crate) const INSERT_API_TOKEN: &str = concat!(
    "INSERT INTO api_tokens (id, label, digest, permissions, password, created_at, last_used_at) ",
    "VALUES (?, ?, ?, ?, ?, ?, ?)"
);

/// Gives a kept API token other permissions. Parameters: the permissions,
/// the token's ID.
pub(crate) const SET_API_TOKEN_PERMISSIONS: &str =
    "UPDATE api_tokens SET permissions = ? WHERE id = ?";

/// Parameter: the token's ID.
pub(crate) const GET_API_TOKEN: &str = select_api_tokens!(" WHERE id = ?");

/// Every API token, oldest first; of two created in the same millisecond,
/// the one kept first.
pub(crate) const ALL_API_TOKENS: &str = select_api_tokens!(" ORDER BY created_at, rowid");

/// Parameter: the token's ID.
pub(crate) const DELETE_API_TOKEN: &str = "DELETE FROM api_tokens WHERE id = ?";

/// Forgets the tokens of every other admin password. Parameter: the
/// fingerprint of the one kept now.
pub(crate) const DELETE_OTHER_PASSWORDS_TOKENS: &str = "DELETE FROM api_tokens WHERE password <> ?";

/// Parameters: the last use, the token's ID.
pub(crate) const RECORD_API_TOKEN_USE: &str = "UPDATE api_tokens SET last_used_at = ? WHERE id = ?";

/// Keeps a session, in place of one of the same digest. Parameters: `digest`,
/// `password`, `started_at`, `last_used_at`, `keep_signed_in`, `secure`.
pub(crate) const INSERT_SESSION: &str = concat!(
    "INSERT OR REPLACE INTO sessions ",
    "(digest, password, started_at, last_used_at, keep_signed_in, secure) ",
    "VALUES (?, ?, ?, ?, ?, ?)"
);

/// Every session, with the `rowid` that names a row whose digest can't be
/// read; the columns `row::kept_session` reads.
pub(crate) const ALL_SESSIONS: &str = concat!(
    "SELECT rowid, digest, password, started_at, last_used_at, keep_signed_in, secure ",
    "FROM sessions"
);

/// Parameter: the session's digest.
pub(crate) const DELETE_SESSION: &str = "DELETE FROM sessions WHERE digest = ?";

/// Parameter: the row's `rowid`.
pub(crate) const DELETE_SESSION_ROW: &str = "DELETE FROM sessions WHERE rowid = ?";

pub(crate) const DELETE_ALL_SESSIONS: &str = "DELETE FROM sessions";

/// Parameters: the last use, the session's digest.
pub(crate) const RECORD_SESSION_USE: &str = "UPDATE sessions SET last_used_at = ? WHERE digest = ?";

/// The session length the admin set; no row while none has been set.
pub(crate) const SESSION_LENGTH: &str = "SELECT session_length FROM sign_in_settings WHERE id = 1";

/// Keeps the session length, in place of the one kept. Parameter: its name.
pub(crate) const SET_SESSION_LENGTH: &str = concat!(
    "INSERT INTO sign_in_settings (id, session_length) VALUES (1, ?) ",
    "ON CONFLICT(id) DO UPDATE SET session_length = excluded.session_length"
);

/// Keeps a reading of a repository's stats. Parameters: `repo_alias`,
/// `repo_uuid`, `taken_at`, `snapshots`, `total_bytes`, `stored_bytes`,
/// `raw_bytes`.
pub(crate) const INSERT_STATS: &str = concat!(
    "INSERT INTO repo_stats ",
    "(repo_alias, repo_uuid, taken_at, snapshots, total_bytes, stored_bytes, raw_bytes) ",
    "VALUES (?, ?, ?, ?, ?, ?, ?)"
);

/// Drops the readings a newer one no longer keeps. Parameters: the alias,
/// the oldest `taken_at` still kept.
pub(crate) const DROP_OLD_STATS: &str =
    "DELETE FROM repo_stats WHERE repo_alias = ? AND taken_at < ?";

/// The columns `row::stats_reading` reads.
macro_rules! select_stats {
    ($tail:literal) => {
        concat!(
            "SELECT id, repo_alias, repo_uuid, taken_at, snapshots, total_bytes, stored_bytes, ",
            "raw_bytes FROM repo_stats WHERE repo_alias = ?",
            $tail
        )
    };
}

/// The reading of an alias taken last, the one kept last among equals.
/// Parameter: the alias.
pub(crate) const LATEST_STATS: &str = select_stats!(" ORDER BY taken_at DESC, id DESC LIMIT 1");

/// Every reading of an alias, oldest first, in the order kept among equals.
/// Parameter: the alias.
pub(crate) const STATS_HISTORY: &str = select_stats!(" ORDER BY taken_at, id");

/// The last release check; no row before the first.
pub(crate) const RELEASE_CHECK: &str =
    "SELECT checked_at, update_version FROM release_check WHERE id = 1";

/// Keeps the release check, in place of the one kept. Parameters:
/// `checked_at`, `update_version`.
pub(crate) const KEEP_RELEASE_CHECK: &str = concat!(
    "INSERT INTO release_check (id, checked_at, update_version) VALUES (1, ?, ?) ",
    "ON CONFLICT(id) DO UPDATE SET checked_at = excluded.checked_at, ",
    "update_version = excluded.update_version"
);

/// How many proposals one proposer has pending at the moment a new one is
/// made: those past their expiry are left out, though nothing has marked
/// them expired yet. Parameters: the proposer's token ID, NULL for the
/// clients with no token (`IS` matches NULL to NULL), and the new
/// proposal's `created_at`. The status is a literal, so the statement
/// serves from `proposals_by_status`.
pub(crate) const COUNT_PENDING_PROPOSALS: &str = concat!(
    "SELECT COUNT(*) FROM proposals ",
    "WHERE status = 'pending' AND token_id IS ? AND expires_at > ?"
);

/// When one proposer made each of its proposals still kept, for the limit
/// on proposals an hour; the ended ones forgotten beyond the newest few
/// hundred bound it. Parameter: the proposer's token ID, as above.
pub(crate) const PROPOSALS_MADE: &str = "SELECT created_at FROM proposals WHERE token_id IS ?";

/// When a person last rejected one of a proposer's proposals, NULL if
/// never. Parameter: the proposer's token ID, as above.
pub(crate) const LAST_REJECTION: &str = concat!(
    "SELECT MAX(decided_at) FROM proposals ",
    "WHERE status = 'rejected' AND token_id IS ?"
);

/// Keeps a new proposal. Parameters: `id`, `token_id`, `token_label`,
/// `base`, `changes`, `rationale`, `status`, `created_at`, `expires_at`.
pub(crate) const INSERT_PROPOSAL: &str = concat!(
    "INSERT INTO proposals ",
    "(id, token_id, token_label, base, changes, rationale, status, created_at, expires_at) ",
    "VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)"
);

/// Forgets the ended proposals beyond the newest so many, the oldest first
/// and, among those made in the same millisecond, by ID. Parameter: how many
/// ended ones are kept.
pub(crate) const FORGET_OLD_PROPOSALS: &str = concat!(
    "DELETE FROM proposals WHERE id IN (",
    "SELECT id FROM proposals WHERE status NOT IN ('pending', 'applying') ",
    "ORDER BY created_at DESC, id DESC LIMIT -1 OFFSET ?)"
);

/// The columns `row::proposal` reads.
macro_rules! select_proposals {
    ($clause:literal) => {
        concat!(
            "SELECT id, token_id, token_label, base, changes, rationale, status, created_at, ",
            "expires_at, decided_at, reasons, approved_from, applied_as FROM proposals",
            $clause
        )
    };
}

/// Parameter: the proposal's ID.
pub(crate) const GET_PROPOSAL: &str = select_proposals!(" WHERE id = ?");

/// Every proposal, newest first; of two made in the same millisecond, the
/// one kept last.
pub(crate) const ALL_PROPOSALS: &str = select_proposals!(" ORDER BY created_at DESC, rowid DESC");

/// Parameters: the new status, the proposal's ID, the status it must have.
pub(crate) const CHANGE_PROPOSAL_STATUS: &str =
    "UPDATE proposals SET status = ? WHERE id = ? AND status = ?";

/// Rejects a pending proposal. Parameters: `decided_at`, the reasons as a
/// JSON array, the proposal's ID.
pub(crate) const REJECT_PROPOSAL: &str = concat!(
    "UPDATE proposals SET status = 'rejected', decided_at = ?, reasons = ? ",
    "WHERE id = ? AND status = 'pending'"
);

/// Starts applying a pending proposal. Parameters: `decided_at`,
/// `approved_from`, the proposal's ID.
pub(crate) const APPROVE_PROPOSAL: &str = concat!(
    "UPDATE proposals SET status = 'applying', decided_at = ?, approved_from = ? ",
    "WHERE id = ? AND status = 'pending'"
);

/// Marks a proposal being applied applied. Parameters: `applied_as`, the
/// proposal's ID.
pub(crate) const PROPOSAL_APPLIED: &str = concat!(
    "UPDATE proposals SET status = 'applied', applied_as = ? ",
    "WHERE id = ? AND status = 'applying'"
);

/// The hub's registry (`pairing_tokens`, `agents`), which only a build with
/// the hub reads and writes (§16, decision 122).
#[cfg(feature = "hub")]
pub(crate) mod hub {
    /// A `SELECT` of the columns `row::pairing_token` reads, followed by
    /// `clause`s.
    macro_rules! select_tokens {
        ($($clause:literal),+ $(,)?) => {
            concat!(
                "SELECT pairing_key, hub_key, kind, made_at, expires_at, state, ",
                "used_agent_id, used_agent_key FROM pairing_tokens",
                $($clause),+
            )
        };
    }

    /// A `SELECT` of the columns `row::agent` reads, followed by `clause`s.
    macro_rules! select_agents {
        ($($clause:literal),+ $(,)?) => {
            concat!(
                "SELECT id, pubkey, hostname, status, status_at, counter, last_seen ",
                "FROM agents",
                $($clause),+
            )
        };
    }

    /// How many tokens are unused at a moment. Parameter: the moment.
    pub(crate) const COUNT_UNUSED_TOKENS: &str =
        "SELECT COUNT(*) FROM pairing_tokens WHERE state = 'unused' AND expires_at > ?";

    /// Keeps a token. Parameters, in order: every column of
    /// `select_tokens!`.
    pub(crate) const INSERT_TOKEN: &str = concat!(
        "INSERT INTO pairing_tokens (pairing_key, hub_key, kind, made_at, expires_at, state, ",
        "used_agent_id, used_agent_key) VALUES (?, ?, ?, ?, ?, ?, ?, ?)"
    );

    /// The tokens unused at a moment, oldest first. Parameter: the moment.
    pub(crate) const UNUSED_TOKENS: &str = select_tokens!(
        " WHERE state = 'unused' AND expires_at > ?",
        " ORDER BY made_at, rowid"
    );

    /// The token with a pairing key. Parameter: the key.
    pub(crate) const TOKEN: &str = select_tokens!(" WHERE pairing_key = ?");

    /// Revokes a token still unused. Parameter: its pairing key.
    pub(crate) const REVOKE_TOKEN: &str =
        "UPDATE pairing_tokens SET state = 'revoked' WHERE pairing_key = ? AND state = 'unused'";

    /// Revokes every token still unused, as a new hub key does.
    pub(crate) const REVOKE_UNUSED_TOKENS: &str =
        "UPDATE pairing_tokens SET state = 'revoked' WHERE state = 'unused'";

    /// Spends a token on a pairing. Parameters: the agent ID, the agent
    /// key, the token's pairing key.
    pub(crate) const SPEND_TOKEN: &str = concat!(
        "UPDATE pairing_tokens SET state = 'used', used_agent_id = ?, used_agent_key = ? ",
        "WHERE pairing_key = ?"
    );

    /// The tokens spent on an agent ID and key. Parameters: the agent ID,
    /// the agent key.
    pub(crate) const SPENT_TOKENS: &str =
        select_tokens!(" WHERE state = 'used' AND used_agent_id = ? AND used_agent_key = ?");

    /// The agent with an ID. Parameter: the ID.
    pub(crate) const AGENT: &str = select_agents!(" WHERE id = ?");

    /// The agent whose hostname has a key. Parameter: the key.
    pub(crate) const AGENT_BY_HOSTNAME: &str = select_agents!(" WHERE hostname_key = ?");

    /// How many agents wait for approval.
    pub(crate) const COUNT_PENDING_AGENTS: &str =
        "SELECT COUNT(*) FROM agents WHERE status = 'pending'";

    /// How many agents are registered, pending or approved.
    pub(crate) const COUNT_AGENTS: &str = "SELECT COUNT(*) FROM agents";

    /// Every agent, oldest first.
    pub(crate) const ALL_AGENTS: &str = select_agents!(" ORDER BY rowid");

    /// Keeps a new agent. Parameters, in order: `id`, `pubkey`, `hostname`,
    /// `hostname_key`, `status`, `status_at`, `counter`, `last_seen`.
    pub(crate) const INSERT_AGENT: &str = concat!(
        "INSERT INTO agents (id, pubkey, hostname, hostname_key, status, status_at, counter, ",
        "last_seen) VALUES (?, ?, ?, ?, ?, ?, ?, ?)"
    );

    /// Writes an agent's record over the one kept under its ID, every
    /// column but the ID. Parameters, in order: `pubkey`, `hostname`,
    /// `hostname_key`, `status`, `status_at`, `counter`, `last_seen`, `id`.
    pub(crate) const UPDATE_AGENT: &str = concat!(
        "UPDATE agents SET pubkey = ?, hostname = ?, hostname_key = ?, status = ?, ",
        "status_at = ?, counter = ?, last_seen = ? WHERE id = ?"
    );
}
