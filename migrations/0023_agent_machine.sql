-- The machine fingerprint the hub took under each agent ID, and when it last
-- refused a second computer under it (technical design §7.2; §16, decision
-- 150; #296). The fingerprint is the SHA-256 digest of a label, the agent ID
-- and the identifier the operating system gives the computer, as 64
-- lower-case hex digits, never the identifier itself; NULL until a Hello
-- carries one, and again once a person forgets it. The refusal's time is
-- Unix milliseconds, NULL when none was refused. Two additions, no data
-- changes: the agents already registered take the fingerprint their next
-- Hello carries.
ALTER TABLE agents ADD COLUMN machine TEXT
    CHECK (machine IS NULL OR (length(machine) = 64 AND machine NOT GLOB '*[^0-9a-f]*'));
ALTER TABLE agents ADD COLUMN second_machine_at INTEGER;
