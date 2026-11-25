-- The baseline schema (technical design §9; §16, decision 165): every table
-- the store keeps, as migrations 0001-0029 had made them, under the machine
-- names (decision 163), with the constraints the review of #1618 added. It
-- replaced those migrations before the MVP; a database they left is refused
-- (`OpenError::OldSchema`) and copied once into this schema by
-- `scripts/copy-to-baseline.ts`. From the first release on, migrations are
-- append-only again.
--
-- Every table is STRICT, so a column keeps only its declared type. Times are
-- Unix MILLISECONDS, UTC, and never before 1970, but where a column says
-- SECONDS: those come from a machine's reports as it sent them. Every
-- table ships in every build, though only a build with the hub writes the
-- hub's (decision 122).

-- The operation log (§9). id is a UUIDv7, lower-case and hyphenated, so its
-- text order is time order. kind, trigger and status are the codec's names
-- (`crates/adapters/store/src/codec.rs`). A failed operation, and only a
-- failed one, keeps its failure: error_code is the api::ErrorCode
-- (kebab-case), error_args a JSON array of [name, MsgArg] pairs, and
-- error_detail the engine's or platform's own text, verbatim, when it gave
-- one. summary_json is the engine's result, locations_json the record of the
-- locations it tried, both JSON. repo_alias is the repository's name in the
-- configuration when it ran, location_no the location it used. token_id and
-- token_label name the API token of an api-token trigger, both or neither.
-- entry is the snapshot entry a restore or download took; restored_to its
-- target, '' for in place. warning_count counts every warning recorded,
-- kept in op_warnings or not.
CREATE TABLE operations (
  id             TEXT PRIMARY KEY NOT NULL,
  kind           TEXT NOT NULL CHECK (kind IN (
    'fake', 'identify', 'init', 'backup', 'forget', 'adopt', 'prune', 'check', 'ls', 'restore',
    'download'
  )),
  plan_id        TEXT,
  repo_uuid      TEXT,
  "trigger"      TEXT NOT NULL CHECK ("trigger" IN (
    'schedule', 'manual', 'catch-up', 'retry', 'hub', 'mcp', 'api-token'
  )),
  status         TEXT NOT NULL CHECK (status IN (
    'queued', 'running', 'succeeded', 'succeeded_with_warnings', 'failed', 'cancelled'
  )),
  queued_at      INTEGER NOT NULL CHECK (queued_at >= 0),
  started_at     INTEGER CHECK (started_at >= 0),
  finished_at    INTEGER CHECK (finished_at >= 0),
  snapshot_id    TEXT,
  summary_json   TEXT CHECK (json_valid(summary_json)),
  error_code     TEXT,
  error_args     TEXT CHECK (json_valid(error_args)),
  error_detail   TEXT,
  warning_count  INTEGER NOT NULL DEFAULT 0 CHECK (warning_count >= 0),
  repo_alias     TEXT,
  location_no    INTEGER,
  locations_json TEXT CHECK (json_valid(locations_json)),
  token_id       TEXT,
  token_label    TEXT,
  entry          TEXT,
  restored_to    TEXT,
  CHECK ((status = 'failed') = (error_code IS NOT NULL)),
  CHECK ((status = 'failed') = (error_args IS NOT NULL)),
  CHECK ((token_id IS NULL) = (token_label IS NULL))
) STRICT;

-- A plan's history, newest first, read from the index.
CREATE INDEX ops_by_plan ON operations (plan_id, id DESC);

-- At most one maintenance operation runs on a repository at a time (I7): the
-- backstop under the check the store makes when it starts one.
CREATE UNIQUE INDEX one_maintenance_per_repo ON operations (repo_uuid)
  WHERE status = 'running' AND kind IN ('forget', 'prune', 'check', 'adopt');

-- Whether a plan ever succeeded, in one probe however many runs failed.
CREATE INDEX ops_succeeded_by_plan ON operations (plan_id)
  WHERE status IN ('succeeded', 'succeeded_with_warnings');

-- The persistent queue (§5), without a scan of the history.
CREATE INDEX ops_queue ON operations (id) WHERE status = 'queued';

-- The warnings an operation kept, in the order recorded (rowid). item NULL
-- and item '' are different things. code is the api::WarningCode.
CREATE TABLE op_warnings (
  op_id  TEXT NOT NULL REFERENCES operations (id) ON DELETE CASCADE,
  item   TEXT,
  during TEXT,
  code   TEXT,
  detail TEXT
) STRICT;

CREATE INDEX op_warnings_by_op ON op_warnings (op_id);

-- Each plan's planned run (§5).
CREATE TABLE schedule_state (
  plan_id  TEXT PRIMARY KEY NOT NULL,
  next_run INTEGER NOT NULL CHECK (next_run >= 0)
) STRICT;

-- Each plan's pending retry: its attempt, from 1, when it runs, and until
-- when retries may run.
CREATE TABLE retry_state (
  plan_id  TEXT PRIMARY KEY NOT NULL,
  attempt  INTEGER NOT NULL CHECK (attempt >= 1),
  retry_at INTEGER NOT NULL CHECK (retry_at >= 0),
  until    INTEGER CHECK (until >= 0)
) STRICT;

-- The API tokens (§10.1; decision 93). Never the secret: digest is the
-- SHA-256 of its 32 random bytes. permissions is what the token may do, as
-- `api_tokens.rs` writes it. password is the fingerprint of the admin
-- password it was created under, a signed 64-bit integer, so a new password
-- revokes it. last_used_at is NULL until the token is first used.
CREATE TABLE api_tokens (
  id           TEXT PRIMARY KEY NOT NULL,
  label        TEXT NOT NULL,
  digest       BLOB NOT NULL,
  password     INTEGER NOT NULL,
  created_at   INTEGER NOT NULL CHECK (created_at >= 0),
  last_used_at INTEGER CHECK (last_used_at >= 0),
  permissions  TEXT NOT NULL DEFAULT ''
) STRICT;

-- The admin's sessions (§10.1; decision 70). digest is the SHA-256 of the
-- cookie's 32 random bytes; password the admin password's fingerprint, as
-- for a token. keep_signed_in is 1 for Keep me signed in on this device;
-- secure is 1 for a cookie marked Secure.
CREATE TABLE sessions (
  digest         BLOB PRIMARY KEY NOT NULL,
  password       INTEGER NOT NULL,
  started_at     INTEGER NOT NULL CHECK (started_at >= 0),
  last_used_at   INTEGER NOT NULL CHECK (last_used_at >= 0),
  keep_signed_in INTEGER NOT NULL DEFAULT 0 CHECK (keep_signed_in IN (0, 1)),
  secure         INTEGER NOT NULL DEFAULT 0 CHECK (secure IN (0, 1))
) STRICT;

-- The session length the admin set, one row at most, named as on the wire.
CREATE TABLE sign_in_settings (
  id             INTEGER PRIMARY KEY NOT NULL CHECK (id = 1),
  session_length TEXT NOT NULL CHECK (session_length IN (
    'until-browser-closes', '7-days', '30-days', '90-days', '1-year', 'until-sign-out'
  ))
) STRICT;

-- The readings of each repository's stats, by alias, the newest kept.
CREATE TABLE repo_stats (
  id           INTEGER PRIMARY KEY NOT NULL,
  repo_alias   TEXT NOT NULL,
  repo_uuid    TEXT,
  taken_at     INTEGER NOT NULL CHECK (taken_at >= 0),
  snapshots    INTEGER NOT NULL CHECK (snapshots >= 0),
  total_bytes  INTEGER NOT NULL CHECK (total_bytes >= 0),
  stored_bytes INTEGER NOT NULL CHECK (stored_bytes >= 0),
  raw_bytes    INTEGER NOT NULL CHECK (raw_bytes >= 0)
) STRICT;

CREATE INDEX repo_stats_by_alias ON repo_stats (repo_alias, taken_at);

-- The last check for a new release, one row at most.
CREATE TABLE release_check (
  id             INTEGER PRIMARY KEY NOT NULL CHECK (id = 1),
  checked_at     INTEGER NOT NULL CHECK (checked_at >= 0),
  update_version TEXT
) STRICT;

-- The configuration changes a program proposed (decision 108). changes is
-- the text the change codec made, which the store doesn't read; reasons a
-- JSON array of a person's reasons, once they rejected it. token_id and
-- token_label name the proposer's API token, both or neither.
CREATE TABLE proposals (
  id            TEXT PRIMARY KEY NOT NULL,
  token_id      TEXT,
  token_label   TEXT,
  base          TEXT NOT NULL,
  changes       TEXT NOT NULL,
  rationale     TEXT NOT NULL,
  status        TEXT NOT NULL CHECK (status IN (
    'pending', 'applying', 'applied', 'failed', 'rejected', 'withdrawn', 'expired', 'stale'
  )),
  created_at    INTEGER NOT NULL CHECK (created_at >= 0),
  expires_at    INTEGER NOT NULL CHECK (expires_at >= 0),
  decided_at    INTEGER CHECK (decided_at >= 0),
  reasons       TEXT CHECK (json_valid(reasons)),
  approved_from TEXT,
  applied_as    TEXT,
  CHECK ((token_id IS NULL) = (token_label IS NULL))
) STRICT;

CREATE INDEX proposals_by_status ON proposals (status, created_at);

-- The hub's pairing tokens (§7.1; decision 120), by pairing key, the 32-byte
-- public half, never by seed. kind is the binding kind byte of its address
-- (0 none, 1 tls-exporter). A used token names the machine ID and key it was
-- spent on, which no other state sets; it keeps them after the machine is
-- removed.
CREATE TABLE pairing_tokens (
  pairing_key      BLOB PRIMARY KEY NOT NULL CHECK (length(pairing_key) = 32),
  hub_key          BLOB NOT NULL CHECK (length(hub_key) = 32),
  kind             INTEGER NOT NULL,
  made_at          INTEGER NOT NULL CHECK (made_at >= 0),
  expires_at       INTEGER NOT NULL CHECK (expires_at >= 0),
  state            TEXT NOT NULL CHECK (state IN ('unused', 'used', 'revoked')),
  used_machine_id  TEXT,
  used_machine_key BLOB,
  CHECK ((state = 'used') = (used_machine_id IS NOT NULL)),
  CHECK ((state = 'used') = (used_machine_key IS NOT NULL))
) STRICT;

-- The hub's machines, pending or approved (§7.1; decision 120). id is the
-- machine ID, a lower-case hyphenated UUID; pubkey its machine key.
-- hostname_key is what the hub compares names by (the lower case of the
-- name's NFC form). counter is the highest connection counter taken, an
-- unsigned 64-bit number as 20 zero-padded digits, which compares in order
-- as text. zone is the time zone it reported; machine the fingerprint of the
-- computer first heard under the ID (decision 150), and second_machine_at
-- when another computer was last refused under it.
CREATE TABLE machines (
  id                TEXT PRIMARY KEY NOT NULL,
  pubkey            BLOB NOT NULL CHECK (length(pubkey) = 32),
  hostname          TEXT NOT NULL,
  hostname_key      TEXT NOT NULL UNIQUE,
  status            TEXT NOT NULL CHECK (status IN ('pending', 'approved')),
  status_at         INTEGER NOT NULL CHECK (status_at >= 0),
  counter           TEXT NOT NULL CHECK (length(counter) = 20 AND counter NOT GLOB '*[^0-9]*'),
  last_seen         INTEGER NOT NULL CHECK (last_seen >= 0),
  zone              TEXT,
  machine           TEXT
    CHECK (machine IS NULL OR (length(machine) = 64 AND machine NOT GLOB '*[^0-9a-f]*')),
  second_machine_at INTEGER CHECK (second_machine_at >= 0)
) STRICT;

-- The machines the hub removed (decision 154), so their IDs and keys never
-- pair again.
CREATE TABLE machine_tombstones (
  id         TEXT PRIMARY KEY NOT NULL,
  pubkey     BLOB NOT NULL CHECK (length(pubkey) = 32),
  hostname   TEXT NOT NULL,
  removed_at INTEGER NOT NULL CHECK (removed_at >= 0)
) STRICT;

-- The fingerprint of the hub's TLS key, one row at most.
CREATE TABLE hub_tls_key (
  id          INTEGER PRIMARY KEY NOT NULL CHECK (id = 1),
  fingerprint BLOB NOT NULL CHECK (length(fingerprint) = 32)
) STRICT;

-- A machine's outbox (§7.2): its one epoch and the last place it gave, so a
-- place is never given twice, even after its report is gone.
CREATE TABLE sync_outbox_meta (
  id       INTEGER PRIMARY KEY NOT NULL CHECK (id = 1),
  epoch    TEXT NOT NULL,
  last_seq INTEGER NOT NULL CHECK (last_seq >= 0)
) STRICT;

-- The reports the hub hasn't acknowledged, by their place; at in SECONDS.
-- kept is 1 for an end, which a full outbox never gives up.
CREATE TABLE sync_outbox (
  seq       INTEGER PRIMARY KEY NOT NULL CHECK (seq >= 1),
  op_id     TEXT NOT NULL,
  happening TEXT NOT NULL CHECK (happening IN ('started', 'succeeded', 'failed', 'cancelled')),
  code      TEXT,
  at        INTEGER NOT NULL CHECK (at >= 0),
  kept      INTEGER NOT NULL CHECK (kept IN (0, 1))
) STRICT;

-- The oldest start, which a full outbox gives up, found without a scan.
CREATE INDEX sync_outbox_starts ON sync_outbox (seq) WHERE kept = 0;

-- The hub's side of each machine's reports (§7.2). Its operation reports,
-- their tally and its watermarks are its history, which the hub keeps after
-- the machine's removal (decision 154): no foreign key ties them to
-- machines.
--
-- The last place the hub took of each machine's epoch, and when.
CREATE TABLE sync_watermarks (
  machine_id  TEXT NOT NULL,
  epoch       TEXT NOT NULL,
  seq         INTEGER NOT NULL CHECK (seq >= 1),
  ingested_at INTEGER NOT NULL CHECK (ingested_at >= 0),
  PRIMARY KEY (machine_id, epoch)
) STRICT;

-- The reports the hub took, in the order it took them (AUTOINCREMENT, so an
-- ID is never given again); at in SECONDS.
CREATE TABLE sync_reports (
  id         INTEGER PRIMARY KEY AUTOINCREMENT,
  machine_id TEXT NOT NULL,
  epoch      TEXT NOT NULL,
  seq        INTEGER NOT NULL CHECK (seq >= 1),
  op_id      TEXT NOT NULL,
  happening  TEXT NOT NULL CHECK (happening IN ('started', 'succeeded', 'failed', 'cancelled')),
  code       TEXT,
  at         INTEGER NOT NULL CHECK (at >= 0)
) STRICT;

CREATE INDEX sync_reports_by_machine ON sync_reports (machine_id, id);

-- The hub holds an operation's report once, whichever machine ID sent it:
-- one a machine sends again under a new ID after its removal is a
-- duplicate (#1513).
CREATE UNIQUE INDEX sync_reports_once ON sync_reports (op_id, happening);

-- How many reports the hub holds of each machine, so it lets go of the
-- oldest past its cap without counting them, and how many it let go of.
CREATE TABLE sync_report_tallies (
  machine_id TEXT PRIMARY KEY NOT NULL,
  held       INTEGER NOT NULL CHECK (held >= 0),
  dropped    INTEGER NOT NULL CHECK (dropped >= 0)
) STRICT;

-- Each machine's current state as the hub last heard it (§7.2): its plans
-- in their order and how many it left out, and its latest progress, in
-- SECONDS. They go with the machine when the hub removes it (decision 165).
CREATE TABLE sync_plans (
  machine_id   TEXT NOT NULL REFERENCES machines (id) ON DELETE CASCADE,
  place        INTEGER NOT NULL CHECK (place >= 0),
  plan_id      TEXT NOT NULL,
  last_at      INTEGER CHECK (last_at >= 0),
  last_outcome TEXT,
  next_run     INTEGER CHECK (next_run >= 0),
  PRIMARY KEY (machine_id, place),
  CHECK ((last_at IS NULL) = (last_outcome IS NULL))
) STRICT;

CREATE TABLE sync_plan_snapshots (
  machine_id TEXT PRIMARY KEY NOT NULL REFERENCES machines (id) ON DELETE CASCADE,
  left_out   INTEGER NOT NULL CHECK (left_out >= 0)
) STRICT;

CREATE TABLE sync_progress (
  machine_id TEXT PRIMARY KEY NOT NULL REFERENCES machines (id) ON DELETE CASCADE,
  op_id      TEXT NOT NULL,
  step       TEXT,
  unit       TEXT NOT NULL,
  current    INTEGER NOT NULL CHECK (current >= 0),
  total      INTEGER CHECK (total >= 0),
  at         INTEGER NOT NULL CHECK (at >= 0)
) STRICT;

-- The hub's registry of maintenance owners (§7.3, I1): for each repository a
-- claim was ever granted for, the last epoch given, so epochs only rise, and
-- the lease at it until its holder releases it. Holder, epoch and expiry are
-- all set or all null. The holder is no foreign key: a lease outlives its
-- holder's removal until it expires (I1).
CREATE TABLE repository_owners (
  repo_id    TEXT PRIMARY KEY NOT NULL,
  last_epoch INTEGER NOT NULL CHECK (last_epoch >= 1),
  holder     TEXT,
  epoch      INTEGER CHECK (epoch >= 1),
  expires_at INTEGER CHECK (expires_at >= 0),
  CHECK ((holder IS NULL) = (epoch IS NULL)),
  CHECK ((epoch IS NULL) = (expires_at IS NULL))
) STRICT;

-- Each machine's last report of the repositories it uses, and when the hub
-- took it. A new report replaces every row of the last; the rows go with
-- the machine when the hub removes it (decision 165).
CREATE TABLE ownership_reports (
  machine_id  TEXT NOT NULL REFERENCES machines (id) ON DELETE CASCADE,
  repo_id     TEXT NOT NULL,
  reported_at INTEGER NOT NULL CHECK (reported_at >= 0),
  PRIMARY KEY (machine_id, repo_id)
) STRICT;

CREATE INDEX ownership_reports_by_repo ON ownership_reports (repo_id);

-- Recovery mode after a restore of the hub's database (§7.3; decision 137):
-- the floors machines proved above the hub's counters, the highest per
-- repository. held is whether the proved lease's holder still held it. The
-- holder is no foreign key, as for repository_owners.
CREATE TABLE hub_recovery_floors (
  repo_id    TEXT PRIMARY KEY NOT NULL,
  holder     TEXT NOT NULL,
  epoch      INTEGER NOT NULL CHECK (epoch >= 1),
  expires_at INTEGER NOT NULL CHECK (expires_at >= 0),
  held       INTEGER NOT NULL CHECK (held IN (0, 1))
) STRICT;

-- A paired machine's leases from its hub (§7.3, I1), until when on its own
-- clock. Each set of grants replaces every row.
CREATE TABLE machine_leases (
  repo_id TEXT PRIMARY KEY NOT NULL,
  epoch   INTEGER NOT NULL CHECK (epoch >= 1),
  ends_at INTEGER NOT NULL CHECK (ends_at >= 0)
) STRICT;

-- The highest lease the machine has seen for each repository, its epoch
-- floor, which only rises and outlives its lease; with the proof of it, the
-- lease's expiry on the hub's clock and the hub key's signature of the lease
-- entry, none for a floor the hub hasn't granted again since it was kept.
CREATE TABLE machine_epoch_floors (
  repo_id         TEXT PRIMARY KEY NOT NULL,
  recovery_number INTEGER NOT NULL CHECK (recovery_number >= 0),
  generation      INTEGER NOT NULL CHECK (generation >= 0),
  epoch           INTEGER NOT NULL CHECK (epoch >= 1),
  expires_at      INTEGER CHECK (expires_at >= 0),
  signature       BLOB CHECK (signature IS NULL OR length(signature) = 64)
) STRICT;

-- The repositories whose maintenance a machine paused, since when, and
-- whether it said so (alerted).
CREATE TABLE machine_maintenance_paused (
  repo_id TEXT PRIMARY KEY NOT NULL,
  since   INTEGER NOT NULL CHECK (since >= 0),
  alerted INTEGER NOT NULL CHECK (alerted IN (0, 1))
) STRICT;
