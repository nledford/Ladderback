# Decision 157: The hub's start window waits on every agent whose last report named a repository, for 7 days and an hour on its monotonic clock

**After any start, the hub refuses a claim on a repository with no lease binding until every agent whose last report named it, however long ago, has reported since the start, or until 7 days and an hour have passed since the start on the hub's monotonic clock. A repository no agent's last report named is held until every agent whose last report named any has reported. Its holder's lease is renewed all the while. The hub reads who used each repository at the first report after the start, and an agent's report counts as its heartbeat once it is stored.**

## Why

[Decision 137](137-a-hub-moves-by-handover-never-by-copy.md) holds every repository from a new owner after a start until every machine that used it has sent a heartbeat, or 7 days have passed. It left open how the hub knows who used a repository, how it counts the 7 days, and what a hub clock that steps does to them. #1444 adds the case of a hub with no battery-backed clock, which may start with a wrong time and step forward once it syncs.

- **The monotonic clock counts the window.** A hub that starts with its clock behind and then steps forward by days would end a window counted on the wall clock at once, and grant a repository whose lease another machine still holds. The `Clock` port's monotonic time doesn't move when the wall clock is set, so the window lasts its full length. Time the hub spends asleep doesn't count, which only makes the window longer, the safe direction.
- **7 days and an hour, not 7 days.** A lease renewed just before the start runs 7 days, and the hub counts it an hour past its expiry before it grants the repository to another (decision 147), so a holder whose view of the hub's time drifted has stopped. The window waits the same.
- **Every last report counts, however old.** A report counts as using the repository for 7 days on the wall clock (decision 147), and a wall clock that is wrong at the start could make a recent report look stale. Counting every agent whose last report named the repository waits on too many rather than too few.
- **A repository nobody had reported waits on everyone who had.** A hub restored from a backup doesn't know which repositories its agents began using after the backup, nor which of them holds one. Every agent that had reported any repository may, so the hub waits on all of them.
- **The list is read at the first report after the start.** Until an agent reports, nothing is decided, so the list read then is the registry as it stood at the start. Kept in memory, it isn't shortened by a machine removed meanwhile, as decision 137 asks.
- **A report counts once it is stored.** An agent's `ReposSnapshot` names what it uses; one that no longer names the repository has stopped using it. Its own report is decided as if it had been heard, but the window stops waiting for it only once the report is stored, so a report that fails or is dropped ends nothing. A release alone isn't a report.
- **Only a claim with no lease binding waits.** The holder's renewal and a claim refused because another holds the lease are decided as before, so nothing a running holder does changes.

## Consequences

- A repository once used by a machine that is gone can't move to a new owner for 7 days and an hour after each start of the hub, and a new repository waits the same for every such machine. Nothing yet removes a machine's reports from the hub; removing a machine (#1492) will. Its holder, if any, keeps renewing, and the refusal is silent.
- A machine paired after the backup a hub was restored from, or one that had reported nothing before it, isn't waited for (decision 137, "What it costs").
- A hub restored behind a machine's epoch floor enters recovery mode when that floor is proved, and grants no leases until a person confirms the old hub is stopped or gone. Ending recovery raises each affected repository's counter above its floor.
- The start window and recovery mode are separate: the window still protects every repository's first owner change after a start, while recovery mode protects repositories whose floors show that this hub was rewound.
- A hub clock that steps forward after the window has ended is still decision 147's limit.

## Rejected

- **The wall clock for the window:** a clock that steps forward ends it early, the case #1444 names.
- **Waiting until the hub's clock is known good:** nothing tells the hub its clock is good. The window's end on the monotonic clock and the reports of every machine cover what that would protect.
- **Leaving out reports older than a report's life:** a backup's old report says nothing about what the machine did after it, so a filter would let exactly the restored case through.
- **Keeping the window in the database:** a restart starts a new window anyway, and a restore can't tell it was restored.
