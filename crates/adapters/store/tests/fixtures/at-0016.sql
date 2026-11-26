-- A database as migrations 0001-0016 of the set before the baseline (§16,
-- decisions 165 and 170) left it: the schema Ladderback 0.5.0 shipped. Its
-- rows are those of `at-0029.sql` each table at 0016 has, column by column
-- (an API token's scopes, which migration 0017 replaced, as 0.5.0 wrote
-- them), so the store's copy takes it through migrations 0017-0029 to the
-- same rows. The store copies it into a baseline database on start.
PRAGMA foreign_keys=OFF;
BEGIN TRANSACTION;
CREATE TABLE operations (
  id            TEXT PRIMARY KEY NOT NULL,   -- UUIDv7, lowercase hyphenated: its text order is time order
  agent_id      TEXT NOT NULL,               -- 'local' on agents (§9)
  kind          TEXT NOT NULL,               -- fake|identify|backup|prune|check
  plan_id       TEXT,
  repo_uuid     TEXT,                        -- the rustic repository ID
  trigger       TEXT NOT NULL,               -- schedule|manual|catch-up|retry|hub|mcp
  status        TEXT NOT NULL,               -- queued|running|succeeded|succeeded_with_warnings|failed|cancelled
  queued_at     INTEGER NOT NULL,            -- Unix MILLISECONDS, UTC
  started_at    INTEGER,
  finished_at   INTEGER,
  snapshot_id   TEXT,                        -- a backup result's snapshot
  summary_json  TEXT,                        -- the engine's result, as JSON
  error_code    TEXT,                        -- the failure's api::ErrorCode (kebab-case); failed operations only
  error_args    TEXT,                        -- JSON array of [name, MsgArg] pairs; failed operations only
  error_detail  TEXT,                        -- the engine's or platform's own text, verbatim; failed operations only
  warning_count INTEGER NOT NULL DEFAULT 0   -- every warning recorded, the rows of op_warnings or not
, repo_alias TEXT, location_no INTEGER, locations_json TEXT, token_id TEXT, token_label TEXT, entry TEXT);
INSERT INTO operations VALUES('00000000-0000-0000-0000-000000000001','local','backup','demo-home','abababababababababababababababababababababababababababababababab','schedule','succeeded_with_warnings',1800000000000,1800000010000,1800000070000,'cdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcd','{"type":"backup","repository":"abababababababababababababababababababababababababababababababab","snapshot":"cdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcd","parent":"efefefefefefefefefefefefefefefefefefefefefefefefefefefefefefefef","files_new":1,"files_changed":2,"files_unmodified":3,"bytes_processed":7,"data_added":4,"data_added_packed":5,"error_count":6}',NULL,NULL,NULL,2,NULL,NULL,NULL,NULL,NULL,NULL);
INSERT INTO operations VALUES('00000000-0000-0000-0000-000000000002','local','prune',NULL,'abababababababababababababababababababababababababababababababab','manual','failed',1800000000000,1800000010000,1800000070000,NULL,NULL,'operation-failed','[]','repository locked',0,NULL,NULL,NULL,NULL,NULL,NULL);
INSERT INTO operations VALUES('00000000-0000-0000-0000-000000000003','local','check',NULL,'abababababababababababababababababababababababababababababababab','manual','queued',1800000000000,NULL,NULL,NULL,NULL,NULL,NULL,NULL,0,NULL,NULL,NULL,NULL,NULL,NULL);
INSERT INTO operations VALUES('00000000-0000-0000-0000-000000000004','local','backup','demo-home','abababababababababababababababababababababababababababababababab','schedule','succeeded',1800000000000,1800000010000,1800000070000,'cdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcd','{"type":"backup","repository":"abababababababababababababababababababababababababababababababab","snapshot":"cdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcd","parent":"efefefefefefefefefefefefefefefefefefefefefefefefefefefefefefefef","files_new":1,"files_changed":2,"files_unmodified":3,"bytes_processed":7,"data_added":4,"data_added_packed":5,"error_count":6}',NULL,NULL,NULL,0,NULL,NULL,NULL,NULL,NULL,NULL);
CREATE TABLE op_warnings (
  op_id  TEXT NOT NULL REFERENCES operations(id) ON DELETE CASCADE,
  item   TEXT,                                -- NULL and '' are different things
  during TEXT,
  code   TEXT,                                -- the api::WarningCode (kebab-case)
  detail TEXT
);
INSERT INTO op_warnings VALUES('00000000-0000-0000-0000-000000000001','/home/demo/file-0','backup','source-unreadable','permission denied');
INSERT INTO op_warnings VALUES('00000000-0000-0000-0000-000000000001','/home/demo/file-1','backup','source-unreadable','permission denied');
CREATE TABLE schedule_state (
  plan_id  TEXT PRIMARY KEY NOT NULL,
  next_run INTEGER NOT NULL
);
INSERT INTO schedule_state VALUES('demo-home',1800003600000);
CREATE TABLE retry_state (
  plan_id  TEXT PRIMARY KEY NOT NULL,
  attempt  INTEGER NOT NULL,
  retry_at INTEGER NOT NULL,
  until    INTEGER
);
INSERT INTO retry_state VALUES('demo-work',2,1800000600000,1800007200000);
CREATE TABLE api_tokens (
  id           TEXT PRIMARY KEY NOT NULL,
  label        TEXT NOT NULL,
  digest       BLOB NOT NULL,
  scopes       TEXT NOT NULL,
  password     INTEGER NOT NULL,
  created_at   INTEGER NOT NULL,
  last_used_at INTEGER
);
INSERT INTO api_tokens VALUES('tok-1','Demo script',x'0303030303030303030303030303030303030303030303030303030303030303','read,run',-42,1800000000000,NULL);
CREATE TABLE sessions (
  digest         BLOB PRIMARY KEY NOT NULL,
  password       INTEGER NOT NULL,
  started_at     INTEGER NOT NULL,
  last_used_at   INTEGER NOT NULL,
  keep_signed_in INTEGER NOT NULL DEFAULT 0 CHECK (keep_signed_in IN (0, 1)),
  secure         INTEGER NOT NULL DEFAULT 0 CHECK (secure IN (0, 1))
);
INSERT INTO sessions VALUES(x'0404040404040404040404040404040404040404040404040404040404040404',-42,1800000000000,1800000001000,1,0);
CREATE TABLE sign_in_settings (
  id             INTEGER PRIMARY KEY NOT NULL CHECK (id = 1),
  session_length TEXT NOT NULL CHECK (session_length IN (
    'until-browser-closes', '7-days', '30-days', '90-days', '1-year', 'until-sign-out'
  ))
);
INSERT INTO sign_in_settings VALUES(1,'7-days');
CREATE TABLE repo_stats (
  id           INTEGER PRIMARY KEY NOT NULL,
  repo_alias   TEXT NOT NULL,
  repo_uuid    TEXT,
  taken_at     INTEGER NOT NULL,
  snapshots    INTEGER NOT NULL CHECK (snapshots >= 0),
  total_bytes  INTEGER NOT NULL CHECK (total_bytes >= 0),
  stored_bytes INTEGER NOT NULL CHECK (stored_bytes >= 0),
  raw_bytes    INTEGER NOT NULL CHECK (raw_bytes >= 0)
);
INSERT INTO repo_stats VALUES(1,'demo','abababababababababababababababababababababababababababababababab',1800000000000,12,1000,400,900);
CREATE TABLE release_check (
  id             INTEGER PRIMARY KEY NOT NULL CHECK (id = 1),
  checked_at     INTEGER NOT NULL,
  update_version TEXT
);
INSERT INTO release_check VALUES(1,1800000000000,'0.9.0');
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
  expires_at  INTEGER NOT NULL, decided_at INTEGER, reasons TEXT, approved_from TEXT, applied_as TEXT,
  CHECK ((token_id IS NULL) = (token_label IS NULL))
);
INSERT INTO proposals VALUES('00000000-0000-0000-0000-0000000000b1','tok-1','Demo script','rev-1','changes-1','Back up the demo folder hourly','rejected',1800000000000,1800086400000,1800000001000,'["Not now"]',NULL,NULL);
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
INSERT INTO pairing_tokens VALUES(x'0505050505050505050505050505050505050505050505050505050505050505',x'0606060606060606060606060606060606060606060606060606060606060606',1,1800000000000,1800000900000,'used','00000000-0000-0000-0000-0000000000a1',x'a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1');
INSERT INTO pairing_tokens VALUES(x'0707070707070707070707070707070707070707070707070707070707070707',x'0606060606060606060606060606060606060606060606060606060606060606',1,1800000000000,1800000900000,'used','00000000-0000-0000-0000-0000000000a3',x'a3a3a3a3a3a3a3a3a3a3a3a3a3a3a3a3a3a3a3a3a3a3a3a3a3a3a3a3a3a3a3a3');
INSERT INTO pairing_tokens VALUES(x'0808080808080808080808080808080808080808080808080808080808080808',x'0606060606060606060606060606060606060606060606060606060606060606',1,1800000000000,1800000900000,'unused',NULL,NULL);
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
INSERT INTO agents VALUES('00000000-0000-0000-0000-0000000000a1',x'a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1','Laptop','laptop','approved',1800000000000,'00000000000000000007',1800000005000);
INSERT INTO agents VALUES('00000000-0000-0000-0000-0000000000a2',x'a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2','Desktop','desktop','pending',1800000000000,'00000000000000000000',1800000005000);
CREATE TABLE _sqlx_migrations (
    version BIGINT PRIMARY KEY,
    description TEXT NOT NULL,
    installed_on TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    success BOOLEAN NOT NULL,
    checksum BLOB NOT NULL,
    execution_time BIGINT NOT NULL
);
INSERT INTO _sqlx_migrations VALUES(1,'operations','2026-11-25 21:45:46',1,x'27ad56a7416ee4e55fdbc6c7671a82d8783984ee9fa2a95253ef23bd3db95f0388e1a69b1048fd42adb5467c8d808a5f',345125);
INSERT INTO _sqlx_migrations VALUES(2,'one maintenance per repo','2026-11-25 21:45:46',1,x'495792bc209277ec7b71b6952b3b2296ef7bc07f5ddda4516ec9a87be30a19f01bd7716dfd971a956e5934f877bff5ab',216375);
INSERT INTO _sqlx_migrations VALUES(3,'adopt is maintenance','2026-11-25 21:45:46',1,x'8d1f91dd6af09506838384d197258b81d28cb5404aac888ac3a55934300389a7bf358b8f4f3e12a8dca58404ba59314d',259583);
INSERT INTO _sqlx_migrations VALUES(4,'operation repository alias','2026-11-25 21:45:46',1,x'cebb9f0f4846fd5f8b3ee81c1fb8027d2c6699db299c8aa04d142eee3b35ecca7d41eda62b28ca4948ab2d03e7ea6960',216000);
INSERT INTO _sqlx_migrations VALUES(5,'succeeded by plan','2026-11-25 21:45:46',1,x'10159eec955d734ba13c1c07860c97510541111484e5ee33e12939ddeaa9fdfadfb29b9810072941e8d931c4be23bf78',130125);
INSERT INTO _sqlx_migrations VALUES(6,'schedule state','2026-11-25 21:45:46',1,x'4a84f175b1178248cfe28b1117d4757156533a3a7dfce2b11e8e9a8e96545d2b7f7142088842cef74ada58758b6ca1ae',183000);
INSERT INTO _sqlx_migrations VALUES(7,'retry state','2026-11-25 21:45:46',1,x'1729efcdc036ba1818418d1a64ef9444c709160cbd3e0471cdefeb2ee6e56f47971704d94f828bae66d51639e68c75e8',198500);
INSERT INTO _sqlx_migrations VALUES(8,'operation locations','2026-11-25 21:45:46',1,x'7154dd8356081399fbbcad0bb11fef4add0563974f83b02131b6a85c9f5c5755662aaa6e394d4127f8249d48ec02c4c6',397000);
INSERT INTO _sqlx_migrations VALUES(9,'api tokens','2026-11-25 21:45:46',1,x'ddc0bc83e36ff7d2bf9f2c00032a2b927779748afc5fa073628810cf9d1437f7797bfcb589ec5b10c450724ff9767ac8',358416);
INSERT INTO _sqlx_migrations VALUES(10,'sessions','2026-11-25 21:45:46',1,x'961881a91d88912756b22c2ad65aabb9e5aa88b8a34b443236d717bdb71ca096f35ed18ac68529c733b4c895cfcd21bd',265583);
INSERT INTO _sqlx_migrations VALUES(11,'repo stats','2026-11-25 21:45:46',1,x'cd8d0a85daae74ff46af6c76947d7c7f752c0f3088e8147e42d3f9e8ba57017308d0f1db91e1319845d6b303952a0ac4',196833);
INSERT INTO _sqlx_migrations VALUES(12,'release check','2026-11-25 21:45:46',1,x'6f46345273e91c654e3cde06e8aa1ac34119bc65836707ff1402c8497ebc7d4fb809d03ee6a7ae23f413f032b206c238',141208);
INSERT INTO _sqlx_migrations VALUES(13,'proposals','2026-11-25 21:45:46',1,x'7fa15de54028b669ff140db4200844314faf4c280bdaf37abbb1fb1fe2f0c02cd084a32d216739265d81e9df77670299',211333);
INSERT INTO _sqlx_migrations VALUES(14,'proposal decisions','2026-11-25 21:45:46',1,x'67517d0b1f90020e6290e1a810f07e66cd7447205e02863022393026f9068ce47d5698428f8f85219c17bdae670fcf75',580500);
INSERT INTO _sqlx_migrations VALUES(15,'operation entry','2026-11-25 21:45:46',1,x'342a20b97c987a48dfe03883e9356e8be99b82b3ce089dea11e75538bdd08924dfb59d357be8b70a8abfb46696f10c20',257959);
INSERT INTO _sqlx_migrations VALUES(16,'hub registry','2026-11-25 21:45:46',1,x'd358afcedf70fbedbb51e9016085519b92106907ae3bd4f5361bd1d57101037c7495f6097c867edb6edcd97674ef0cc7',262875);
CREATE INDEX ops_by_plan ON operations(plan_id, id DESC);
CREATE INDEX op_warnings_by_op ON op_warnings(op_id);
CREATE UNIQUE INDEX one_maintenance_per_repo ON operations(repo_uuid)
  WHERE status = 'running' AND kind IN ('forget', 'prune', 'check', 'adopt');
CREATE INDEX ops_succeeded_by_plan ON operations(plan_id)
  WHERE status IN ('succeeded', 'succeeded_with_warnings');
CREATE INDEX ops_queue ON operations(id) WHERE status = 'queued';
CREATE INDEX repo_stats_by_alias ON repo_stats (repo_alias, taken_at);
CREATE INDEX proposals_by_status ON proposals (status, created_at);
COMMIT;
