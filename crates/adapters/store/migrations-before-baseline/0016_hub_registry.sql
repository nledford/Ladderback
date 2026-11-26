-- The hub's registry (technical design §7.1, §9; decision 120, "Pairing" and
-- "Pending approval"): the pairing tokens the hub has made and the agents it
-- has registered. Two additions, no data changes and no table rebuilt. They
-- ship in every build, though only a build with the hub writes to them
-- (decision 122).
--
-- A pairing token is kept by its pairing key, the 32-byte public half, never
-- by its seed, so a stolen database holds no usable token. hub_key is the hub
-- key the token carries, kind the binding kind byte of its address (0 none,
-- 1 tls-exporter). state is unused, used or revoked; a used token names the
-- agent ID and agent key it was spent on, which no other state sets.
-- made_at and expires_at are Unix MILLISECONDS, UTC.
CREATE TABLE pairing_tokens (
  pairing_key    BLOB PRIMARY KEY NOT NULL CHECK (length(pairing_key) = 32),
  hub_key        BLOB NOT NULL CHECK (length(hub_key) = 32),
  kind           INTEGER NOT NULL,
  made_at        INTEGER NOT NULL,
  expires_at     INTEGER NOT NULL,
  state          TEXT NOT NULL CHECK (state IN ('unused', 'used', 'revoked')),
  used_agent_id  TEXT,
  used_agent_key BLOB,
  CHECK ((state = 'used') = (used_agent_id IS NOT NULL)),
  CHECK ((state = 'used') = (used_agent_key IS NOT NULL))
);

-- The agents, pending or approved. id is the agent ID, a lower-case
-- hyphenated UUID; pubkey the agent key, 32 bytes. hostname is the name it
-- paired under, and hostname_key what the hub compares names by (the lower
-- case of its NFC form), unique, so two agents can't hold one name. status
-- is pending or approved, since status_at. counter is the highest connection
-- counter the hub has taken from the agent, an unsigned 64-bit number as 20
-- decimal digits, zero-padded: it holds 18446744073709551615, which no
-- SQLite INTEGER does, and compares in order as text. status_at and
-- last_seen are Unix MILLISECONDS, UTC.
CREATE TABLE agents (
  id           TEXT PRIMARY KEY NOT NULL,
  pubkey       BLOB NOT NULL CHECK (length(pubkey) = 32),
  hostname     TEXT NOT NULL,
  hostname_key TEXT NOT NULL UNIQUE,
  status       TEXT NOT NULL CHECK (status IN ('pending', 'approved')),
  status_at    INTEGER NOT NULL,
  counter      TEXT NOT NULL CHECK (length(counter) = 20 AND counter NOT GLOB '*[^0-9]*'),
  last_seen    INTEGER NOT NULL
);
