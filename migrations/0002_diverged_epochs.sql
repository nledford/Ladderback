-- A machine's outbox epoch whose report, at a place the hub took, wasn't
-- the one the hub took there: the outbox gave the place again, as one
-- restored from an older copy of its database does. The hub tells a
-- machine that states this epoch to start a new one; the mark goes with
-- the epoch's row when the hub forgets the epoch (decision 177, #1387).
ALTER TABLE sync_watermarks
  ADD COLUMN diverged INTEGER NOT NULL DEFAULT 0 CHECK (diverged IN (0, 1));

-- The report taken at a place of a machine's epoch, read to check a report
-- sent at a place the hub took against it (decision 177).
CREATE INDEX sync_reports_at ON sync_reports (machine_id, epoch, seq);
