-- The release check (technical design §5, §9; decision 107). One addition, no
-- data changes and no table rebuilt:
--
-- The agent's last release check: one row at most (id 1), none until it
-- first asks for Ladderback's newest release. checked_at is when it asked,
-- in Unix MILLISECONDS, UTC; update_version the available update it knew of
-- then, as SemVer writes it (0.3.0), or NULL when there was none. Kept so a
-- restart neither asks again within the day nor hides the update.
CREATE TABLE release_check (
  id             INTEGER PRIMARY KEY NOT NULL CHECK (id = 1),
  checked_at     INTEGER NOT NULL,
  update_version TEXT
);
