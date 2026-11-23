-- The hub's latest plans and progress of each agent (technical design
-- §7.2, #1440): each snapshot of an agent's plans replaces the one before,
-- in its order, with how many plans it left out; its latest progress
-- replaces the one before, of whichever operation.

CREATE TABLE sync_plans (
    agent_id TEXT NOT NULL,
    place INTEGER NOT NULL CHECK (place >= 0),
    plan_id TEXT NOT NULL,
    last_at INTEGER,
    last_outcome TEXT,
    next_run INTEGER,
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
    current INTEGER NOT NULL,
    total INTEGER,
    at INTEGER NOT NULL
) STRICT;
