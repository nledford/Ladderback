## 9. Data model (SQLite)

```sql
-- runtime state; config lives in config.toml
CREATE TABLE operations (           -- also the job queue (status = 'queued')
  id TEXT PRIMARY KEY,            -- UUIDv7 (time-ordered)
  agent_id TEXT NOT NULL,         -- 'local' on agents; registry id on hub
  kind TEXT NOT NULL,             -- backup|forget|prune|check|restore|download|stats|...
  plan_id TEXT, repo_uuid TEXT,   -- repo_uuid = rustic repository ID
  repo_alias TEXT,                -- maintenance ops, restores and downloads: the repository's alias when the run was first written (decision 52)
  location_no INTEGER,            -- the position, from 1, of the location the run used; NULL for a repository of one location (decision 80)
  locations_json TEXT,            -- every location of the entry as the run saw it: position, address as written with credentials masked (at most 256 bytes), outcome code (`not-tried` after the one used); local to the agent, never synced; NULL as above
  entry TEXT,                     -- restore/download: the snapshot entry it read, `/` + its path from the top, the repository's secrets masked, at most 1 KiB (#847); NULL otherwise
  trigger TEXT NOT NULL,          -- schedule|manual|catch-up|retry|hub|mcp|api-token
  token_id TEXT, token_label TEXT, -- the API token an api-token trigger names (decision 93)
  status TEXT NOT NULL,           -- queued|running|succeeded|succeeded_with_warnings|failed|cancelled
  queued_at INTEGER, started_at INTEGER, finished_at INTEGER,
  index_read_at INTEGER, deadline INTEGER,     -- backups (§7.3)
  lease_epoch INTEGER,            -- maintenance ops: owner lease they ran under (§7.3)
  attempt INTEGER DEFAULT 1,
  requested_by TEXT,              -- restores: local UI or hub (§10.4)
  snapshot_id TEXT,               -- for backups
  summary_json TEXT,              -- OpResult (files new/changed/unmodified, bytes processed and added, duration, …)
  error_code TEXT, error_args TEXT, error_detail TEXT,   -- api::ErrorCode, stored arguments (§9), rustic's verbatim text (§8.4)
  warning_count INTEGER DEFAULT 0
);
CREATE INDEX ops_by_plan ON operations(plan_id, started_at DESC);
CREATE INDEX ops_queue   ON operations(id) WHERE status = 'queued';   -- the persistent queue (§5), oldest first
CREATE UNIQUE INDEX one_maintenance_per_repo ON operations(repo_uuid)
  WHERE status = 'running' AND kind IN ('forget', 'prune', 'check');   -- I7; OperationStore::try_start

CREATE TABLE op_warnings (op_id TEXT NOT NULL, item TEXT, during TEXT, code TEXT, detail TEXT);
CREATE INDEX op_warnings_by_op ON op_warnings(op_id);   -- capped at 10,000 rows per op; warning_count keeps the total
CREATE TABLE op_logs     (op_id TEXT, seq INTEGER, ts INTEGER, level TEXT, line TEXT,
                          PRIMARY KEY (op_id, seq));      -- redacted; retained N days

CREATE TABLE schedule_state (plan_id TEXT PRIMARY KEY, next_run INTEGER, last_success INTEGER);   -- last_success: not built yet
CREATE TABLE retry_state (plan_id TEXT PRIMARY KEY NOT NULL, attempt INTEGER NOT NULL,
                          retry_at INTEGER NOT NULL, until INTEGER);   -- each plan's pending retry (§5)

CREATE TABLE snapshot_cache (     -- refreshed after backups/forget, and on demand
  repo_uuid TEXT, id TEXT, time INTEGER, hostname TEXT, label TEXT,
  ladderback_plan TEXT, ladderback_agent TEXT,                -- from tags; NULL if untagged
  paths_json TEXT, tags_json TEXT, summary_json TEXT,
  PRIMARY KEY (repo_uuid, id)
);

-- agents with a hub configured
CREATE TABLE sync_outbox (epoch TEXT NOT NULL, seq INTEGER NOT NULL, op_id TEXT, event_json TEXT,
                          created_at INTEGER, PRIMARY KEY (epoch, seq));
                          -- rows deleted once acknowledged; capped at 100,000 rows, dropping
                          -- log events first and never operation start/finish records
CREATE TABLE repo_grants (repo_uuid TEXT PRIMARY KEY, owner TEXT, epoch INTEGER, expires_at INTEGER,
                          keep_delete_secs INTEGER, prune_margin_secs INTEGER, updated_at INTEGER);

CREATE TABLE api_tokens (id TEXT PRIMARY KEY, label TEXT NOT NULL,
                         digest BLOB NOT NULL,      -- SHA-256 of the secret part (§10.1, decision 93)
                         scopes TEXT NOT NULL,      -- comma list of read|run|preview|propose (§10.1)
                         password INTEGER NOT NULL, -- fingerprint of the admin password it was created under
                         created_at INTEGER NOT NULL, last_used_at INTEGER);

CREATE TABLE sessions (digest BLOB PRIMARY KEY NOT NULL,  -- SHA-256 of the cookie's token (§10.1, decision 70)
                       password INTEGER NOT NULL,         -- fingerprint of the admin password it started under
                       started_at INTEGER NOT NULL, last_used_at INTEGER NOT NULL,
                       keep_signed_in INTEGER NOT NULL DEFAULT 0, secure INTEGER NOT NULL DEFAULT 0);
CREATE TABLE sign_in_settings (id INTEGER PRIMARY KEY NOT NULL CHECK (id = 1),
                               session_length TEXT NOT NULL);  -- Stay signed in for (§10.1)
CREATE TABLE repo_stats (id INTEGER PRIMARY KEY NOT NULL,      -- one reading of a repository's stats, kept a year (decision 104)
                         repo_alias TEXT NOT NULL, repo_uuid TEXT,  -- the ID its entry recorded then, if any
                         taken_at INTEGER NOT NULL,
                         snapshots INTEGER NOT NULL CHECK (snapshots >= 0),
                         total_bytes INTEGER NOT NULL CHECK (total_bytes >= 0),
                         stored_bytes INTEGER NOT NULL CHECK (stored_bytes >= 0),
                         raw_bytes INTEGER NOT NULL CHECK (raw_bytes >= 0));
CREATE INDEX repo_stats_by_alias ON repo_stats (repo_alias, taken_at);
CREATE TABLE release_check (id INTEGER PRIMARY KEY NOT NULL CHECK (id = 1),
                            checked_at INTEGER NOT NULL,  -- the last release check (§5)
                            update_version TEXT);         -- the available update it found, if any
CREATE TABLE proposals (id TEXT PRIMARY KEY NOT NULL,   -- 12 characters of lower-case base32
                        token_id TEXT,                 -- the proposing token's ID and label, both
                        token_label TEXT,              -- NULL for a client with no token (§10.1)
                        base TEXT NOT NULL,            -- SHA-256 hex of the config.toml drafted against
                        changes TEXT NOT NULL,         -- JSON of the changes, the config adapter's
                        rationale TEXT NOT NULL,
                        status TEXT NOT NULL,          -- pending|applying|applied|failed|rejected|withdrawn|expired|stale
                        created_at INTEGER NOT NULL,
                        expires_at INTEGER NOT NULL);
CREATE INDEX proposals_by_status ON proposals (status, created_at);

-- hub only
CREATE TABLE pairing_tokens (pairing_key BLOB PRIMARY KEY,  -- the one-time key's public half, never its seed
                             hub_key BLOB, kind INTEGER, made_at INTEGER, expires_at INTEGER,
                             state TEXT,                    -- unused|used|revoked
                             used_agent_id TEXT, used_agent_key BLOB);
CREATE TABLE hub_tls_key (id INTEGER PRIMARY KEY CHECK (id = 1),  -- one row, written once
                          fingerprint BLOB NOT NULL);            -- SHA-256 of the TLS certificate's public key
CREATE TABLE agents (id TEXT PRIMARY KEY,          -- generated by the agent (§7.1)
                     pubkey BLOB NOT NULL, hostname TEXT NOT NULL,
                     hostname_key TEXT NOT NULL UNIQUE,      -- casefold(NFC(hostname)), decision 120
                     status TEXT, status_at INTEGER,         -- pending|approved, since
                     counter TEXT,                           -- the connection counter, 20 digits
                     last_seen INTEGER,
                     version TEXT, paired_at INTEGER,        -- these five later (§7.2)
                     last_acked_epoch TEXT, last_acked_seq INTEGER, clock_offset_ms INTEGER);
CREATE TABLE agent_plans (agent_id TEXT, plan_id TEXT, plan_json TEXT, updated_at INTEGER,
                          PRIMARY KEY (agent_id, plan_id));
CREATE TABLE repo_owners (repo_uuid TEXT PRIMARY KEY, owner TEXT NOT NULL,  -- 'hub' | 'agent:<id>' | 'none'
                          epoch INTEGER NOT NULL, expires_at INTEGER, granted_at INTEGER);
CREATE TABLE repo_claims (repo_uuid TEXT, agent_id TEXT, requested TEXT, status TEXT,  -- granted|rejected
                          PRIMARY KEY (repo_uuid, agent_id));
```

**The operation log (M1b).** `migrations/0001_operations.sql`, at the repository root and applied by `sqlx::migrate!` when the store opens (§8.3), creates the `operations` and `op_warnings` tables, and nothing else above. The migrations after it:

- `0002_one_maintenance_per_repo.sql` adds `one_maintenance_per_repo` (I7, below).
- `0003_adopt_is_maintenance.sql` names `adopt` in it.
- `0004_operation_repository_alias.sql` adds `repo_alias`.
- `0005_succeeded_by_plan.sql` adds `ops_succeeded_by_plan` (below).
- `0006_schedule_state.sql` adds `schedule_state` without `last_success` (each scheduled plan's planned run, in milliseconds; the column arrives with the minimum-interval condition that reads it) and the queue index `ops_queue`, a partial index of the queued rows by ID, so reading the persistent queue doesn't grow with the history.
- `0007_retry_state.sql` adds `retry_state` (below).
- `0008_operation_locations.sql` adds `location_no` and `locations_json` to `operations` (below).
- `0009_api_tokens.sql` adds the `api_tokens` table and `token_id` and `token_label` to `operations` (below, decision 93).
- `0010_sessions.sql` adds the `sessions` and `sign_in_settings` tables (below, decision 70).
- `0011_repo_stats.sql` adds the `repo_stats` table (below, decision 104).
- `0012_release_check.sql` adds the `release_check` table (below, decision 107).
- `0013_proposals.sql` adds the `proposals` table (below, decision 89).
- `0014_proposal_decisions.sql` adds to `proposals` the columns of what a person decided (below, decision 89).
- `0015_operation_entry.sql` adds `entry` to `operations` (below, #847).
- `0016_hub_registry.sql` adds the hub's `pairing_tokens` and `agents` tables (below, decision 120).
- `0018_hub_tls_key.sql` adds the hub's `hub_tls_key` table (below, decision 120).
- `0019_agent_zone.sql` adds `zone` to the hub's `agents` (below, #114).

**Released migrations are append-only.** Every installed database records each applied migration's sqlx checksum and the store refuses to open one whose file has since changed, so a released migration is never edited, renamed or removed: a change is a new migration. `crates/adapters/store/tests/migrations_pinned.rs` pins each migration's checksum and fails on any such edit, or on a migration it doesn't list; adding a migration adds its line there in the same PR (#1068).

**The hub's migrations ship in every build,** a release before M8 included, though only a build with the hub writes to their tables before then, so that a database has one schema history whichever build opened it (§16, decision 122).

Later tables, columns and indexes arrive as migrations of the features that need them. As of version 16:

- `operations` has the columns the log writes: `id`, `agent_id` (always `local` for now), `kind`, `plan_id`, `repo_uuid`, `repo_alias` (below), `trigger`, `token_id` and `token_label` (below), `status`, `location_no` and `locations_json` (below), `entry` (below), `queued_at`, `started_at`, `finished_at`, `snapshot_id` (a backup's), `summary_json` (what the engine reported), `error_code`, `error_args`, `error_detail`, `warning_count`. `queued_at` and `warning_count` are `NOT NULL`. `kind` is `fake|identify|init|backup|forget|adopt|prune|check|ls`: `fake` and `identify` are the diagnostic and repository-opening jobs, `init` creates a repository, `forget`/`adopt`/`prune`/`check` are the maintenance ones (I7), `ls` a read that is never logged. `trigger`, `status` and the other columns take the values listed above. `repo_uuid` is the repository the operation runs on: an operation queued from an entry that records no ID names none while it is queued, and from its start the one its job opened, which an identify found first (`Operation::start_on`, decision 81).
- `kind` and `op_warnings.code` are each written and read by one spelling, the writer's, through one table each (`crates/adapters/store/src/codec.rs`). The store writes an operation's kind (`kind_text`) and reads the column through its table of every kind (`KINDS`); the domain writes a warning's code (`WarningKind::code`) and the store reads it through its table of every kind of warning but `other` (`WARNING_KINDS`). Reading a kind is strict: a word no kind is written as is `StoreError::Corrupt`, and a stored `ls` reads back as a listing. A warning code this version never wrote, `unknown` included, reads as `other` (below).
  - The schema has no `CHECK` on either column, so a new warning code needs no migration, and neither does a new kind unless it is a maintenance kind, which the SQL behind I7 names again (below).
  - The codec's tests walk every variant of both enums, which their derived `Deserialize` names, through a match with no wildcard that spells each word apart from the code: a new variant doesn't compile until it has its word there, and fails until its table reads it back. So a new `OperationKind` needs its word in `kind_text`, its row in `KINDS` and its word in that test.
- `repo_alias` (migration `0004_operation_repository_alias.sql`, decision 52) is the alias of the repository a maintenance operation (forget, adopt, prune, check) ran on, or a restore or a download read, as this agent named the repository when the run was first written: a maintenance run and a download when they start, a restore when it is queued (#893). The Operations page names the row by it, and a later rename of the alias leaves the row as it was. It is NULL for every other kind and for every row an earlier build wrote, which the page then names by what ran alone; adding the nullable column rewrote no row. It is read back as strictly as an ID: one that isn't a valid alias is corrupt.
- `location_no` and `locations_json` (migration `0008_operation_locations.sql`, decision 80) say which location of a repository reached at several a run used: `location_no` is its position, from 1 (NULL when the choice used none, as in a run that failed `no-location-usable`), and `locations_json` every location of the entry as the run's choice saw it, in order, as `[{"position": …, "address": …, "outcome": …, "source": …}]`.
  - `address` is the location's address as the entry wrote it then, credentials masked and cut at 256 bytes on a character boundary.
  - `outcome` is a `LocationOutcome` code (`used`, `not-tried` for one after it, `silent`, `other-repository`, …), the one the `location-skipped` warning and the API carry.
  - `source`, left out when there is none, names where an unresolved secret was looked for or the setting that can't be used, never a value.
  - The store owns this spelling (`locations.rs`), as it owns `summary_json`'s.
  - Both are NULL for a repository of one location, for every operation that opens none and for every row an earlier build wrote, which reads with no location; adding the nullable columns rewrote no row, and the change is one-way, as decision 52's was.
  - Reading is strict: a `location_no` that isn't the position of the record's `used` location, or that is set with no record, an unknown outcome, a missing field or more locations than an entry may have (`MAX_LOCATIONS`) is corrupt; a field a newer build added is ignored.
- `entry` (migration `0015_operation_entry.sql`, #847) names the entry of a snapshot a restore or a download read, so the history tells which file or folder it was. It is `/` and then the entry's path from the snapshot's top (`/` alone for the whole snapshot), each name as the file system wrote it (rustic's escaped form undone, a byte that isn't UTF-8 as U+FFFD), with every secret of the repository masked as its engine output is (I4), every character that would break or reorder a line made visible as a shown name is, masked again in that shown form, and a path past 1 KiB cut on a character boundary to its end after `…`.

  It is NULL for every other kind and for every row an earlier build wrote; adding the nullable column rewrote no row, and the change is one-way, as decision 52's was. A queued restore a restart ends keeps it.
- `token_id` and `token_label` (migration `0009_api_tokens.sql`, decision 93) name the API token a run with the trigger `api-token` was started with, its ID and its label as they were when it started, never its secret (I4); revoking the token later leaves the row as it was. Both are set for that trigger and NULL for every other one and for every row an earlier build wrote; a row whose trigger and token columns disagree is corrupt.
- `api_tokens` (migration `0009_api_tokens.sql`, §10.1, decision 93) keeps one row per API token: `id` (12 characters of lower-case base32), `label` (1 to 64 characters, not blank, no control characters), `digest` (the 32-byte SHA-256 of the secret; the secret itself is never kept), `scopes` (a comma list of `read|run|preview|propose`, at least one), `password` (the fingerprint of the admin password the token was created under, a signed 64-bit integer, §10.1) and `created_at` and `last_used_at` (milliseconds; `last_used_at` is NULL until the first use, and written at most once a minute per token). `ApiTokenStore::add` counts and inserts in one transaction under the write lock (`BEGIN IMMEDIATE`), so two creations at once never keep more than 100. Revoking deletes the row; listing and creating first delete the rows of another password's fingerprint. Reads are strict as above: an ID, label, scope list, digest length or time this build wouldn't write is `store-corrupt`.
- `sessions` (migration `0010_sessions.sql`, §10.1, decision 70) keeps one row per session, so sessions outlive a restart: `digest` (the 32-byte SHA-256 of the cookie's token; the token itself is never kept, and neither is the CSRF token, which a restart draws anew), `password` (the fingerprint of the admin password the session started under, as in `api_tokens`), `started_at` and `last_used_at` (milliseconds; `last_used_at` written at most once a minute per session, so a restart loses at most a minute of its idle time), `keep_signed_in` (1 for a session signed in with Keep me signed in on this device, which has no idle limit) and `secure` (1 for a session whose cookie is `Secure`, as its sign-in decided).
  - `SessionStore::add` inserts a session and deletes the ones it ended in one transaction.
  - Unlike `api_tokens`, a row this build can't read back is not `store-corrupt`: at start the daemon deletes those and logs how many, then the ones that have ended or started under another password, and brings back the rest.
- `sign_in_settings` (migration `0010_sessions.sql`, §10.1) holds at most one row (`id` 1): `session_length`, Stay signed in for, by its wire name (`until-browser-closes`, `7-days`, `30-days`, `90-days`, `1-year`, `until-sign-out`). There is no row until the admin first sets one, and the length is 30 days until then. Forgetting sessions never touches it.
- `repo_stats` (migration `0011_repo_stats.sql`, §16 decision 104) keeps one row per reading of a repository's stats: `id` (an integer key, in the order they were saved), `repo_alias`, `repo_uuid` (the ID the configuration entry recorded then, NULL when it recorded none), `taken_at` (milliseconds) and the figures `snapshots`, `total_bytes`, `stored_bytes` and `raw_bytes`, each `NOT NULL` and at least 0. An index on `(repo_alias, taken_at)` serves the latest reading and the retention. `StatsStore::record` inserts a reading and deletes the alias's readings taken more than 365 days before it, in one transaction; `latest` reads the alias's reading with the latest `taken_at`, the last saved among equals. A figure past `i64::MAX` (8 EiB) is refused as `store-unavailable` when it is kept, never wrapped, and a negative one read back is `store-corrupt`.
- `release_check` (migration `0012_release_check.sql`, §5, decision 107) holds at most one row (`id` 1): `checked_at`, when the agent last asked for Ladderback's newest release, and `update_version`, the available update it knew of then, as SemVer writes it (`0.3.0`), or NULL when there was none. There is no row until the first check. A version this build couldn't have written is `store-corrupt`; the release check then counts as never made, and its next check writes the row anew.
- `proposals` (migration `0013_proposals.sql`, §16 decision 89) keeps one row per proposal, pending or ended: `id`, the proposing token's `token_id` and `token_label` as they were then (never its secret, I4; both NULL for a session or any client while sign-in is off), `base` (the SHA-256, in hex, of `config.toml`'s bytes when it was drafted), `changes` (a JSON array of the changes as the wire writes them, each plan as its table in the file's keys, read back as the file is read when it loads), `rationale` (the proposer's own words, at most 4000 characters), `status` and `created_at` and `expires_at` (milliseconds, 48 hours apart).

  Migration `0014_proposal_decisions.sql` adds what a person decided, each NULL until its step happens: `decided_at` (milliseconds, when a person approved or rejected it), `reasons` (a rejection's reasons, one per change in order, as a JSON array of strings), `approved_from` (the address of the browser it was approved from) and `applied_as` (the configuration history's version its apply kept).

  `ProposalStore::add` reads the proposer's record (by `token_id`, NULL matching NULL): its pending proposals, when it made each proposal still kept, and the latest `decided_at` of its rejected ones. It refuses the proposal when the record says the proposer may not propose now (20 pending, 30 in the last hour, or 5 minutes after a rejection), and otherwise inserts it and forgets the oldest ended proposals beyond 200, all in one transaction under the write lock (`BEGIN IMMEDIATE`), so of two submissions racing for a proposer's last of 20 places one is kept.

  `change_status` changes a status only from the one named, in one statement, so of two changes racing from one status one is made; `reject` (`pending` to `rejected`, with `decided_at` and `reasons`), `approve` (`pending` to `applying`, with `decided_at` and `approved_from`) and `applied` (`applying` to `applied`, with `applied_as`) do the same. A proposal a hand edit sends back from `applying` to `pending` keeps the `decided_at` and `approved_from` of the approval it stopped, until a new approval writes them again. A pending row past its expiry, or whose changes no longer apply, is written `expired` or `stale` when it is next read. A row this build couldn't have written (an unknown status, a token ID without its label) is `store-corrupt`.
- `pairing_tokens` and `agents` (migration `0016_hub_registry.sql`, §7.1, decision 120) are the hub's registry, the `AgentRegistry` port's, which only a build with the hub reads or writes (decision 122).
  - `pairing_tokens` keeps one row per token the hub made: `pairing_key` (the 32-byte public half of its one-time key, never the seed, so the database holds no usable token), `hub_key`, `kind` (the binding kind byte of its address), `made_at` and `expires_at` (milliseconds), and `state` (`unused`, `used` or `revoked`); a used token names the agent it was spent on in `used_agent_id` and `used_agent_key`, which no other state sets.
  - `agents` keeps one row per agent, pending or approved: `id` (the agent ID, lower-case and hyphenated), `pubkey` (the agent key), `hostname` as it paired, `hostname_key` (the lower case of its NFC form, unique, so two agents can't hold one name), `status` and `status_at`, `counter`, `last_seen` and `zone`. `counter` is the highest connection counter the hub has taken, as 20 zero-padded decimal digits: a SQLite `INTEGER` holds no `u64` above `i64::MAX`, and the text compares in the counter's order. `zone` (migration `0019_agent_zone.sql`) is the machine zone the agent reported last, as the hub's bundled zone database spells it, or `NULL` when it reported none or one that database doesn't know (§7.2). A pairing writes it with the record, and `AgentRegistry::record_zone` writes the one each `Hello` reports over it.
  - `AgentRegistry::pair` reads the token, the agent under the ID, the agent holding the hostname and the count of pending agents, decides with `domain::fleet::decide_pairing`, and writes the agent and spends the token, in one `BEGIN IMMEDIATE` transaction, so of two pairings on one token one is paired. A refusal writes nothing. A row this build couldn't have written (a small-order key, an unknown binding kind, a counter past `u64::MAX`, a zone not shaped like a zone name) is `store-corrupt`.
  - `AgentRegistry::void_tokens_for_new_key` counts the agents and, when there are none, sets every `unused` token to `revoked`, in one `BEGIN IMMEDIATE` transaction. The hub runs it before it keeps a new hub key, which it makes only while no agent is registered: each unused token carries the key being replaced (decision 120, "Rotation, recovery and the handover chain"). A `Pair` whose token's `hub_key` isn't the hub's is refused as `hub-key-mismatch` whatever the token's state.
  - `hub_tls_key` (migration `0018_hub_tls_key.sql`) holds at most one row, the 32-byte fingerprint of the hub's TLS key (the SHA-256 of its certificate's SubjectPublicKeyInfo), recorded when the hub makes the key or first finds one with none recorded. `AgentRegistry::record_tls_key` inserts it unless a row is there, which stands, so of two first starts the first record is kept; `AgentRegistry::tls_key` reads it. A hub whose `hub-tls-key` is gone, or holds another key than the recorded one, doesn't serve its TLS and makes no replacement (decision 120, "The hub key"). So does a hub with agents, no row and no key file, a development hub from before the table, since an agent may have pinned a key whose record went with an earlier database.
- A backup's `summary_json` keeps `bytes_processed`, the bytes of the source the backup went through, changed or not, only when the engine reported them: the key is left out otherwise, so a summary an earlier build wrote reads back as it was, with none.
- Times are Unix **milliseconds**, UTC, cut to the millisecond and never rounded.
- `retry_state` holds each plan's pending retry, one row per plan: `attempt` (from 1), `retry_at` (when it runs) and `until` (the plan's next scheduled run when its series began, `NULL` for a plan with no schedule), in milliseconds. A row goes once its retry's run is recorded, when its series ends, when its plan no longer sets `retry` and when the plan's `attempts` go below its `attempt`; the scheduler reads them once, as it starts, so a retry planned before a restart runs after it (§5, decision 76). A row this version didn't write, such as an `attempt` below 1, is `store-corrupt`.
- `ops_succeeded_by_plan` is a partial index on `plan_id` of the rows whose `status` is `succeeded` or `succeeded_with_warnings`: whether a plan ever succeeded is one probe of it, however many of its operations failed, and the Repositories page asks it of a repository whose newest operation found none there (decision 53). The query names the two statuses as literals, so SQLite matches it to the index when it prepares it. Migration `0005` only adds the index: no table is rebuilt and no row changes.
- `ops_by_plan` is `(plan_id, id DESC)`, not `started_at`: history pages by operation ID, which is time-ordered (UUIDv7), and a queued operation has no start yet.
- The cap on warnings is the `Operation` aggregate's: it keeps the first 10,000 and counts the rest, so `op_warnings` holds at most 10,000 rows per operation and `warning_count` the total. Warnings are read back in the order they were recorded. The aggregate also caps each text field of a warning it keeps, `item`, `during` and `detail`, at 4,096 bytes, so one operation's warnings take about 120 MB at the most: longer text is cut at a character boundary and ends in `…`, and text of exactly 4,096 bytes is kept whole.
- A failed operation stores its reason as `error_code` (the failure's kebab-case code: the store owns the literals, which match `api::ErrorCode`'s), `error_args` (a JSON array of `[name, argument]` pairs, `[]` for none) and `error_detail` (the platform's or engine's own text, verbatim; an empty text is `''`, not NULL). The three columns are NULL unless the operation failed. The codes are:
  - `engine-unavailable`
  - `invalid-job-spec`
  - `engine-exited` (argument `exit_code`, left out when no exit status is known)
  - `operation-failed`
  - `protocol-violated` (argument `transition`)
  - `deadline-passed`
  - `repository-exists`
  - `repository-replaced` (no argument: the location holds another repository than the ID the configuration recorded, multi-machine.md §7.3)
  - `repository-not-found` (no argument and no detail: the job found an empty folder on this machine, local or OpenDAL `fs`, where the repository should be, decision 53)
  - `repository-config-not-found` (no argument; the detail is the engine's words, or for an OpenDAL `fs` root folder that isn't there Ladderback's naming it: the job found no repository config file where it looked, decision 112)
  - `credentials-refused` (no detail: a REST server answered 401 or 403 to the repository's credentials, decision 85; argument `half`, `cold` or `hot`, for a hot/cold repository, left out otherwise, and a stored half that is neither doesn't read back, decision 112)
  - `engine-start-timed-out`
  - `engine-exit-timed-out`
  - `hook-failed` (arguments `level`, `agent`, `repository` or `plan`, and `position`, the hook's place in that level's `run-before` list from 1; `ended`, `exit`, `signal`, `timeout` or `not-started`; and `exit_code`, for an exit only. A row an earlier version wrote has only `exit_code`, or nothing: it reads back with no hook, and with how it ended taken from `exit_code` when there is one, #480)
  - `secrets-too-large-to-mask` (arguments `limit`, in bytes, and `field`, the proxy variable whose credentials took the job's secrets past the cap, left out when the job's own secrets did, security.md §10.2)
  - `daemon-stopped` (no argument and no detail: startup recovery wrote it for an operation the daemon never saw end)
  - `daemon-shut-down` (no argument and no detail: the daemon's orderly stop wrote it for an operation still running once its workers had stopped, with the stop's time as `finished_at`; §4.2, decision 92)
- The store owns the spelling of `error_args`, since the format is part of the data-migration contract: an argument is `{"type": …, "value": …}` with `type` one of `count`, `bytes`, `duration-ms` (a number each), `text` (a string) or `timestamp` (an object with `utc_ms`, the milliseconds since the Unix epoch, and `zone` when there is one). `api::MsgArg` is mapped to and from it at the store's edge (`error_args.rs`), so the API can rename or reshape its argument type without changing what a stored row means, and a new `MsgArg` kind doesn't compile until the store has spelled it.

  The spelling is the one the store already wrote, byte for byte, so no migration was needed and rows written before the type existed read as they always did. Reading stays strict about the kinds: a `type` this version never wrote, a value of the wrong shape or a zone that isn't shaped like a time-zone name is `StoreError::Corrupt`.
- `OperationStore::save` writes one operation by ID in a single transaction and replaces its warnings. The job runner saves an operation when it is queued, when the engine reports it running and when it finishes, and never in between, so a busy backup doesn't write the database for every progress event.

  A write the store refuses, or that is still not answered after 20 s (`SAVE_BOUND` in `app::log_write`, past the store's own bound of about 15 s: a stalled disk, not a busy one), never changes how the operation ends and never holds a run or a cancel (backups never depend on the database); the run reports it, drops the write where it stands, and the next write is the whole operation again (the last write has no next one: the operation stays as the log last had it, as after a refused write). The write that says an operation is running does not hold a cancel or the deadline either: they cut it short.

  The pages are told how the operation ended once its last write is done, whether the log stored it, refused it or was given up on (§4.4): a page that reads the history on hearing it finds the end there unless that write was lost, and then the history shows the operation as the log last had it, and the next start ends it as `daemon-stopped` (below).
- When a run ends the daemon says so in its own log, once, and says each write the log refused the run: one line for each, at WARN, with the code of the failure (`StoreError::code`, `store-unavailable` or `store-corrupt`) and never its words, which nobody has checked for a value (I4). A run goes on without a log that refuses its writes, so a log that has stopped taking them would otherwise be invisible to the person running the daemon (#135). A maintenance start the log refuses began no operation, so it has no end to log or to announce to the pages: the client is told it was refused.
- A write dropped where it stands, by a cancel, the deadline or the bound above, leaves the log as usable as it was: it has stored the whole operation or none of it, and the write after it isn't held up by what it began. sqlx keeps that for the SQLite store: it starts a transaction on the connection's own thread, which rolls back a `BEGIN` whose answer nobody took, and a dropped transaction queues its rollback ahead of the pool taking the connection back, so the connection goes back to the pool outside any transaction.

  sqlx 0.8 didn't: a write dropped after the thread had acknowledged the start but before the transaction was handed back left it open, holding the database's write lock, and the pool took the connection back that way: the next write waited out the five-second busy timeout and failed (or was given that connection and failed at once), and so did every write after it until the connection was closed, so the log never learned how the operation ended and the history showed it as queued (issue #123). The store closed such a connection instead of returning it until sqlx 0.9 made that unneeded (#174).

  The `OperationStore` contract says so, and `crates/adapters/store/tests/cancellation.rs` cuts a save and a start at each of their await points, so a sqlx that stops rolling back fails them. Which await point a cut after a given number of polls lands on depends on how the connection's thread is scheduled, so the tests count no polls: each first cuts the call once its `BEGIN IMMEDIATE` has run and before the call has taken the answer, the cut of #123, which a second connection that holds the write lock while the call is polled makes them reach every run (#166).
- A caller isn't told whether a write it dropped stored anything, so one that has no one to end what the write stored doesn't drop it. A `try_start` cut short after SQLite committed it leaves a `running` row, and for a maintenance operation that row holds its repository (I7) until something ends it: in a live process only the run that was to follow does, and recovery ends it at the next start.

  `crates/adapters/store/tests/cancellation.rs` cuts a start at each of its await points and checks that the row is then `queued` or `running`, never in between, and that another maintenance start is refused exactly when it is `running`. No lock another connection holds keeps a commit back in WAL mode, so the store's own `a_start_cut_after_its_commit_holds_the_repository` makes the cut after the commit every run: a commit hook on the store's connections holds the start's commit until the call has stopped being polled, and the call is dropped once another connection sees the start.

  The daemon's route therefore never drops one: `RunMaintenance::assess` reads and decides on the request's own future and holds nothing, and the task that `start_maintenance` then spawns takes the slot and runs the operation to its end (§16, decision 42).
- History lists newest first (`id DESC`) with a keyset cursor: `after` is the `next` of the previous page and need not name an operation that still exists. A page holds 50 operations unless the caller asks for another size, always between 1 and 200. The only filter is the plan. Each row carries the codes of its operation's kept warnings (the first 10,000), each code once, in the order the first of each was recorded, read in the same read transaction as the page, so a save between the two reads isn't seen half done (#508).
- Reads are strict: an unknown status, kind, trigger or error code, malformed JSON, an ID or time that doesn't parse, or a warning with no code is `StoreError::Corrupt`, never a silent default (one bad row fails the page that contains it). A database that holds a migration this build doesn't ship, because a newer Ladderback wrote it, is refused with `OpenError::NewerSchema` and left as it was.
- The store opens the file in WAL mode with `synchronous = NORMAL`, a five-second busy timeout and a pool of four connections, and a save takes the write lock when it starts (`BEGIN IMMEDIATE`), so concurrent saves queue instead of failing half way. `op_warnings.op_id` references `operations(id)` and its rows go with their operation. Reads are lenient only about additions: fields and error arguments this build doesn't know are ignored and an unknown warning code reads as `other`; an ID that isn't in its canonical lower-case form is corrupt.

**One maintenance operation per repository (I7).** A running operation of kind `prune` or `check` (and `forget`, which the index names ahead of its domain kind) that names its repository *holds* that repository: no other maintenance operation may be running on it. Queued and finished operations, backups, identifies and diagnostic jobs hold nothing, are never refused, and never make another operation wait. `OperationRecord::maintaining` (through `maintained_repository`) names the repository a record holds, and is the definition the in-memory store and SQLite's `try_start` use; `one_maintenance_per_repo` and the `try_start` lookup spell the same rule in SQL: a unique index on `repo_uuid` over the rows with `status = 'running'` and `kind IN ('forget', 'prune', 'check')`.

**A read is not an operation (M1b).** Listing a repository's snapshots (`OperationKind::Ls`, `SnapshotDto`) takes no I7 slot and writes no row to `operations` or the operation log: I7 exists because forget/prune/check *write* to the repository, and a read is safe to run alongside anything. The UI fetches the list live from the engine (`Job::Ls`), so nothing stores it. `ls` therefore never appears in `operations.kind`, and a listed snapshot's `time` crosses the API as an RFC 3339 string with its paths and tags capped at 64 each.

Its `offset` is the offset of the clock that took it, as RFC 3339 writes one (`-06:00`, seconds only when it has some), read from the snapshot file; a daemon older than the field leaves it out, and the daemon refuses a worker's listing with an offset no clock has (beyond ±25:59:59) (§16, decision 56). A snapshot that had more, or text cut short, says so in `paths_truncated` and `tags_truncated`; a listing of any length reaches the daemon as a series of worker events (§4.1, decision 32).

Reading a page of a snapshot's folder (`Job::Browse`, decision 102) is a read in the same way: no I7 slot, no row, and it waits for one of the daemon's few turns to read a folder ([§4.1](components/worker.md#41-worker-process-the-engine-boundary)).

A download (`Job::Dump`, §10.4, decision 114) reads the repository too and takes no I7 slot, but it is a row, as a restore is, because it hands a snapshot's contents out of the agent: kind `download`, trigger `manual`, written once the worker has found the entry (running), and again when it ends. It succeeds with the bytes it handed over (`Summary::Dump`), fails when a piece couldn't be read, and is cancelled when the browser went away or never fetched it within its minute. Like a restore's, its row names the entry it read (`entry`, above).

The downloads whose minute ran out are given up together: each stops its worker and gives back its turn, cancelled as of then, before the first of their rows is written, and the rows are then written one after another, so a slow log holds no worker or turn (#888). A download being confirmed gives them up first and takes its own turn before their rows are written, so one asked for while the log is slow can't take the turn it freed.

- `OperationStore::try_start(&Operation)` stores an operation that has just started running, as `save` does, and decides in the same atomic step. The result is `Start::Started`, or `Start::Refused { running }` naming the holder, with nothing stored. The operation that holds a repository may start again. Of any number of starts racing for one repository, through any number of handles, exactly one wins. An operation that isn't running is `StoreError::Unavailable`. SQLite takes the write lock (`BEGIN IMMEDIATE`) before it looks for a holder, so no other handle can start one between the look and the write; a holder whose ID can't be read is `StoreError::Corrupt` and the start fails closed.
- `save` stays unconditional about everything but the index: a `save` that would make a second running maintenance operation fails with `StoreError::Unavailable` and changes nothing (SQLite reports the index's violation, the in-memory store checks the same rule). It is a backstop, not the way to start one.
- Migration `0002` must cope with a log an earlier build wrote, which may already hold several running maintenance rows on one repository. It keeps the newest by ID running, and turns each older one into a `failed` operation with `error_code = 'engine-exited'`, `error_args = '[]'`, `error_detail` NULL, as a run that died with no exit status. Their `finished_at` stays NULL: when they ended is unknown, and nothing needs a finish time of a finished operation (the read model has it optional, the history order is by ID, and a duration is `None` without one). Rows of other kinds, queued rows and rows with no repository are not touched.
- **The contract for the maintenance slice.** `RunOperation` is unchanged: it saves `Running` once the engine reports `Started`, and has no product caller yet. The maintenance use case must call `try_start` *before* it starts the engine, start the engine on `Started` only, and fail closed on `Refused` and on any error from the store. Otherwise the `save(Running)` of a second maintenance operation fails into the run's recording errors while the engine is already running.
- Open points.
  - ~~A crash leaves its `running` row holding the repository until startup recovery (the queue, M2) closes it.~~ Resolved 2026-11-10: startup recovery (below) ends the row as failed before the daemon accepts a request, and the repository is free again.
  - ~~Maintenance with no repository ID isn't serialized: `RepositoryAccess.id` is `None` until the first `Identify`, so the maintenance slice must pass a known repository ID.~~ Resolved 2026-11-09: `RunMaintenance` runs an `Identify` read before `try_start`, so every maintenance start holds a known repository ID and the I7 index serializes it; the ID is never persisted in config.
  - The list of maintenance kinds is spelled three times (`OperationKind::is_maintenance`, the index, the lookup in `sql.rs`): a new maintenance kind must change all three, the index through a new migration that drops and recreates it.
  - `StoreError::Unavailable` also carries the store's refusals under I7, which can't succeed on a retry until the holder ends; a typed variant can replace that when a caller needs to tell them apart.

Retention: logs 30 days, operations 1 year (configurable), repository stats 1 year (decision 104), snapshot cache rebuilt as needed.

**Recovery.** Backups never depend on the database. It keeps a daily copy (`VACUUM INTO`). `ladderback db rebuild` recreates a corrupt database: schedule state from config, the snapshot cache from the repositories, and history from the latest daily copy. Backrest has open SQLite failures on btrfs and with corruption ([#978](https://github.com/garethgeorge/backrest/issues/978), [#1174](https://github.com/garethgeorge/backrest/issues/1174)), so CI runs the store tests on btrfs and a Docker volume, and NFS is documented as unsupported for the data dir.

**One process per data dir.** `serve` takes an exclusive `flock` on `<data>/ladderback.lock` before it reads the configuration or opens the database, and a second process on the same data dir is refused and touches nothing: `cli-data-dir-in-use` names the data folder and the holder's PID, and `cli-data-dir-in-use-unnamed` stands in when the file names none. Both advise a configuration folder and a data folder of their own: a second daemon with the first one's configuration and a data folder of its own would keep a log of its own, and I7's slot is a row in that log.

On Linux and macOS the holder may be the installed service, which its manager starts again once its process is killed (operations §12, #1035), so "stop that process" alone would lead to a kill the manager undoes. There the refusal is `cli-data-dir-in-use-systemd` or `cli-data-dir-in-use-launchd` (each with its `-unnamed-` form), chosen when the binary is built: the same words, then the manager's own stop (`systemctl --user stop ladderback`, or `launchctl bootout gui/$(id -u)/io.github.nledford.ladderback`) and `ladderback remove-service`, as literals the person types.

The refusal is a command-line message, not an API code: it happens before anything listens, so no request could carry it. `mcp --headless` and `db rebuild` will take the same lock when they exist (M3, M10). SQLite's own locking allows several writers, so without this two schedulers could share one queue, and the second daemon's startup recovery (below) would end the first one's live operations as a crash's leftovers.

The holder writes its PID into the file once it has the lock, and a refused process reads it without changing it; the file stays after the holder exits and names a process that is gone until the next holder writes its own. The OS releases the lock when the process dies, so a crash never leaves it stale. The file is the lock itself, not a stale PID file: it is never deleted while a daemon serves, because a daemon that found none would create a fresh one and lock that.

A file system that can't lock (some network and FUSE mounts) stops the start with `cli-cannot-lock-data-dir`, naming the file; a daemon never runs without the lock. A daemon of a build from before this lock takes none, so it must be stopped before this build starts on its data dir: recovery would end the operations it is still running. The lock is std's `File::try_lock`, so it needs no dependency of its own.

A daemon that stops lets go of the lock last, after it has closed the operation log (§4.2, shut-down order), so no other daemon recovers the log while one of its writes could still land.

**Startup recovery.** Before it binds a socket, `serve` ends every operation the log still shows as `running`: the daemon that was running them stopped (a crash, a kill, a power cut, or a stop that didn't get as far as ending them) before they ended, and the lock above says no other process is running them now. An orderly stop ends what it interrupted itself, as its operation log closes: `daemon-shut-down`, finished at the stop's time, in one transaction, recorded and not announced (§4.2, decision 92), so a start finds nothing of it to end.

They end as `failed` with the typed reason `daemon-stopped`, never as succeeded: nothing the log holds says how far the engine got, so the outcome is unknown, and the message (`error-daemon-stopped`) says so. It is one transaction under the write lock (`OperationStore::recover`), which ends the running rows and reads the queued ones, oldest first, so it is all or nothing: a crash during recovery leaves the log as it was for the next start, and recovering twice is recovering once.

A recovered row keeps its `started_at`, target and warnings; `finished_at` stays NULL, as for the rows migration 0002 demoted, because when the daemon stopped is unknown. A running maintenance row ending is what frees its repository: the I7 slot is that row, so no other code lets it go.

Recovery runs no hooks, which belonged to the process that is gone, but it announces each operation it ended once the transaction has committed, as any failed operation is announced (`OperationFailed` with `daemon-stopped`, §4.4), so the notification rules hear it (§11): its time is the start's, and it is told at most once per start, since nothing records that the news went out: a run whose end decision 41 announced though the log refused its last write is still `running` here, so it is announced again, as `daemon-stopped` (decision 91).

A queued operation never reached the engine, so recovery leaves it queued: it is the persistent queue's, which runs it once the daemon listens (§5, decision 73), and announces one it can't run again once the log has its end; a queued row this build can't read stays queued and is said in the daemon's log, by its code, and so is a running row whose ID it can't read, which ends all the same but can't be announced. A log that can't be changed refuses the start (`cli-cannot-recover-operations`), because serving over it would keep a repository held for good.

The order of a start is the data folder, its lock, the configuration, the log, the event bus and the notifier, recovery, the agent ID, the engine, the listener and the `listening on` line, and then the queued backups and the scheduler.

Recovery ends rows, not processes. A worker the dead daemon left behind ends by itself: on Linux the kernel signals it when its parent dies, and elsewhere it ends at its next event, when the pipe is broken (§4.1). Until then a worker in a long silent phase on macOS, or an `rclone` it started, can still be writing to the repository, and a repository lock a killed worker left behind is not recovery's to clear.
