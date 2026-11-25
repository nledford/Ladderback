# Decision 164: The store commits with synchronous = FULL, so a lease survives a crash and, on Linux, a power cut

**The store opens every connection in WAL mode with `synchronous = FULL`, on the hub and on every machine: SQLite puts each commit's log on the disk before the commit returns, so an operating-system crash, and on Linux a power cut, can't take back a grant the hub has sent or a lease a machine has let go of. `NORMAL`, used before, kept the database whole but could lose its last commits. On macOS a power cut still can, since `fsync` there stops at the drive's own cache; flushing that too (`fullfsync`) is left out for its cost** (recorded 2026-11-25, #1622; §7.3, I1).

## Why

Both sides act on a commit as soon as it returns. The hub stores its decisions on a report, then signs and sends the grants (`HubPairing::grant`); a machine lets go of a lease, then sends its `ReleaseAck` (`exchange_leases`). Under `NORMAL` the log reaches the disk only at a checkpoint, or when the operating system writes it, so a power cut can take back a commit after the other side has heard of it.

The defences against a rewound hub (decisions 137 and 157) don't catch a lost grant:

- Recovery mode starts on a floor *above* the hub's counter (`RepositoryOwnership::is_behind`). A hub that lost the grant of epoch N gives N again, and the first holder's floor of N is then *at* the counter, which changes nothing.
- The start window waits, for each repository, only on the agents whose last *stored* report named it, and the report that won the grant is lost with it.

A sequence that breaks I1:

1. A NAS, just approved, asks in its first report to maintain repository R, whose history an old cron job wrote. The hub grants it epoch 1, and the NAS keeps the lease and starts a prune.
2. The power is cut before the hub's commit reaches the disk. The hub starts again with no lease on R, no epoch for it and no report of the NAS's.
3. No stored report of the NAS's names a repository, so the start window doesn't wait on it. Once the machines it waits on have reported, a laptop that also asks to maintain R, and whose listing shows no other agent's snapshots, is granted epoch 1, if its report reaches the hub before the NAS's: the hub knows of no other machine using R. (Were the NAS's first, its floor of 1, above a counter of none, would start recovery mode.)
4. The NAS's next report shows a floor of 1, at the hub's counter: no recovery mode. The NAS loses its lease at that exchange, but its prune runs on, since a lease is checked when a run starts, and the laptop may start its own.

On a machine, a release taken back brings back a lease the hub may already have given to another, which the machine maintains under if the person asks for the repository again before its next exchange; a grant taken back lowers its floor, which decision 137 says never falls.

## Consequences

- One more sync per commit. The busiest writer is the hub keeping each machine's progress, at most once every 5 s per running operation (`PROGRESS_EVERY`). On the maintainer's Mac, the store's 148 tests took 5.21 to 5.25 s with `NORMAL` and 5.24 to 5.28 s with `FULL`, three runs each. That is macOS, where `fsync` doesn't flush the drive; on Linux, where SQLite's `fdatasync` does, the cost is unmeasured, and a flush slow enough to matter would count against the write lock's 5 s wait.
- `every_connection_the_store_opens_waits_for_each_commit_to_reach_the_disk` (`crates/adapters/store/src/pool_tests.rs`) reads `PRAGMA synchronous` on each of the store's connections. It protects I1, not the wiring: the setting is the connection's, not the file's, so nothing else would notice it go.
- **macOS keeps the hole for a power cut** (#1630). `fsync` there leaves the data in the drive's own cache, which a power cut can lose, along with older commits a checkpoint moved into the database file. Any installed machine can become the hub (decision 136), so a Mac mini hub, or a Mac machine with no battery, can still lose a grant or a release that way and break I1 by the sequence above. An operating-system crash is covered on a Mac too.
- **A restored hub keeps the same gap** (#1630). A hub restored from a backup taken just before a grant can give that grant's epoch again, as after a lost commit, and recovery mode still starts only on a floor above the counter. This decision closes only the crash path.

## Rejected

- **Keeping `NORMAL` and relying on the recovery floors:** they catch a hub restored behind a floor, not one that gives the floor's own epoch to another machine.
- **Recovery mode on a floor at the counter held by another machine:** it would hear of the second grant only at the first holder's next report, after both hold the lease.
- **`FULL` on the hub only:** a machine's releases and floors need it too, and one setting for the store is simpler than one per role.
- **`fullfsync` and `checkpoint_fullfsync`, for now.** They would close the macOS hole: SQLite would flush the drive's cache (`F_FULLFSYNC`) at each commit and each checkpoint, and do nothing elsewhere. On the maintainer's Mac they took the store's tests from 5.2 s to 125 s, two of them past their time limit: each commit would cost far more on every Mac, in the daemon and in every developer's test run. The maintainer accepted the macOS gap; #1630 tracks it.
