# Decision 92: The daemon's stop has a limit

**Told to stop, the daemon waits for each step of its stop within a bound of its own, the bounds add up to its limit of 65 s, and the service manager waits 75 s before it kills it; it stops its workers before it closes the operation log, and records each operation the stop interrupts as failed with `daemon-shut-down`, announcing none; the log says when the stop began and how long each step took** (recorded 2026-11-14, §4.2, §9; amends decision 91; #543, #321).

Before:

- **No limit.** `serve` waited for every open request with axum's graceful shutdown, which has no limit, and then dropped the runtime, which waits for its blocking tasks without one either; the running workers were killed as the runtime dropped them, so a backup that archived never wound down (decision 50).
- **Cut short.** A stop's length depended on what clients were connected, and the service manager's SIGKILL (launchd's `ExitTimeOut`, 20 s by default) could land in the middle of it.
- **Silent.** Nothing in the log said a stop had begun or ended: after #542 the log couldn't tell whether the daemon got the signal or how long it took to go.
- **Announced as a crash.** And every operation a stop interrupted was left running in the log, so the next start ended it `daemon-stopped` and, since decision 91, announced it: a service restart or a computer shutting down in the middle of a backup sent the same `backup.failed` as a crash.

The maintainer decided on #543 that an orderly stop records what it interrupts and announces nothing, and a crash, a kill or a power cut is still announced.

Decided:

1. **The steps and their bounds**, in order, each from what is left of the limit at most:
   - the requests still open: 5 s (`REQUESTS_BOUND`). A request still open then is no longer waited for: it ends with the daemon, as the runtime drops the task that serves its connection;
   - the workers: 40 s (`WORKERS_BOUND`), the grace a stopped worker gets before SIGKILL (35 s, `STOP_GRACE`) and 5 s to reap it.

     Every worker is stopped as a cancel stops one: SIGTERM, and what the worker still writes is read and dropped, so a backup that archives winds down and saves the index of the packs it wrote (decision 50). Unlike a cancel's, the job's events then never end: a run that read an end would record it as the worker's (`engine-exited`), announce it and run its `failed` and `finally` hooks. The run ends with the daemon instead, and the next step records its end.

     A job asked for once this step has begun is neither started nor answered: no worker is left to stop it, and a refusal (`engine-unavailable`) would end the operation, announce it and run its hooks. Its run waits for the daemon to end and holds nothing: not the data folder's lock, which the daemon holds until it exits whatever its tasks do, and no task of the stop;
   - the operation log: 20 s (`LOG_CLOSE_BOUND`, as before).

     `SqliteStore::close_ending_running` waits for the log's calls in flight, so a write a run began commits or fails as it would have, and refuses any call made after it; then, in one transaction (`BEGIN IMMEDIATE`, as recovery's), it ends every operation the log shows as running as failed with `daemon-shut-down`, with the stop's moment as its finish time, and closes the log whatever happened. Nothing is published, so no rule hears of it and no page is told until it reads the history.

     An operation the log shows as queued stays queued, for the next start's persistent queue (decision 73), and so does one a job asked for during the stop queued: the stop interrupted nothing of it. A queued one that start can't run again (a maintenance operation, a backup whose plan is gone) it ends and announces as decision 91 says, since what was asked for never ran.

     When the ends don't commit (the daemon is killed or loses power before the commit, the step is given up and its transaction dropped, or the store refuses the write: read-only, full or corrupt), none is ended, the daemon's log says so at WARN, and the next start ends each `daemon-stopped` and announces it, as after a crash (decision 91);
   - what the runtime's tasks were still doing: what is left. A blocking task still running then (a read of a folder that stopped answering, a keyring prompt nobody answers) is left behind with the process.

   The limit (`shutdown::LIMIT`) is their sum, 65 s, so every step gets its full bound however long the others took.
2. **The service manager waits longer.** The systemd unit sets `TimeoutStopSec=75` and the launchd plist `ExitTimeOut` 75 (`KILL_AFTER`), 10 s past the limit for the process to let go of the data folder's lock and exit. `install-service` and `remove-service` wait 85 s (`STOP_LIMIT`, decision 64) for what waits for that stop: launchd to let an agent go after a `bootout`, and a `systemctl restart` or `disable --now`, which returns only once the unit has stopped; any other call of the manager keeps its 30 s. The order of these numbers is checked: the build checks that the limit is under `KILL_AFTER`, and tests that the grace outlasts the worker's own wind-down (30 s) and that `STOP_LIMIT` outlasts what each service file gives.
3. **The log says it.** At the signal, `stopping (SIGTERM)` or `stopping (SIGINT)`; the number of operations ended `daemon-shut-down`, at INFO, or, at WARN, that they couldn't be and are the next start's; a step given up, at WARN, with what that leaves; and last, `stopped after 2.4 s: open requests 0.0 s, workers 2.3 s, operation log 0.1 s, tasks 0.0 s`, at WARN with `(gave up)` after a step given up. A stop the service manager cut short has no last line.
4. **Every run is frozen where it stands, at the signal** (maintainer, second review round of #589). Before the log says `stopping`, the scheduler is aborted, the workers are asked to stop (`Workers::stop` asks at the call; the step above waits for them) and every run of an operation is frozen (`composition::runs::Runs`).

   From then on it is never polled again and is dropped where it stands (a hook it was running is killed at once, as the runtime's end killed it before), so it starts no worker, runs no further hook, records no end and announces none, whatever its worker read, its deadline timer, the location probes' bound or its turn in the queue do meanwhile.

   A worker a frozen run lets go of still gets its SIGTERM and its grace: the stop was asked first, and the worker's supervisor sees a stop before a job let go of. A backup asked for during the stop is recorded queued and frozen, so it stays queued; a maintenance start frozen answers `operation-failed`. A COMMIT the store had already sent can still complete after a step's future is dropped: all or none either way.
5. **A backup in its after hooks counts as interrupted, and keeps its snapshot.** The run writes the engine's result on the running operation before any ending hook runs, so a backup whose snapshot is written but whose after hooks are still running when the stop comes is recorded `daemon-shut-down` with its snapshot ID on the record, and History links the snapshot; no notification, no further hooks. A crash in its after hooks keeps the snapshot the same way, with `daemon-stopped`.

Rejected:

- one limit shared by the steps, which would let a stalled operation log or a request that never ends take a backup's wind-down from it;
- a limit as short as launchd's default 20 s, which the backup's wind-down alone outlasts;
- closing the log before the workers stop, which left each interrupted operation running for the next start to end `daemon-stopped` and announce, as a crash's;
- announcing `daemon-shut-down`, which tells nobody anything they need to act on, since the stop was asked for;
- reusing `daemon-stopped` with a flag that it was orderly, where the history would show both the same;
- letting a stopped run record the end it read, which would say the worker failed (`engine-exited`) and run its hooks;
- ending the queued operations too, which would lose a "run now" the persistent queue keeps across a restart;
- no finish time, as recovery leaves (decision 40), where an orderly stop knows when it ended them;
- ending each running operation in a transaction of its own, which a kill halfway through would leave half recorded and half announced at the next start;
- `systemctl --no-block` and a wait of our own for the unit, which would need a second way to ask systemd whether the unit has stopped, where the call itself already waits for it.
