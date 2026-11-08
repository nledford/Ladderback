-- The operation log (technical design §9): every operation the agent ran and
-- the warnings it recorded. Only the columns the log uses so far; later
-- migrations add the rest of §9 (the queue index, the maintenance lock, the
-- operation logs) when the code that needs them lands.

CREATE TABLE operations (
  id            TEXT PRIMARY KEY NOT NULL,   -- UUIDv7, lowercase hyphenated: its text order is time order
  agent_id      TEXT NOT NULL,               -- 'local' on agents (§9)
  kind          TEXT NOT NULL,               -- fake|identify|backup|prune|check
  plan_id       TEXT,
  repo_uuid     TEXT,                        -- the rustic repository ID
  trigger       TEXT NOT NULL,               -- schedule|manual|catch-up|retry|hub|mcp
  status        TEXT NOT NULL,               -- queued|running|succeeded|succeeded_with_warnings|failed|cancelled
  queued_at     INTEGER NOT NULL,            -- Unix MILLISECONDS, UTC
  started_at    INTEGER,
  finished_at   INTEGER,
  snapshot_id   TEXT,                        -- a backup result's snapshot
  summary_json  TEXT,                        -- the engine's result, as JSON
  error_code    TEXT,                        -- the failure's api::ErrorCode (kebab-case); failed operations only
  error_args    TEXT,                        -- JSON array of [name, MsgArg] pairs; failed operations only
  error_detail  TEXT,                        -- the engine's or platform's own text, verbatim; failed operations only
  warning_count INTEGER NOT NULL DEFAULT 0   -- every warning recorded, the rows of op_warnings or not
);

-- History is newest first by ID, one plan at a time or all together. §9 names
-- (plan_id, started_at DESC), but a queued operation has not started, and a
-- page continues from an ID.
CREATE INDEX ops_by_plan ON operations(plan_id, id DESC);

-- At most 10,000 rows per operation; warning_count keeps the total.
CREATE TABLE op_warnings (
  op_id  TEXT NOT NULL REFERENCES operations(id) ON DELETE CASCADE,
  item   TEXT,                                -- NULL and '' are different things
  during TEXT,
  code   TEXT,                                -- the api::WarningCode (kebab-case)
  detail TEXT
);

CREATE INDEX op_warnings_by_op ON op_warnings(op_id);
