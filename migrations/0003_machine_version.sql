-- The version of Ladderback a machine runs, as its last Hello wrote it,
-- shown on its card with how it stands against the hub's own; NULL until
-- the machine first connects (#1707).
ALTER TABLE machines ADD COLUMN version TEXT;
