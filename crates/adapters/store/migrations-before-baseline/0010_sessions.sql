-- The admin's sessions (technical design §10.1, §9; decision 70), so they
-- outlive a restart, and the session length the admin set. Two additions, no
-- data changes and no table rebuilt:
--
-- Never the cookie's token, and no CSRF token: digest is the SHA-256 of the
-- token's 32 random bytes. password is the fingerprint of the admin password
-- the session started under, as a signed 64-bit integer, so a new password
-- ends it. started_at and last_used_at are Unix MILLISECONDS, UTC.
-- keep_signed_in is 1 for a session signed in with Keep me signed in on this
-- device, which has no idle limit, else 0. secure is 1 for a session whose
-- cookie is marked Secure, as its sign-in decided and every renewal keeps,
-- else 0.
CREATE TABLE sessions (
  digest         BLOB PRIMARY KEY NOT NULL,
  password       INTEGER NOT NULL,
  started_at     INTEGER NOT NULL,
  last_used_at   INTEGER NOT NULL,
  keep_signed_in INTEGER NOT NULL DEFAULT 0 CHECK (keep_signed_in IN (0, 1)),
  secure         INTEGER NOT NULL DEFAULT 0 CHECK (secure IN (0, 1))
);

-- The sign-in settings: one row at most (id 1), none until the admin first
-- sets Stay signed in for, which is 30 days until then. session_length is
-- the name the wire uses too.
CREATE TABLE sign_in_settings (
  id             INTEGER PRIMARY KEY NOT NULL CHECK (id = 1),
  session_length TEXT NOT NULL CHECK (session_length IN (
    'until-browser-closes', '7-days', '30-days', '90-days', '1-year', 'until-sign-out'
  ))
);
