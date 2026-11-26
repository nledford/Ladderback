# Decision 170: A machine upgrades from 0.5.0 by starting 0.6.0

**A machine that ran 0.5.0, or any build after it before the baseline schema, upgrades by installing 0.6.0 and starting it (#1646). On start, its `agent-id` and `agent-key` become `machine-id` and `machine-key`, keeping the ID and the key. Its operation log is copied into the baseline schema when its migrations are exactly 1 to N of the 29 before the baseline (N from 1 to 29), each with the checksum of the file Ladderback shipped. Any other log is still refused and left as it was. This is a one-off upgrade and a deliberate exception to "no shims": both moves, `migrations-before-baseline/` and this decision's code are removed at 1.0.**

## Why

Decision 163 renamed the key files and decision 165 replaced the migrations. Together they left a machine running 0.5.0 (migration 16) with no way to upgrade except by hand. It would have to rename two files, start a build that no release shipped to reach migration 29, and run a Node script from a checkout. The maintainer chose a built-in upgrade instead (2026-11-26): installing the release is the whole upgrade.

## The key files

`platform::agent_id::load`, which `load_or_create` calls, runs the move before it reads the ID. It moves only while `machine-id` is absent and `agent-id` is there.

- The key moves first, then the ID. Each move is a hard link followed by an unlink, so a start that stopped partway leaves a state the next one finishes: `machine-key` with `agent-id`, or one file with two names. Starts at the same time agree. A link that finds the target already there compares inodes. If it is the same file, the move was made beside this one. If `agent-key` sits beside a different `machine-key`, the start is refused, every file is left as it was, and the error names both files and holds neither key (I4).
- A link keeps the file's mode, so the key stays its owner's alone. Nothing reads the key.
- After the move, the files an earlier build staged and a crash left are deleted: `agent-key.*` and `agent-id.*.tmp` (#1621). The folder is synced.
- When `machine-id` and a different `agent-id` are both there, both are left and `machine-id` is the ID. A build after decision 163 chose it, and it may have been used since: the maintainer's laptop chose its new ID on 2026-11-26. The daemon logs one line naming both IDs, never a key.

## The operation log

`SqliteStore::open` reads `_sqlx_migrations` as before. When the log is from the old set, it closes its pool and copies the log, then opens it again and runs the migrations this build ships, as on any log. The copy runs under `<log>.baseline-lock`, an advisory lock from the operating system, which is let go when the process ends however it ends. A second open, such as a `pair` beside the daemon's start, waits for the first and then finds the log in the baseline. The lock file is removed once the log is in the baseline or refused. A failed copy leaves the file, which holds no later start back.

1. **Which logs.** The versions must be exactly 1 to N with N at most 29, each applied successfully, each checksum equal to that of the file in `crates/adapters/store/migrations-before-baseline/`. Those are the files as of `4f2cae86c`, and every tagged release (0.1.0 to 0.5.0) shipped a prefix of them unchanged. A checksum that doesn't match, a gap, a migration that never finished, or one past 29 is `OpenError::OldSchema`, and the log is left byte for byte as it was. `cli-store-old-schema` now says its migrations are not ones a release shipped.
2. **A snapshot.** `PRAGMA wal_checkpoint(TRUNCATE)` must find no other reader, so another program that has the log open stops the copy. Then `VACUUM INTO <log>.before-baseline-new`.
3. **The old migrations.** A copy of the snapshot, `<log>.baseline-source`, gets migrations N+1 to 29 from the embedded old set, as one start of the last build before the baseline would have run them.
4. **The baseline.** `<log>.baseline-new` gets the baseline migration, then every row of the source in one transaction. This is the build `scripts/copy-to-baseline.ts` did, ported: the same tables, the same columns except `operations.machine_id`, and rowids kept. Only junk is dropped: the current state of removed machines, and a second report of a happening (#1513), with the tally updated. Then the counts, `foreign_key_check` and `integrity_check` must agree.
5. **The swap.** The source is removed. Both new files take the old log's permissions and are synced. The snapshot gets the first free name of `<log>.before-baseline`, `<log>.before-baseline-2` and on. It is placed by a link that fails rather than replace an earlier one, which may be the only untouched original. The emptied `-wal` and `-shm` are removed, the new log is renamed over the old one, and the folder is synced.

The old log is never written before the final rename, which replaces it whole. A crash at any step leaves it usable, and the next start removes the work files and copies again. A step that fails does the same now and returns `OpenError::BaselineCopy` with the last migration and the database's or the operating system's words, never a row's values. The daemon, the hub commands and `set-password` say `cli-store-baseline-copy-failed`. The daemon logs a finished copy: the migration it came from, where the old log is kept, and the rows dropped from each table.

`scripts/copy-to-baseline.ts` and its tests are gone. The header of `migrations/0001_baseline.sql` still names the script, because that file's checksum is pinned (`migrations_pinned.rs`) and every baseline database records it.

## Snapshots

Snapshots 0.5.0 took carry the tag `ladderback-agent:<id>`. The release notes give the retag, two `rustic tag` passes per repository, because rustic 0.11 refuses `--add` with `--remove`. Ladderback doesn't retag them itself: that rewrites snapshot files in the person's repositories, which only a command they run should do.

## Consequences

- The copy needs about three times the log's size free beside it, for the snapshot, the source and the new log, until the source is removed.
- A 0.5.0 daemon still running beside a 0.6.0 start, which the service manager prevents, could write to the old log after the snapshot, and that write would be lost. The checkpoint stops the copy only while it has a reader. This is decision 165's limit, unchanged.
- `config.toml` needs no change, since its schema is 1 in both. The `hub-pin` file is not migrated.
- A build from before the baseline can't open the copied log. Going back means going back to `<log>.before-baseline`.

## Rejected

- **Keeping the Node script for 0.5.0.** It took only a log at 29, needed a checkout and Node, and left the key files to the user.
- **Copying only a log at 16 (0.5.0) and at 29.** Every build between them wrote a prefix of the same files, so the old migrations take any prefix to 29 at no extra cost.
- **Refusing a second start while a copy runs.** `pair` opens the log without the data folder's lock, so it waits for the copy instead of failing beside the daemon.
- **Replacing an earlier `.before-baseline` on a retry.** The first one holds the untouched original. A later one could hold a log a failed build had changed since.
