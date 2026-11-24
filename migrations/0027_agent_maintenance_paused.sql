-- A paired machine's paused maintenance (technical design §7.3, decision
-- 158): one row for each repository it asks to maintain and holds no live
-- lease on, since when on the machine's own clock, in milliseconds, and
-- whether its notification rules were told (`repo.maintenance_paused`).
-- Each watch replaces every row; a grant leaves them as they are.
CREATE TABLE agent_maintenance_paused (
    repo_id TEXT PRIMARY KEY,
    since INTEGER NOT NULL,
    alerted INTEGER NOT NULL CHECK (alerted IN (0, 1))
) STRICT;
