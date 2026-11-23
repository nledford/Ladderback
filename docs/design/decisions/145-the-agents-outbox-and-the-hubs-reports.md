# Decision 145: The agent's outbox and the hub's reports are bounded, and each says what it gives up

**An agent keeps each operation's start and end in a persistent outbox of at most 10,000 reports and replays them to its hub in order, at most 32 places ahead of the hub's acknowledgements, on a connection per burst; a failed one is tried again after 1 s, doubled up to 5 minutes. Full, the outbox gives up its oldest start for a new report and refuses one only when it holds 10,000 ends. The hub takes a report once, keeps the last place of each agent's 8 newest epochs and at most 10,000 reports of each agent, and lets go of the oldest past that. Every report given up, refused or let go of is said: a warning in the agent's log for each, and on the hub a count per agent and one warning per agent while it runs** (recorded 2026-11-22, §7.2, #568).

## Why

§7.2 set the outbox's shape, an epoch and a place per report, replayed from the hub's last acknowledgement, and left its bounds open. Both sides need them: an agent whose hub is gone for months, and a hub that keeps the reports of many agents, would otherwise grow their databases without end.

- **What a full outbox gives up.** A start says less than an end: the end says how the operation came out, and the hub can show an operation it knows only by its end. So a full outbox gives up its oldest start first, and keeps every end it can. One full of ends refuses the new report rather than lose an outcome the hub hasn't seen.
- **What the hub lets go of.** The hub keeps the newest 10,000 reports of each agent, a few years of an agent's daily backups, so one agent can't fill the hub's store. Losing the oldest silently would leave a dashboard that looks whole and isn't, so the hub counts what it let go of, per agent, for the dashboard (#569) to show, and its log says so once per agent while it runs. The line names the agent and the count, never what a report says (I4).
- **The window.** 32 places ahead of the acknowledgements lets a burst flow without a round trip per report, and bounds what a dropped connection makes the agent send again. A window spans places, not reports, so no report sent again lies 32 places or more behind the last the hub took, however far apart a full outbox's gaps spread its reports: the hub tells an older database's reports by that distance.
- **Epochs.** The hub keeps the last place of each agent's 8 newest epochs, by when it last took one, and always of the epoch it just took, so a step back of the hub's clock can't forget the place a resend is checked against: a rebuilt agent database starts a new epoch, and 8 is more than an agent rebuilds in its life, while an older epoch's place is forgotten.
- **The connection.** An approved agent's connection leaves the handshake's bounds once the hub welcomed it, and the hub closes one that sends no frame for 10 minutes, so a stuck agent doesn't hold a socket for good, and one whose agent connected again, so an agent holds one live connection ([decision 120](120-agent-hub-connection-security.md)); its frames keep the 4 KiB cap. A second computer under the agent's ID is refused at its `Hello`, before it sends a report ([decision 150](150-the-hub-takes-one-computer-per-machine-id.md)).

## Consequences

- A revoked agent's connection already open runs until the agent closes it; the revocation takes effect at its next connection.
- An agent connects once per burst of reports, about twice per backup, until #571's heartbeats hold a connection open.
- The hub warns once per agent per run of the hub; later drops are only counted.
- An agent restored from an older database numbers its new reports again from places the hub took, so the hub drops them as taken. It can tell only those 32 or more places behind the last it took, as no resend lies that far, and logs the first once per agent per run; nearer ones pass as resends. #1387 proposes the fix: the agent states its next place in the handshake, and a hub that took further starts a new epoch.
- The outbox's epoch is made by SQLite (`randomblob`) with the outbox, so the store needs no randomness port.
- An operation ended by the daemon's own stop, or by the next start's recovery, ends after the reporting task stopped or before it subscribed: the hub holds its start without its end.
- The event bus can drop a start or an end from the reporting task's queue when the task falls behind (#1437), and then neither side says so: the bounds above cover only what reaches the outbox.
