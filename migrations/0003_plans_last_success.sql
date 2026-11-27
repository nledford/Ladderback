-- When each plan a machine reported last backed up successfully, in seconds
-- since the Unix epoch, which the hub's dashboard tells while a later run of
-- the plan waits or runs (#1693). NULL when the machine reported none: a
-- plan that never succeeded, or a machine that predates the field.
ALTER TABLE sync_plans
  ADD COLUMN last_success_at INTEGER CHECK (last_success_at >= 0);
