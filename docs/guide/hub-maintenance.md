# Maintenance with a hub

Maintaining a repository means forgetting the snapshots its plans no longer keep, pruning the data no snapshot uses, checking it, and adopting history into a plan. Exactly one machine may maintain a repository, so that two never delete from it at once. Without a hub, you keep it to one: a machine maintains a repository when you tick **Maintain this repository from this machine** as you connect it, or press **Maintain from this machine** for it on the **Snapshots** page, which sets `maintenance = "local"` in its entry in `config.toml`. Once the machine is paired with a hub, the hub makes sure of it too.

The MVP's release is the first with hubs ([The hub](hub.md)).

## Leases

A paired machine maintains a repository it asks to maintain only while it holds the hub's **lease** on it. The machine asks with each report of the repositories it uses, about every 15 minutes while it can reach the hub. The hub grants the first machine that asks a lease of 7 days, and renews it each time that machine asks again. Only one machine holds a repository's lease at a time: the hub refuses every other while it holds. A machine waiting for approval is granted nothing.

A lease runs out on the hub's clock, never the machine's, so a machine whose clock is wrong still stops in time. When its holder stops asking, because it is off, removed from the hub or can't reach it, its lease runs out 7 days after it was last renewed, and an hour later the hub may grant the repository to another machine that asks. A machine that stops maintaining a repository, with **Stop maintaining from this machine**, gives its lease back, so another can have it at once.

## A repository more than one machine uses

The hub grants a lease only on a repository no other machine uses. When a second approved machine reports the same repository, or the repository holds a snapshot another machine made in the last 30 days, the hub grants it to no machine. A machine that held its lease before keeps it until it runs out, unrenewed; after that, no machine maintains the repository.

To tell, a machine lists each repository it asks to maintain at most once a day, before it reports to the hub; until a listing succeeds, the hub counts the repository as used by another machine. Snapshots the machine's earlier installation made, under another machine ID, count as another machine's until you adopt them into one of its plans.

## When maintenance is paused

A paired machine that holds no lease on a repository it asks to maintain forgets, prunes, checks and adopts nothing there; its backups run as usual. The **Repositories** page says **Maintained from this machine, but paused: it holds no lease on this repository from its hub.**, and the **Snapshots** page says since when. **Forget snapshots** and the other maintenance buttons are refused with the code `maintenance-no-lease`. Maintenance starts again once the hub grants a lease.

The pause shows from the moment you turn maintenance on with **Turn on maintenance** for a repository the hub doesn't lease to this machine, for example one it leases to another machine, and the **Snapshots** page warns of it at once. Turning maintenance off ends the pause. A repository whose ID was never recorded shows no pause, though its maintenance is refused all the same.

Once maintenance of a repository has been paused for 14 days, the machine sends the notification event `repo.maintenance_paused` once, to each rule that names it, so a machine that lost its hub doesn't go unnoticed.

`maintenance = "hub"` in a repository's entry leaves it to the hub, and the Repositories page says **Left to the hub to maintain.** The hub doesn't maintain repositories itself yet, so nothing maintains such a repository.

## When a prune waits for another machine's backup

A prune deletes data that a backup still running elsewhere may be about to use. So on a paired machine, the hub holds a repository's prune while another machine connected to it is backing up to that repository, and the machine refuses the prune before changing anything, with the code `maintenance-prune-held`. For a repository named `demo`, it says: **Another machine may be backing up to the repository demo, so the hub holds its prune, and nothing was changed.** Start the prune again with **Prune unused data** once that backup has finished; a held prune isn't queued.

The hub learns what each machine runs from the heartbeat it sends every 30 seconds. When it can't tell, it holds the prune rather than guess: for up to 30 seconds after the hub starts, while a machine runs more than 16 backups at once, and when this machine hasn't heard from its hub in the last 90 seconds. Update the hub and its machines together: a hub from before this check holds every prune.

## After the hub restarts

After any start, the hub gives no repository to another machine until every machine that used it, and every machine approved before the start, has reported since, or 7 days and an hour have passed. The machine that holds the lease keeps it, renewed as before. A machine that was off when the hub restarted therefore doesn't lose its repositories, but while one stays off, no repository gets a new maintainer until those 7 days and an hour are up.

A hub restored from an older backup of its data folder may be shown, by a machine, a lease it granted after that backup. It then grants and renews no lease, and its **Machines** tab says why, until you confirm that the hub that granted it is stopped or gone for good: press **End recovery mode**, and confirm as for a pairing token. Do that only once the old hub can't run again, since two hubs that both grant leases could each give one repository to a different machine.

## Known limits

- **A repository that two machines back up to is maintained by neither.** Making the hub its maintenance owner comes later ([README, roadmap](https://github.com/nledford/Ladderback/blob/main/README.md#roadmap), M9).
- **Forget and prune don't run on a schedule yet**, with a hub or without one ([Getting started](getting-started.md#known-limits)): a machine that holds the lease runs them when you press their buttons.
- **A machine that lost its hub maintains nothing** once its leases run out, until it leaves that hub with `ladderback unpair` ([Leaving a hub](hub.md#leaving-a-hub)).
