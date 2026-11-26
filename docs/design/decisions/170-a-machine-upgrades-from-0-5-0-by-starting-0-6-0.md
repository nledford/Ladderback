# Decision 170: A machine upgrades from 0.5.0 by starting 0.6.0

**A machine that ran 0.5.0, or any build after it before the baseline schema, upgrades by installing 0.6.0 and starting it (#1646). On start, its `agent-id` and `agent-key` become `machine-id` and `machine-key`, keeping the ID and the key. Its operation log is copied into the baseline schema when its migrations are exactly 1 to N of the 29 before the baseline (N from 1 to 29), each with the checksum of the file Ladderback shipped. Any other log is still refused and left as it was. Only a process that holds the data folder's lock (`ladderback.lock`) upgrades the folder; a command that runs beside the daemon takes the lock for it when it is free and otherwise changes nothing and is refused. This is a one-off upgrade and a deliberate exception to "no shims": both moves, `migrations-before-baseline/` and this decision's code are removed at 1.0.**

## Why

Decision 163 renamed the key files and decision 165 replaced the migrations. Together they left a machine running 0.5.0 (migration 16) with no way to upgrade except by hand. It would have to rename two files, start a build that no release shipped to reach migration 29, and run a Node script from a checkout. The maintainer chose a built-in upgrade instead (2026-11-26): installing the release is the whole upgrade.

## Who upgrades

0.5.0 takes the same `ladderback.lock` advisory lock on the data folder that 0.6.0 does, so a process that holds it knows no other Ladderback, of either version, has the key files or the log open. The upgrade, `composition::upgrade`, runs only there:

- The daemon, `set-password`, `pairing-token` and `unpair` already take the lock before they open the log or read the key. Each upgrades the folder right after (`upgrade::under`). The daemon logs a finished copy before it opens the log, so a later refusal doesn't lose it.
- `pair`, `hub-status` and `export-profile` run beside the daemon without the lock. Each first looks, without changing anything, whether the folder needs the upgrade (`agent_id::earlier_names_left`, `SqliteStore::needs_copy`). Nearly always it doesn't, and the command goes on as before. When it does, the command takes the lock: if it is free, the command upgrades the folder, lets the lock go and goes on; if another Ladderback holds it, the command is refused with `cli-upgrade-ladderback-running` and every file is left as it was. That Ladderback is 0.5.0's daemon not yet stopped, or a 0.6.0 one starting, which upgrades the folder itself.
- Nothing else moves or copies: `agent_id::load` and `SqliteStore::open` only read. `open` refuses a log still from the old set with `OpenError::BeforeBaseline` (`cli-store-before-baseline`), and `load_or_create` refuses to make a new ID beside an `agent-id` not yet moved, a net under the rules above that a correct caller never reaches.

## The key files

`platform::agent_id::move_earlier_names`, which needs a held `DataDirLock`, moves the files. It moves only while `machine-id` is absent and `agent-id` is there.

- The key moves first, then the ID. Each move is a hard link, a sync of the folder, then an unlink, so a crash at any point leaves the file under one name or both, never neither, and the next start finishes: `machine-key` with `agent-id`, or one file with two names. A link that finds the target already there compares inodes. If it is the same file, an earlier move stopped between its link and its unlink. If `agent-key` sits beside a different `machine-key`, the upgrade is refused with `MoveError::TwoKeys` (`cli-upgrade-two-keys`), which names both files and holds neither key (I4), and every file is left as it was.
- A link keeps the file's mode, so the key stays its owner's alone. Nothing reads the key.
- After the move, and on any later start that finds them, the files an earlier build staged and a crash left are deleted: `agent-key.*.tmp` and `agent-id.*.tmp` (#1621). Any other name beside the key, such as an `agent-key.bak` the person keeps, is left. The folder is synced.
- When `machine-id` and a different `agent-id` are both there, both are left and `machine-id` is the ID. A build after decision 163 chose it, and it may have been used since: the maintainer's laptop chose its new ID on 2026-11-26. The daemon logs one line naming both IDs, never a key.

## The operation log

`SqliteStore::needs_copy` reads `_sqlx_migrations` alone. `SqliteStore::copy_into_baseline`, which `upgrade` calls with the lock held, copies a log from the old set; the open that follows runs the migrations this build ships, as on any log. A copy that finds no log, or a log already in the baseline, does nothing.

1. **Which logs.** The versions must be exactly 1 to N with N at most 29, each applied successfully, each checksum equal to that of the file in `crates/adapters/store/migrations-before-baseline/`. Those are the files as of `4f2cae86c`, and every tagged release (0.1.0 to 0.5.0) shipped a prefix of them unchanged. A checksum that doesn't match, a gap, a migration that never finished, or one past 29 is `OpenError::OldSchema`, and the log is left byte for byte as it was. `cli-store-old-schema` now says its migrations are not ones a release shipped.
2. **A snapshot.** `PRAGMA wal_checkpoint(TRUNCATE)` must find no other reader, so another program that has the log open stops the copy with `OpenError::LogInUse` (`cli-store-baseline-log-in-use`). Then `VACUUM INTO <log>.before-baseline-new`.
3. **The old migrations.** A copy of the snapshot, `<log>.baseline-source`, gets migrations N+1 to 29 from the embedded old set, as one start of the last build before the baseline would have run them.
4. **The baseline.** `<log>.baseline-new` gets the baseline migration, then every row of the source in one transaction. This is the build `scripts/copy-to-baseline.ts` did, ported: the same tables, the same columns except `operations.machine_id`, and rowids kept. Only junk is dropped: the current state of removed machines, and a second report of a happening (#1513), with the tally updated. Then the counts, `foreign_key_check` and `integrity_check` must agree.
5. **The swap.** The source is removed. Both new files are synced. The snapshot gets the first free name of `<log>.before-baseline`, `<log>.before-baseline-2` and on. It is placed by a link that fails rather than replace an earlier one, which may be the only untouched original; the folder is synced before the work name is dropped. The emptied `-wal` and `-shm` are removed, the folder is synced again so the kept copy's name is durable, the new log is renamed over the old one, and the folder is synced. Once the rename is made the copy is done: a failure of that last sync is `BaselineCopy::unsynced`, a warning in the daemon's log, never a refusal saying the old log was left as it was.

Before step 2, the three work files are made empty with the old log's permissions (`VACUUM INTO` takes an empty file, and SQLite gives a database's journals its permissions), so none of them is ever more open than the log whose rows, sessions and API tokens' fingerprints among them, it holds.

The old log is never written before the final rename, which replaces it whole. A crash at any step leaves it usable, and the next copy removes the work files and copies again. A step that fails does the same now and returns `OpenError::BaselineCopy` with the last migration and the database's or the operating system's words, never a row's values. Every command that upgrades says `cli-store-baseline-copy-failed`. The daemon logs a finished copy: the migration it came from, where the old log is kept, and the rows dropped from each table.

`scripts/copy-to-baseline.ts` and its tests are gone. The header of `migrations/0001_baseline.sql` still names the script, because that file's checksum is pinned (`migrations_pinned.rs`) and every baseline database records it.

## Snapshots

Snapshots 0.5.0 took carry the tag `ladderback-agent:<id>`. The release notes give the retag, two `rustic tag` passes per repository, because rustic 0.11 refuses `--add` with `--remove`. Ladderback doesn't retag them itself: that rewrites snapshot files in the person's repositories, which only a command they run should do.

## Consequences

- The copy needs about three times the log's size free beside it, for the snapshot, the source and the new log, until the source is removed.
- A 0.5.0 daemon still running holds the data folder's lock, so a 0.6.0 start beside it is refused as any second daemon is, and a command beside it is refused with `cli-upgrade-ladderback-running`. Neither copies a log the old daemon could still write, nor moves a key it could still read. A program other than Ladderback that has the log open stops the copy at the checkpoint only while it reads.
- A connection a command beside the copy had opened on the old log before the lock was taken closes without touching the new one: SQLite finds the file it opened renamed over and leaves the `-wal` and `-shm` beside the path alone.
- `config.toml` needs no change, since its schema is 1 in both. The `hub-pin` file is not migrated.
- A build from before the baseline can't open the copied log. Going back means going back to `<log>.before-baseline`.

## Rejected

- **Keeping the Node script for 0.5.0.** It took only a log at 29, needed a checkout and Node, and left the key files to the user.
- **Copying only a log at 16 (0.5.0) and at 29.** Every build between them wrote a prefix of the same files, so the old migrations take any prefix to 29 at no extra cost.
- **Copying on open, under a lock of the log's own.** The first version did, so that `pair` waited for the daemon's copy instead of failing. But `pair` and `hub-status` don't take the data folder's lock, so beside a 0.5.0 daemon not yet stopped they moved the key files and copied the log under it, losing what it wrote after the snapshot. The data folder's lock is the one both versions take.
- **Leaving an earlier folder as it is for a command beside the daemon.** It would have to read the ID under both names and the log in both schemas, every such command, until 1.0. Refusing while the folder is held costs one stop of the old daemon, which the upgrade needs anyway.
- **Replacing an earlier `.before-baseline` on a retry.** The first one holds the untouched original. A later one could hold a log a failed build had changed since.
