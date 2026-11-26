-- Recovery mode after a restore of the hub's database (technical design
-- §7.3; §16, decision 137).
--
-- On a machine: the proof of each epoch floor, the expiry and the hub
-- key's signature of the lease entry that set it, which the machine shows
-- the hub with its report. The expiry is on the hub's clock in Unix
-- milliseconds, UTC. A floor kept before this migration has none, and is
-- reported with none until the hub grants the repository again.
ALTER TABLE agent_epoch_floors ADD COLUMN expires_at INTEGER;
ALTER TABLE agent_epoch_floors ADD COLUMN signature BLOB
    CHECK (signature IS NULL OR length(signature) = 64);

-- On the hub: the floors machines proved above its counters, at most one
-- per repository, the highest. While any row is here, the hub is in
-- recovery mode: it grants and renews no lease until a person confirms the
-- old hub is stopped or gone, which raises each counter to its floor and
-- empties the table. `expires_at` is the proved lease's expiry in Unix
-- milliseconds, UTC; `held` whether its holder still held it.
CREATE TABLE hub_recovery_floors (
    repo_id TEXT PRIMARY KEY,
    holder TEXT NOT NULL,
    epoch INTEGER NOT NULL CHECK (epoch >= 1),
    expires_at INTEGER NOT NULL,
    held INTEGER NOT NULL CHECK (held IN (0, 1))
) STRICT;
