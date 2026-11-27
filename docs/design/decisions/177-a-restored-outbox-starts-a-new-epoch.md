# Decision 177: A machine whose database was restored from an older copy starts a new outbox epoch

**A machine's `Hello` says where its outbox stands: its epoch and the place it gives its next report (`outbox: { epoch, next }`), absent before its first report. When the hub already took that place of that epoch, the machine numbers its reports again from an older place, as one whose database was restored from an older copy does, and the hub's signed `Welcome` says `new_epoch`. The machine then gives its outbox a new random epoch, keeping its reports and their places, and sends them under it; the hub keeps both epochs. Neither field changes what another means, so the sync protocol stays 1** (recorded 2026-11-27; §7.2, #1387).

## Why

Each report of a machine's outbox has a place in its epoch, and the hub takes each place of an epoch once ([decision 145](145-the-agents-outbox-and-the-hubs-reports.md)). A database restored from an older copy, as the daily copy ([decision 174](174-the-daily-copy-of-the-database.md)) now makes a normal recovery step, keeps its epoch and its older count, so it gives again places the hub took, and the hub dropped each new report as one it took before. Only reports 32 or more places behind were even told apart, in the hub's log.

## Decided

1. **The handshake says it.** `Hello` carries the outbox's epoch and its next place, `last_seq + 1`. The hub compares it with the last place it took of that epoch: at or below it, the hub took places this outbox gives again, and `Welcome` carries `new_epoch: true`. Past it, of an epoch the hub doesn't hold, or with no position at all, `Welcome` says nothing new. The rule is `domain::fleet::starts_new_epoch`; nothing else, the hostname or the clock, decides it.
2. **One step on the machine.** Before it sends anything on that connection, the machine replaces the epoch it stated with a new random one in `sync_outbox_meta`, in one `UPDATE`, and only if the outbox still holds that epoch. Its places, the reports still waiting and its count stay as they are, so no migration is needed. A machine that stops before the step states the old epoch again at its next connection and is told again; one that stops after it states the new epoch, which the hub doesn't hold, and goes on. Both sides log a warning; a failed step ends the session with nothing sent.
3. **What "sends them again" means.** The reports the hub acknowledged left the outbox, under either epoch, and are never sent again. The ones still waiting are sent under the new epoch, the ones the hub took after the copy among them: the hub holds each operation's start and end once, whichever epoch or machine ID brings it (`sync_reports_once`, #1513), so it acknowledges those as duplicates and keeps the rest. The new epoch's places start where the outbox's count stood, which the hub takes as a gap.
4. **What the hub keeps.** Nothing new: the last place of each of a machine's 8 newest epochs (`sync_watermarks`) and its reports. Each restore adds one epoch.
5. **No new protocol.** §7.3 asks for one when a field's meaning changes. None does here: `outbox` and `new_epoch` are optional fields an older peer ignores, as `zone` and `machine` were, and a signature covers a payload's bytes, so an older reader verifies them.

## Compatibility with 0.6.0

| Machine | Hub | What happens |
|---|---|---|
| This release | This release | A restored machine starts a new epoch, and the hub keeps every report it sends after |
| This release | 0.6.0 | The hub ignores `outbox` and says nothing of an epoch: as today, the hub drops the restored machine's reports at places it took, and logs those 32 or more behind |
| 0.6.0 | This release | Its `Hello` has no `outbox`: as today |
| 0.6.0 | 0.6.0 | As today |

A `Hello` whose `outbox` is out of shape (an epoch that isn't a UUID, a place of 0) is refused, as one with a fingerprint out of shape is. `new_epoch` is a boolean, absent when false.

## Consequences

- Two computers under one machine ID, a copied data folder the hub can't tell apart (no fingerprint; decisions 144 and 150), share one epoch. The one that connects with a next place the other already gave starts an epoch of its own, after which the hub keeps both computers' reports; until then, a report at a place the other gave first is still dropped. Decision 150 refuses the second computer whenever the hub can tell.
- A restore whose copy is newer than what the hub took changes nothing: the machine's next place is past the hub's.
