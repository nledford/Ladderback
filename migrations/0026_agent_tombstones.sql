-- What the hub keeps of each agent a person removed or rejected (technical
-- design §7.1; §16, decisions 120 and 154; #1439): its agent ID, the
-- hostname it held and its last key, and when it was removed, in Unix
-- milliseconds, UTC. The hostname is a record only, with no unique index:
-- another machine may take the name. A tombstone is kept for good, so the
-- removed agent's next connection is answered, signed, that it was removed,
-- and an ordinary token under its ID is refused. One new table, no data
-- changes.
CREATE TABLE agent_tombstones (
    id         TEXT PRIMARY KEY NOT NULL,
    pubkey     BLOB NOT NULL CHECK (length(pubkey) = 32),
    hostname   TEXT NOT NULL,
    removed_at INTEGER NOT NULL
) STRICT;
