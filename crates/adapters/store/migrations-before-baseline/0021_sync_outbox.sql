-- The agent's outbox and the hub's reports of its agents (technical design
-- §7.2). `sync_outbox_meta` holds the outbox's one epoch and the last place
-- it gave, so a place is never given twice, even after its report is gone.
CREATE TABLE sync_outbox_meta (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    epoch TEXT NOT NULL,
    last_seq INTEGER NOT NULL CHECK (last_seq >= 0)
) STRICT;

-- The reports the hub hasn't acknowledged, by their place. `kept` is 1 for
-- an end, which a full outbox never gives up.
CREATE TABLE sync_outbox (
    seq INTEGER PRIMARY KEY CHECK (seq >= 1),
    op_id TEXT NOT NULL,
    happening TEXT NOT NULL CHECK (happening IN ('started', 'succeeded', 'failed')),
    code TEXT,
    at INTEGER NOT NULL,
    kept INTEGER NOT NULL CHECK (kept IN (0, 1))
) STRICT;

-- The oldest start, which a full outbox gives up, found without a scan.
CREATE INDEX sync_outbox_starts ON sync_outbox (seq) WHERE kept = 0;

-- The last place the hub took of each agent's epoch, and when.
CREATE TABLE sync_watermarks (
    agent_id TEXT NOT NULL,
    epoch TEXT NOT NULL,
    seq INTEGER NOT NULL,
    ingested_at INTEGER NOT NULL,
    PRIMARY KEY (agent_id, epoch)
) STRICT;

-- The reports the hub took, in the order it took them.
CREATE TABLE sync_reports (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    agent_id TEXT NOT NULL,
    epoch TEXT NOT NULL,
    seq INTEGER NOT NULL,
    op_id TEXT NOT NULL,
    happening TEXT NOT NULL,
    code TEXT,
    at INTEGER NOT NULL
) STRICT;

CREATE INDEX sync_reports_by_agent ON sync_reports (agent_id, id);

-- How many reports the hub keeps of each agent, so it lets go of the
-- oldest past its cap without counting them, and how many it let go of,
-- ever.
CREATE TABLE sync_report_tallies (
    agent_id TEXT PRIMARY KEY,
    held INTEGER NOT NULL CHECK (held >= 0),
    dropped INTEGER NOT NULL CHECK (dropped >= 0)
) STRICT;
