-- Retries of a failed backup (technical design §5, §9; decision 76). One
-- addition, no data changes and no table rebuilt:
--
-- Each plan's pending retry, planned and not started yet, kept while the
-- agent is off, so a retry planned before a restart runs after it. One row
-- per plan. attempt counts from 1; retry_at is when the retry runs and until
-- the plan's next scheduled run when its series began (NULL for a plan with
-- no schedule): no retry of the series runs at or after it. Both are Unix
-- MILLISECONDS, UTC.
CREATE TABLE retry_state (
  plan_id  TEXT PRIMARY KEY NOT NULL,
  attempt  INTEGER NOT NULL,
  retry_at INTEGER NOT NULL,
  until    INTEGER
);
