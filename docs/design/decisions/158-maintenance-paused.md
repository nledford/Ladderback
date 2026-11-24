# Decision 158: A paired machine shows when its maintenance paused for want of a lease, and tells it after 14 days

**A paired machine that holds no live lease on a repository it asks to maintain shows since when its maintenance of it is paused, on the repository list and the Snapshots page. It notes each pause in its database every 15 minutes, keeping its date until a grant clears it, and tells `repo.maintenance_paused` through the notification rules once a pause has lasted 14 days, once per pause. The lease cache saves pauses apart from leases and floors.**

## Why

[Decision 151](151-a-paired-machine-maintains-only-under-its-hubs-lease.md) stops a paired machine's maintenance while it holds no lease (I1), and nobody pruning is the safe direction (§7.3). Left silent, that safe stop becomes a quiet failure: a laptop the hub never grants again, or a hub down for good, leaves a repository unpruned and unchecked for months. Issue #1473 asks for the stop to be seen and, if it lasts, told.

- **Since when.** The date a person needs is when maintenance stopped, not when the machine noticed. A lease that ran out gives it: the lease's end on the machine's clock. A machine that never held one, or whose lease the hub let go, has no such date, so the first watch that finds none gives it.
- **Kept, not computed.** A grant that lets a lease go replaces every lease the machine holds (decision 151), so the lease's end is gone with it. A pause is noted in its own table (`agent_maintenance_paused`), so its date outlives the lease, a restart and the hub letting it go.
- **A grant clears it at once.** The page asks whether the machine holds a live lease before it shows a pause, so a grant clears the warning on the next read. The next watch drops the stale pause; a lease that runs out again starts a new one from its end.
- **Only what it asks to maintain.** A pause is noted for a repository whose entry asks `local` and whose ID is known. A repository the configuration stops asking for loses its pause on the next watch. A machine that isn't paired drops every pause; one whose pin can't be read notes nothing, so a damaged pin neither starts nor clears a pause.
- **14 days, once.** A lease lives 7 days (decision 147). Two lease lives keep a laptop away from its hub for a week, or a hub down a few days, quiet. The alert is noted as told only once a rule's channel took it, so a pause that lasts months doesn't repeat it, and one every channel failed to take, likely when a lost lease means a lost network, is told again at the next watch. A new pause, after a grant, can be told again.
- **Every 15 minutes, and never at the first sight.** The watch runs as often as the machine asks its hub for grants (decision 151), and once at start. A pause is told only by a watch after the one that noted it, so a machine that starts or wakes long after its lease ran out asks its hub before it tells, and a grant then ends the pause untold. Before telling, the watch reads the pause again, so a grant taken since it read the leases isn't told either.
- **Two saves.** The lease cache's `save` writes the leases and floors and leaves the pauses; `save_pauses` writes the pauses and leaves the leases and floors. A watch reads the cache, then saves; a grant taken between the two would otherwise be overwritten by the lease the watch read, which the grant had let go or renewed (I1). Each save is one transaction (`BEGIN IMMEDIATE`).
- **An event with no operation.** `repo.maintenance_paused` tells no operation, so an event's operation is optional; a template's `{operation}` writes nothing and a webhook's `operation` is `null`. Its time is since when maintenance is paused, and its ping is `fail`, as a failed prune's is.

## Consequences

- A machine that never held a lease shows its pause from the first watch after it started, so the date can be up to 15 minutes later than the moment it held none.
- A pause is told once. A person who doesn't act on the alert hears nothing more until the pause ends and a new one starts.
- Scheduled maintenance doesn't exist yet (#974): today the pause stops runs asked for by hand, which answer `maintenance-no-lease` while the page warns.
- The page reads the pause when the repository list is read; a pause that starts while the page is open shows on the next read.
