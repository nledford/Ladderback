# Decision 165: One baseline schema replaces the migrations before the first release

**`migrations/0001_baseline.sql` replaces migrations 0001 to 0029 before the first release. It makes the same tables under the machine names (decision 163) and adds the rules the code already kept: every table `STRICT`, a `CHECK` on each value the store writes from a fixed list, a failure only on a failed operation, `json_valid` on the JSON the store reads, times and counts at least 0, a machine's current state deleted with it by `ON DELETE CASCADE`, and an operation's report held once across machine IDs (#1513). `operations.machine_id`, always `local`, is gone. A database the old migrations left is refused with `OpenError::OldSchema` and left as it was; `scripts/copy-to-baseline.ts` copies one at migration 29 into the baseline once. From the first release on, migrations are append-only again** (recorded 2026-11-25, #1618; §9).

## Why

The 29 migrations were a project's history before anyone ran a release. Every new database ran all of them, several rebuilding a table or renaming one, and `migration.rs` kept 1,100 lines of tests for upgrades no installed release needs. Their schema also kept less than the code does: SQLite accepted a time written as text, a status no build writes, a failure on a succeeded row, and the plans and progress of a machine the hub had removed, which the store then had to report as corrupt or never read.

Ladderback has one user before 1.0, and breaking changes to stored data are allowed with no shim (the maintainer's policy, 2026-11-25). A baseline now costs one copy of one database; after the first release it would cost every installation an upgrade path.

## The rules

Each is a rule the store's code already keeps, so no write the store makes is refused:

- **`STRICT`** on every table: a column holds only its declared type.
- **`CHECK`s** on `operations.kind`, `trigger` and `status` (the words of `codec.rs`), `proposals.status`, `sign_in_settings.session_length`, `pairing_tokens.state`, `machines.status` and the outbox happenings.
- **A failure belongs to a failed operation:** `error_code` and `error_args` are set exactly when `status` is `failed` (`Encoded::new`, and the recovery that fails a running row writes both). An API token's ID and label are both set or neither.
- **`json_valid`** on `summary_json`, `error_args`, `locations_json` and `proposals.reasons`.
- **Ranges:** every time and count at least 0, a retry's `attempt` and every epoch at least 1, each 0/1 flag 0 or 1, and keys, fingerprints and signatures of their length.
- **Cascades from `machines(id)`** on `sync_plans`, `sync_plan_snapshots`, `sync_progress` and `ownership_reports`: a removed machine's current state goes with it. Its history does not: `sync_reports`, `sync_report_tallies` and `sync_watermarks` stay (decision 154), and so do `repository_owners.holder` and `hub_recovery_floors.holder`, since a lease outlives its holder's removal until it expires (I1), and `pairing_tokens.used_machine_id`. The store already opens every connection with `foreign_keys` on.
- **One report per operation and happening** (#1513): a unique index on `sync_reports (op_id, happening)`. A machine that pairs again under a new ID after its removal resends what its outbox holds; `ingest` now takes the report's place, so the machine moves on, and returns `Ingest::Duplicate` without keeping or counting it.

`synchronous = FULL` (decision 164) is a connection setting, not the schema's, and stays as it is.

## The refusal and the copy

The store reads `_sqlx_migrations` before it runs any migration. When version 1 has the checksum sqlx recorded for the old `0001_operations.sql`, it closes the pool and returns `OpenError::OldSchema` with the last version recorded, having only read. Without this, sqlx would fail on the changed checksum of version 1 with a message about a modified migration, which says nothing of what to do. The daemon and the hub commands say `cli-store-old-schema`: the log is from an earlier version, can't be upgraded in place, and is copied once by the script.

`scripts/copy-to-baseline.ts` refuses any log that does not hold exactly versions 1 to 29, each applied, with the first and last checksums Ladderback shipped. It keeps the old log, write-ahead log included, as `<log>.before-baseline` and refuses to run when that file is there. It builds the new log beside it from the baseline, copying each table with its rowids, and drops only the rows the baseline's rules prove are junk, naming how many of each: a removed machine's current state, which the cascades would have deleted, and a second report of an operation's happening, the first kept, with the tally of a machine that lost one set to what it still holds. Any other row the baseline refuses stops the copy with nothing changed. It then checks the counts, the foreign keys and the file's integrity, removes the old log's `-wal` and `-shm` and renames the new log over it.

## Upgrade (#1621)

On each machine and on the hub, once. The script takes a log at migration 29 only, so a log an older build left (the maintainer's was at 28 on 2026-11-25) first needs one start of the last build before the baseline (`origin/main` before #1618 merges, with #1220), which migrates it to 29; then:

1. Stop Ladderback: `launchctl bootout gui/$(id -u)/io.github.nledford.ladderback` on macOS, `systemctl --user stop ladderback` on Linux, and any `ladderback serve` started by hand.
2. From a checkout of the build with the baseline, run `node scripts/copy-to-baseline.ts "<data folder>/ladderback.db"`, the path quoted (the data folder is `~/Library/Application Support/ladderback` on macOS, `~/.local/share/ladderback` on Linux, or `LADDERBACK_DATA_DIR`). It names what it dropped.
3. Install that build and start the service again with `just install-restart --now`: the service is stopped, so it can't wait for the daemon to be idle.
4. Once the daemon serves, delete `<data folder>/ladderback.db.before-baseline` when it is no longer wanted.

A log the script refuses stays as it was. A build from before the baseline can't open the copied log: sqlx finds version 1 with another checksum (`VersionMismatch`, which the store reports as `OpenError::Migration`), so going back means going back to `ladderback.db.before-baseline`.

## Consequences

- One migration, one pinned checksum in `migrations_pinned.rs`, and no upgrade tests; `crates/adapters/store/tests/migration.rs` is gone. `tests/fixtures/at-0029.sql` keeps one database at 0029 for the refusal and the copy.
- The store's corrupt-row tests write past the `CHECK`s with `PRAGMA ignore_check_constraints`, as a later version that relaxed one could. A value of another type `STRICT` refuses outright, so those cases are gone: the store still reads strictly, but can no longer be handed text where it keeps a number.
- A new kind, trigger or status is a migration that rebuilds `operations`; a new warning code still isn't.
- The SQLite contract harnesses register the machines their suites report as, since a machine's plans and progress now need its row.
- **The script can't tell whether a daemon has the log open.** The upgrade steps stop the service first; a daemon still running could write to the old log after the copy read it, and that write would be lost with the old `-wal`.

## Rejected

- **Upgrading in place with migration 30.** It would keep the 29 migrations and their tests forever, and SQLite can add neither `STRICT` nor a `CHECK` to a table without rebuilding it.
- **Unique machine keys** (`UNIQUE (pubkey)` on `machines`). Pairing doesn't refuse a new machine ID with a key already registered (`decide_pairing`), so the schema would refuse a write the code makes.
- **A 0/1 check on `api_tokens.password` and `sessions.password`.** They are password fingerprints, signed 64-bit integers, not flags.
- **`json_valid` on `proposals.changes`.** It is the change codec's text, which the store never reads.
- **`started_at` and `finished_at` at or after `queued_at`.** The wall clock can step back, so the code can't promise it.
- **`WITHOUT ROWID` tables.** Several reads order by rowid, and `sync_reports` uses `AUTOINCREMENT`.
- **Triggers in place of foreign keys for the cascades.** A foreign key is the rule itself, and `foreign_key_check` can prove a copy keeps it.
