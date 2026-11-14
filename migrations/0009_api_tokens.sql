-- API tokens (technical design §10.1, §9; decision 93). Two additions, no
-- data changes and no table rebuilt:
--
-- The tokens a program uses the API with while sign-in is on. Never the
-- secret: digest is the SHA-256 of its 32 random bytes. scopes is a comma
-- list of read|run|preview|propose; password is the fingerprint of the admin
-- password the token was created under, as a signed 64-bit integer, so a new
-- password revokes it. created_at and last_used_at are Unix MILLISECONDS,
-- UTC; last_used_at is NULL until the token is first used.
CREATE TABLE api_tokens (
  id           TEXT PRIMARY KEY NOT NULL,
  label        TEXT NOT NULL,
  digest       BLOB NOT NULL,
  scopes       TEXT NOT NULL,
  password     INTEGER NOT NULL,
  created_at   INTEGER NOT NULL,
  last_used_at INTEGER
);

-- The token an operation's api-token trigger names, its ID and its label as
-- they were when it started: both set for that trigger, both NULL for every
-- other one and for every row logged before this build.
ALTER TABLE operations ADD COLUMN token_id TEXT;
ALTER TABLE operations ADD COLUMN token_label TEXT;
