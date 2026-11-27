-- A paired machine's ledger of its hub's commands (§16, decisions 120 and
-- 176), and the hub session that asked for an operation.
--
-- hub_commands: the ID of each command the machine took, for 24 hours, so a
-- replay after a restart is refused; taken_at is the machine's clock, Unix
-- MILLISECONDS. At most 1000 rows: the store refuses more, as the port says.
CREATE TABLE hub_commands (
  command_id TEXT PRIMARY KEY NOT NULL,
  taken_at   INTEGER NOT NULL CHECK (taken_at >= 0)
) STRICT;

-- Forgetting the old IDs and finding the oldest read by time.
CREATE INDEX hub_commands_taken_at ON hub_commands (taken_at);

-- hub_runs: each backup the hub started, for an hour, counted against its
-- rate limit (one per plan per 5 minutes, 12 an hour); at in MILLISECONDS.
CREATE TABLE hub_runs (
  plan_id TEXT NOT NULL,
  at      INTEGER NOT NULL CHECK (at >= 0)
) STRICT;

-- The rate check reads the runs in time order and forgets those an hour old.
CREATE INDEX hub_runs_at ON hub_runs (at);

-- hub_session: the tag of the hub session that asked for an operation, 16
-- lower-case hex digits and nothing secret (I4); set only on the hub's.
ALTER TABLE operations ADD COLUMN hub_session TEXT
  CHECK (hub_session IS NULL OR ("trigger" = 'hub' AND length(hub_session) = 16));
