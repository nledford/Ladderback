-- A paired machine's leases from its hub (technical design §7.3, I1): one
-- row for each repository it holds a lease on, at its epoch, until when on
-- the machine's own clock, in milliseconds. Each set of grants replaces
-- every row.
CREATE TABLE agent_leases (
    repo_id TEXT PRIMARY KEY,
    epoch INTEGER NOT NULL CHECK (epoch >= 1),
    ends_at INTEGER NOT NULL
) STRICT;

-- The highest lease the machine has seen for each repository, its epoch
-- floor: a grant below it is refused (`epoch-lower`). A floor only rises
-- and outlives the lease it came from.
CREATE TABLE agent_epoch_floors (
    repo_id TEXT PRIMARY KEY,
    recovery_number INTEGER NOT NULL CHECK (recovery_number >= 0),
    generation INTEGER NOT NULL CHECK (generation >= 0),
    epoch INTEGER NOT NULL CHECK (epoch >= 1)
) STRICT;
