# Decision 176: How the hub runs and cancels a machine's backups

**A person signed in on the hub's page can ask a connected machine to run one of its plans now, or to cancel a backup it runs. The hub sends a signed `RunPlan` or `CancelOp` on the machine's connection and waits at most 20 seconds for a `CommandResult`. The machine acts on a command only when its configuration allows the hub's commands, the hub asked within 15 minutes of the hub's time, and the command's ID is new to it; it runs a plan through the same use case as Back up now. The hub sends nothing to a machine that isn't connected and keeps nothing to send later. Only a session asks, never an API token; the machine learns which session by a tag, never a secret (I4)** (recorded 2026-11-27, §7.2, §10.3; #1464, which folds in #576).

## Why

The dashboard's Back up menu (#1461) needs the hub to start a backup on a machine, and §7.2 reserved `RunPlan` and `CancelOp` for it from the first protocol: signed, with a command ID, the time requested and a 15-minute expiry, never run twice. This decision builds them and says who may ask, what a hostile hub can do with them, and what an older machine does.

## Decided

1. **What a machine takes, said once.** `Hello` gains `accepts`, the names of the commands the machine takes (`run-plan`, `cancel-op`), at most 16; a name it doesn't know is passed over, so a later command (restore, fetch log) extends the list without a new protocol. The hub keeps the list while the machine is online, and its page reads it as `accepts` on each machine of `GET /api/v1/hub/dashboard`, so it offers only what the machine takes.
2. **The routes.** `POST /api/v1/hub/machines/{id}/runs` with `{ "plan": … }` answers `202 Accepted` with `{ "operation": … }`, the backup that runs the plan. `POST /api/v1/hub/machines/{id}/operations/{op}/cancel` answers `200` with the backup canceled. A refusal is an error code: the hub's own, or the machine's, which the hub reads by its name and passes on only when a machine refuses that kind of command with it and gives what its message reads (a rate limit's `retry-at`). Any other answer is the kind's own refusal: `machine-run-refused` for a run, `machine-cancel-refused` for a cancel.
3. **One code path.** The machine starts a hub's run through the plan runner that Back up now uses, so the rules (a plan it has, one run of a plan at a time, the key files left out) hold in one place. A plan whose backup is already running or waiting answers with that backup's operation, so pressing twice is harmless. A cancel reaches only a backup (`command-backups-only`); an operation the machine doesn't know, or that has ended, is refused with `operation-unknown` or `operation-not-running`.
4. **The trigger.** The operation's trigger is `hub`, with the tag of the session that asked (`Trigger::Hub { asked_by }`), kept in the operation log's `hub_session` column (§9), so the machine's own history says that the hub started it and which session asked. The retry rule treats it as a run a person asked for, with no automatic retry.
5. **Not connected.** A machine offline is refused at once: `machine-offline-next-run`, naming the plan's next scheduled run, when the machine's last report had one still to come, else `machine-offline`. Nothing is queued: a backup never starts hours after it was asked for.
6. **An answer for every command.** The hub waits at most 20 seconds; a machine that doesn't answer in time, or whose connection ends first, is `machine-no-answer`, which says that whether it acted isn't known. The hub waits on at most 8 commands to one machine at once; a ninth is `command-rate-limited` with when to ask again.

## Threat model

**Who may ask.** A signed-in session, through the page: the routes carry the session's cookie and its CSRF token, and the request checks refuse another site's `Origin` (`origin-not-allowed`) before sign-in is looked at. No API token permission opens them (decision 127's table names neither route), so a script, an MCP client or a token that leaked can't start a machine's backup. With sign-in off, only the hub's own computer may ask: from beyond loopback the hub refuses with `pairing-needs-sign-in`, as for pairing (decision 120). No step-up is asked, as for a local Back up now: nothing is changed or deleted.

**What crosses (I4).** A command carries the plan's ID or the operation's ID, its own ID, the hub's time when it was asked and the session's tag, never a plan's contents, a path or a secret. The tag (`SessionTag`) is the first 8 bytes of the SHA-256 of a fixed label and the session's digest, as 16 hex digits: it tells one session from another and gives back neither the session's token nor its digest, which already is a hash of the token. A machine's answer carries an operation ID or an error code.

**Expiry and replay.** A command is signed on its connection with the sequence number every message carries (decision 120), so a proxy can't forge or replay it there. Across connections, the machine acts on a command only when its `requested_at` lies within 15 minutes of the hub's time either way, both when the command arrives and when the machine acts on it, judged as the `Welcome`'s `signed_at` plus the machine's monotonic time since, not the machine's wall clock. It remembers each command's ID for 24 hours, far past the 15 minutes, in its database (`hub_commands`, §9), so a replay after a restart, or from a hub whose clock was set back, is `command-replayed`. A command it can't remember, as when its database can't be written, runs nothing.

**Held commands.** A command that arrives while the machine waits for another message is held and answered after that exchange, and between the windows of a replay of its outbox, so none waits behind a long backlog. The machine holds at most 8 unanswered: one more makes it drop the oldest, which the hub has stopped waiting for, and answer that one `command-expired`, keeping the connection.

**The rate limit.** The worst a compromised hub can do with these commands is ask for extra backups of plans the machine already has, and cancel its backups. The machine bounds the first: one backup of a plan the hub started every 5 minutes, and 12 an hour, counted in its database (`hub_runs`, §9) on its own clock, a run a clock set back puts ahead counted by how far it lies; past that it answers `command-rate-limited` with when to ask again. No command touches maintenance (I1 stays with leases), a plan's contents or a restore (§10.4).

**The ledger's bound.** The machine remembers every command it takes before acting on it, a run it refuses too, so a replay can never act. It remembers at most 1,000 command IDs, refusing more rather than recording them: that bounds its database, not a hostile hub, which can fill the 1,000 and have the machine refuse its hub's commands for up to 24 hours, while schedules and the machine's own Back up now go on. The hub sends one machine at most 40 commands an hour, refusing more with `command-rate-limited`, so an honest hub's asks never fill it.

**The off switch.** `[agent] allow-hub-commands = false` (§6.1; on by default) turns the commands off: the machine's next `Hello` says it takes none, so the hub answers `machine-commands-off` without sending anything, and a command that still arrives is refused the same way. The machine reads the setting for every command, and a configuration it can't read counts as off. A person who doesn't want the hub to start backups on a machine turns it off there; the hub can't turn it back on.

**Older machines.** A machine whose `Hello` has no `accepts`, a release before this one, takes no commands: the hub answers `machine-commands-unsupported` and sends nothing. The new fields are optional within protocol 1 (decision 88), so an older hub never sends a command and a newer machine runs as before.

## Considered and rejected

- **Queuing a command for a machine offline.** A backup that starts hours after it was asked for surprises the person and may run on a battery or a metered link; the 15-minute life rules it out anyway.
- **Remembering a command's ID for 15 minutes only,** as #1464 first said. A hub whose clock was set back could have an old command taken again; 24 hours costs a few rows.
- **The session's token, or its digest, as who asked.** Either would let a machine, or anyone reading its database, act as the session; a tag can't.
- **A step-up, as for a restore.** Nothing is changed or deleted, and a local Back up now asks none.

## Known limits

- The hub compares the plan's next run, as the machine last reported it, with its own clock, so a skewed clock can turn `machine-offline-next-run` into `machine-offline`.
- The offline refusal is built for a run only: should a cancel ever be answered with `machine-offline-next-run`, its `plan` argument would be empty.
- That nothing waits for a machine that isn't connected is tested by nothing arriving later, which can't prove a negative; the hub keeps no queue to look in.
- The machine tells its hub what it takes only in `Hello`, so turning `allow-hub-commands` back on takes effect at its next connection: until then the hub answers `machine-commands-off` without sending anything. Turning it off takes effect at once, since the machine reads the setting for every command.
- A rate limit's `retry-at` is on the machine's clock, which the hub passes on as it is, so a skewed machine clock moves when the person is told to ask again. A cancel refused because the ledger is full reads as `command-rate-limited`, the code a run's limit gives.
- A run counts toward the machine's limits only when recording it succeeds. The machine records the run in `hub_runs` after the backup has started, so should that write fail, just after the command's own write to the same file succeeded, the backup runs uncounted and the machine logs the error.
- The hub's idle bound on a connection also ends when it sends a command, so asks made within that bound of each other keep a silent machine's connection marked online.
