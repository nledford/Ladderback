-- The readings of each repository's stats (technical design §9; decision
-- 104), kept a year so they can be charted. One addition, no data changes and
-- no table rebuilt:
--
-- id orders the readings as they were kept. repo_alias is the repository's
-- alias, repo_uuid the ID its configuration entry recorded then, NULL when it
-- recorded none. taken_at is Unix MILLISECONDS, UTC. The figures are counts
-- and bytes, never negative: a figure past what a signed 64-bit integer
-- holds is refused, never wrapped.
CREATE TABLE repo_stats (
  id           INTEGER PRIMARY KEY NOT NULL,
  repo_alias   TEXT NOT NULL,
  repo_uuid    TEXT,
  taken_at     INTEGER NOT NULL,
  snapshots    INTEGER NOT NULL CHECK (snapshots >= 0),
  total_bytes  INTEGER NOT NULL CHECK (total_bytes >= 0),
  stored_bytes INTEGER NOT NULL CHECK (stored_bytes >= 0),
  raw_bytes    INTEGER NOT NULL CHECK (raw_bytes >= 0)
);

-- The latest reading of a repository, and the readings a newer one drops.
CREATE INDEX repo_stats_by_alias ON repo_stats (repo_alias, taken_at);
