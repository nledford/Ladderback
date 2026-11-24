-- The hub's latest plans and progress of each agent (technical design
-- §7.2, #1440): each snapshot of an agent's plans replaces the one before,
-- in its order, with how many plans it left out; its latest progress
-- replaces the one before, of whichever operation. The agent's outbox
-- takes a cancel as an end too, so its progress leaves the dashboard:
-- SQLite changes no CHECK in place, so the table is built again with its
-- reports.

CREATE TABLE sync_outbox_cancels (
    seq INTEGER PRIMARY KEY CHECK (seq >= 1),
    op_id TEXT NOT NULL,
    happening TEXT NOT NULL
        CHECK (happening IN ('started', 'succeeded', 'failed', 'cancelled')),
    code TEXT,
    at INTEGER NOT NULL,
    kept INTEGER NOT NULL CHECK (kept IN (0, 1))
) STRICT;

INSERT INTO sync_outbox_cancels (seq, op_id, happening, code, at, kept)
SELECT seq, op_id, happening, code, at, kept FROM sync_outbox;

DROP TABLE sync_outbox;

ALTER TABLE sync_outbox_cancels RENAME TO sync_outbox;

CREATE INDEX sync_outbox_starts ON sync_outbox (seq) WHERE kept = 0;

CREATE TABLE sync_plans (
    agent_id TEXT NOT NULL,
    place INTEGER NOT NULL CHECK (place >= 0),
    plan_id TEXT NOT NULL,
    last_at INTEGER CHECK (last_at >= 0),
    last_outcome TEXT,
    next_run INTEGER CHECK (next_run >= 0),
    PRIMARY KEY (agent_id, place),
    CHECK ((last_at IS NULL) = (last_outcome IS NULL))
) STRICT;

CREATE TABLE sync_plan_snapshots (
    agent_id TEXT PRIMARY KEY,
    left_out INTEGER NOT NULL CHECK (left_out >= 0)
) STRICT;

CREATE TABLE sync_progress (
    agent_id TEXT PRIMARY KEY,
    op_id TEXT NOT NULL,
    step TEXT,
    unit TEXT NOT NULL,
    current INTEGER NOT NULL CHECK (current >= 0),
    total INTEGER CHECK (total >= 0),
    at INTEGER NOT NULL CHECK (at >= 0)
) STRICT;
