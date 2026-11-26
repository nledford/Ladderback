-- The scheduler (technical design §5, §9; decision 73). Two additions, no
-- data changes and no table rebuilt:
--
-- Each scheduled plan's planned run, kept while the agent is off or asleep,
-- so a run that passed meanwhile is made up for when it comes back. One row
-- per plan; next_run is Unix MILLISECONDS, UTC. §9 names a last_success
-- column as well: it arrives with the minimum-interval condition that reads
-- it.
CREATE TABLE schedule_state (
  plan_id  TEXT PRIMARY KEY NOT NULL,
  next_run INTEGER NOT NULL
);

-- The persistent queue: the queued operations, oldest first, which startup
-- recovery reads. A partial index holds the queued rows alone, so the read
-- doesn't grow with the history; the query names the status as a literal,
-- so SQLite matches it to the index when it prepares it.
CREATE INDEX ops_queue ON operations(id) WHERE status = 'queued';
