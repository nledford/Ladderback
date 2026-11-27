# Decision 174: When and where the daemon keeps a daily copy of its database

**While `ladderback serve` runs, it copies its database with `VACUUM INTO` to `ladderback.db.daily` beside it, once a day. The copy's file modification time says when it was taken: a copy is due at once when there is none, a day after the one there is, and at once when that one is dated after now. Each copy is written to `ladderback.db.daily-new`, made empty with the database's permissions first, then synced and renamed over the last. One that can't be taken is a warning in the daemon's log and tried again an hour later. `mcp --headless` and the other commands take none** (recorded 2026-11-27; #1067).

## Why

§9 promised a daily copy, and nothing wrote one. The operation history is the one part of the database nothing else rebuilds: the schedule state comes from the configuration and the snapshot cache from the repositories (§9, "Recovery"). Until `db rebuild` (#580), a person restores the copy by hand, so it has to be one file with a plain name, always whole.

## Decided

1. **Where.** `ladderback.db.daily`, beside the database in the data folder, one copy. The work file is `ladderback.db.daily-new` beside it, as the copy into the baseline schema writes `-new` files beside the log ([decision 170](170-a-machine-upgrades-from-0-5-0-by-starting-0-6-0.md)): a rename within one folder is atomic, and the copy is never on another file system than the database.
2. **When.** A day after the copy there is, by its file's modification time, which the next copy renews. The copy itself is the one fact: a copy that was deleted is taken again at the next check, a database restored from a backup carries no stale date, and no migration adds a column. A copy dated after now, by a clock set back, is taken again at once, or it would stand until the clock caught up. The daemon checks at its start, then sleeps on the system clock until the next copy is due, as the release check does.
3. **Whole or not at all.** The work file is removed if a stopped copy left it, made empty with the database's mode, written by `VACUUM INTO`, synced, renamed over the last copy, and its folder synced. A copy that fails, is dropped at shutdown or is cut by a crash before the rename leaves the copy before it as it was.
4. **As private as the database.** The copy holds what the database holds, the API tokens' and sessions' digests among them and no secret (I4), so it is never more open than the database: the work file has the database's permissions before any row is written to it.
5. **Beside the log's writes.** The copy reads a snapshot through one of the store's connections, so operations are saved while it runs, and those it doesn't hold are in the next. It holds the store as a call does, so the store's close waits for it.
6. **A copy that can't be taken.** A copy the store can't take, or whose date can't be read, is a warning in the daemon's log, in English as its other lines, which says the copy before it is kept and the next try is in an hour. It touches no operation and shows nothing in the web UI.
7. **Only `serve`.** The copy runs with the daemon's background work, beside the scheduler and the release check. A headless MCP session or a one-off command takes none.
8. **One routine for any copy.** The store's copy takes its destination, so the copy before a migration (#1474) is the same routine with another path.

## Considered and rejected

- **A date kept in the database.** It would say a copy was taken when the file was deleted or restored since, and it needs a migration.
- **A copy per calendar day, or at a set hour.** It depends on the machine's time zone, and a daemon started just before the hour would copy twice within minutes.
- **Several dated copies.** Each holds the whole history, so one copy a day kept forever grows without bound; how many to keep is a setting nobody asked for. One copy, never older than a day while the daemon serves, is what `db rebuild` needs.
- **The copy in the cache folder or a temporary folder.** The cache holds nothing Ladderback needs, and a rename across file systems isn't atomic.

## Known limits

- The schedule follows the file system's clock: a copy whose time a sync tool or a restore changed is taken again when that time says.
- A machine that never runs `serve` keeps no copy.
- A copy in flight at shutdown isn't interrupted: SQLite runs its `VACUUM INTO` to the end, and the log's close waits for it within its 20-second bound. A copy stopped before its rename leaves `ladderback.db.daily-new` until the next copy removes it; it has the database's mode.
- A large database's copy holds one of the store's connections while `VACUUM INTO` runs, and its file syncs run on the task the scheduler shares.
- A clock set back while `serve` runs is noticed at the next copy or the next start, not at once: the daemon sleeps until the copy it last took is a day old.
- A folder sync that fails after the rename is told as a failed copy, though the new copy is in place, and the next is a day later.
