-- The proposals (technical design §9; decision 89): configuration changes
-- an API client submitted for a person to confirm, pending and ended. One
-- addition, no data changes and no table rebuilt:
--
-- id is 12 characters of lower-case base32. token_id and token_label name
-- the API token that proposed it, its ID and its label as they were then,
-- never its secret (I4); both are NULL for a client with no token. base is
-- the SHA-256, in hex, of the configuration file the proposal was drafted
-- against. changes is the JSON the configuration adapter wrote of them;
-- rationale the proposer's own words. created_at and expires_at are Unix
-- MILLISECONDS, UTC.
CREATE TABLE proposals (
  id          TEXT PRIMARY KEY NOT NULL,
  token_id    TEXT,
  token_label TEXT,
  base        TEXT NOT NULL,
  changes     TEXT NOT NULL,
  rationale   TEXT NOT NULL,
  status      TEXT NOT NULL CHECK (status IN (
    'pending', 'applying', 'applied', 'failed', 'rejected', 'withdrawn', 'expired', 'stale'
  )),
  created_at  INTEGER NOT NULL,
  expires_at  INTEGER NOT NULL,
  CHECK ((token_id IS NULL) = (token_label IS NULL))
);

CREATE INDEX proposals_by_status ON proposals (status, created_at);
