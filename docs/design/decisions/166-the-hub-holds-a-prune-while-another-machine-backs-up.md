# Decision 166: Does a prune wait while another machine backs up to its repository?

**Yes, on a paired machine: its hub holds the prune while another machine it holds a connection of is backing up to the repository, or while it can't tell what such a machine runs, and the machine refuses a held prune with `maintenance-prune-held` before anything is stored. Decided 2026-11-25 for #573 (slice 2), on [decision 159](159-long-lived-hub-connections.md)'s heartbeat and beside [decision 161](161-a-prune-asks-before-it-deletes.md)'s re-check before deletion.**

## Context

§7.3's layer 2 of I3 says that the owner prunes only once the backups that may reference the packs it deletes have finished. Before this, a paired owner pruned whenever it held its lease, and only layer 1 (each backup's deadline) protected another machine's long backup. The heartbeat every machine sends every 30 seconds was `{}`.

## Decision

- **Wire.** `Heartbeat { running_ops?: [{ op_id, kind, repo_id? }] }` names the backups the machine runs (`kind` is `backup`; other kinds are passed over), with `repo_id` once the machine knows it. A machine that runs more than 16 backups leaves the list out. `HeartbeatAck { prune_held?: [repo_id] }` names the repositories whose prune the hub holds for the machine that sent the heartbeat. Each list holds at most 16 entries, so the largest of each fits one 4096-byte frame. A longer list, or an ID that isn't one, refuses the message.
- **Hub.** The hub keeps in memory what each machine's latest heartbeat said. A machine's entry goes when its connection goes offline (90 seconds of silence, decision 159) or when the machine is removed. The hub holds a prune of repository R for machine M while another machine it holds a connection of is backing up to R. It holds every prune while it can't tell: a machine online that hasn't named what it runs (an older build, or none sent yet on this connection), a backup with no repository ID, or more than 16 repositories. A machine offline holds nothing. The answer isn't filtered by the repositories M uses: all of a hub's machines belong to one person (decision 99), and filtering would need a registry read on every heartbeat.
- **Machine.** A paired machine keeps the hub's latest answer, with when it came, in memory. It holds a prune whenever the answer holds the repository, the answer didn't say, no answer came in the last 90 seconds on its wall clock or its monotonic clock, the wall clock went back past the answer, the repository's ID isn't known, or the pin can't be read. A machine that isn't paired is never held.
- **Where it is checked.** The check runs when the prune is assessed, and again when it starts, under the configuration lock and after the lease check. A held prune is refused with `maintenance-prune-held` (409, argument `alias`) before anything is stored. The person starts it again once the backup has finished. The check is made at the start only: a backup that starts while the prune runs is the next prune's to wait for, as what this prune marks is deleted no sooner than `keep-delete` later (I2). The deletion re-check (decision 161) is unchanged.
- **Running backups.** Each backup takes its place in an in-memory list (`app::running::RunningBackups`) from before its engine is asked until its run ends. It gets the repository from its entry, or else from the engine's `Started` event. Event-bus news isn't used, because it can be dropped (#1437).
- **No new protocol number.** §7.3 asks for one when the meaning of `running_ops` changes. These fields are new, and their absence holds every prune on both sides: an older machine's `{}` holds other machines' prunes, and an older hub's `{}` answer holds every prune on the machine. So no older peer can act on a meaning it doesn't know (decision 88's concern).
- **Nothing stored.** The hold lives in memory only; there is no migration.

## Consequences and known limits

- Frequent or overlapping backups to a repository can keep its prune held. The §7.3 filter on backups that started reading the index before the previous prune finished isn't built, as it would compare clocks across machines. Layer 1, the deadline, stays the guarantee.
- A hub that restarts holds every prune until each online machine's next heartbeat, at most 30 seconds.
- The hub's own backups and its own maintenance (hub ownership, #1445, M9) aren't covered. A machine that isn't paired has no other machine to wait for.
- Each machine learns which of the hub's repositories other machines are backing up to now. A machine with a valid key can hold every prune by never naming what it runs; it is one of the person's own machines (decision 99), and removing it ends the hold.
- Backups made outside Ladderback aren't seen, as before.
- The owner's own running backups don't hold its prune, and a machine offline holds nothing, though §7.3 planned both. Layer 1 covers them.
- A hub that hears a late heartbeat from a machine's replaced connection keeps it until the next heartbeat on the new one, up to 30 seconds. The hub keys what it heard by machine, not by connection.
- A machine's ack that it can't read holds every prune and keeps the connection. A hub refuses a heartbeat with more than 16 backups, passing over other kinds unread.
