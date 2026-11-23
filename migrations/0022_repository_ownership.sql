-- The hub's registry of maintenance owners (technical design §7.3, I1). One
-- row for each repository a claim was ever granted for: the last epoch
-- given, so epochs only rise, and the lease given at it until its holder
-- releases it. A lease's holder, epoch and expiry are all set or all null.
CREATE TABLE repository_owners (
    repo_id TEXT PRIMARY KEY,
    last_epoch INTEGER NOT NULL CHECK (last_epoch >= 1),
    holder TEXT,
    epoch INTEGER CHECK (epoch >= 1),
    expires_at INTEGER,
    CHECK ((holder IS NULL) = (epoch IS NULL)),
    CHECK ((epoch IS NULL) = (expires_at IS NULL))
) STRICT;

-- Each approved agent's last report of the repositories it uses, and when
-- the hub took it, in milliseconds. An agent's new report replaces every
-- row of its last.
CREATE TABLE ownership_reports (
    agent_id TEXT NOT NULL,
    repo_id TEXT NOT NULL,
    reported_at INTEGER NOT NULL,
    PRIMARY KEY (agent_id, repo_id)
) STRICT;

-- The agents that reported a repository, found without a scan.
CREATE INDEX ownership_reports_by_repo ON ownership_reports (repo_id);
