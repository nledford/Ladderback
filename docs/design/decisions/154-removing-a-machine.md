# Decision 154: How a person removes or rejects a machine on the hub, and when a waiting machine lapses

**On the running hub's Settings → Machines, a waiting machine has Reject beside Approve and an approved one has Remove. Either opens a review that names the machine and shows its key fingerprint in full, starts on Cancel and asks for no step-up. The hub removes the agent ID under the key the page listed, drops its record and keeps a tombstone of its ID, its key and, as a record only, its hostname. The removed machine's next connection is answered with a signed `agent-removed`, and so is a pairing under its ID with an ordinary token. A machine nobody approves within 7 days lapses: the hub drops it the next time it reads its agents and keeps no tombstone.**

## Why

[Decision 120](120-agent-hub-connection-security.md) set the rules for removal, rejection and a pending agent's lapse ("Pending approval", "Removing, re-keying and recovering an agent"), and [decision 121](121-pairing-a-machine-with-its-hub.md) the page that approves. Until now the hub could approve a machine but not take one away, and a stranger that paired with a stolen token stayed in the list for good. This decision is the part of those rules built now (#1439), and where it differs from them.

- **No step-up.** A removal or a rejection only takes authority away, as revoking a token or holding an agent does (decision 120, "Who may make a token, approve or re-key"). It needs a signed-in session when sign-in is on, never an API token, and beyond loopback with sign-in off it is refused as `pairing-needs-sign-in`, as approval is.
- **Under the key the page listed.** `POST /api/v1/hub/agents/{id}/removal` carries the key the row showed. A machine that paired again with another key since, or one already removed or lapsed, is `agent-unknown`, and nothing changes. Rejecting a waiting machine and removing an approved one are one request: the hub decides which by the record's state.
- **One transaction.** The record is read, removed and its tombstone kept in one `BEGIN IMMEDIATE` transaction, so of two removals at once the second finds no agent, and an approval racing a removal either lands first or finds the agent gone.

## The tombstone

- **It keeps the agent ID and the key,** so the hub can prove to the removed machine, signed, that it removed it, and refuse an ordinary token under that ID (decision 120, "A removed agent's ID is a tombstone"). A proof under another key gets `proof-refused`, as any stranger does, so the tombstone tells a stranger nothing.
- **It keeps the hostname only as a record.** The hostname is not held and not unique among tombstones: another machine may pair under the name at once. Comparing a new name with removed ones for lookalikes stays with #1346.
- **It is kept for good,** in `agent_tombstones` (migration `0025_agent_tombstones.sql`). A second removal under the same ID replaces the one before.

## The lapse

- **7 days, judged when the hub reads its agents.** A machine that has waited for approval 7 days or longer is dropped the next time the hub lists, pairs, approves or takes a proof, so no timer runs. An approved machine never lapses.
- **No tombstone and no marker.** Nobody judged the machine, so its hostname is free and it may pair again with a new token under the same ID and key. Decision 120's 30-day marker and its signed `pending-expired` are not built: a lapsed machine that reconnects is refused `proof-refused`, which already says its wait may have lapsed.
- **It is logged.** The hub writes one line for each lapse and each removal, naming the machine as a pairing names it.

## Telling the machine

- **On its next connect.** A machine whose proof holds under the key its tombstone keeps gets a signed `agent-removed` and a close. Decision 120 answered it `proof-refused`, as a stranger; the signed refusal goes only to the holder of the removed key, so it tells a stranger nothing and lets the machine say plainly what happened. `ladderback hub-status` and `ladderback pair` show its message: the hub removed or rejected this machine and no longer accepts it, and it keeps backing up and runs no maintenance. The message names no action, since no replacement token exists yet to bring it back.
- **A live connection ends at its next `ReposSnapshot`.** Decision 120's signed `Removed` message, sent as the hub closes the connection, is not built. A machine connected at the moment of its removal is closed with `protocol-error` the next time it asks for its grants, within 15 minutes ([decision 151](151-a-paired-machine-maintains-only-under-its-hubs-lease.md)), since the hub finds no record of it; until then the hub still takes its reports and tells them to its browsers. It learns of the removal when it next connects.

## Telling the browsers

The hub tells its browsers `AgentRemoved` with the machine's agent ID, hostname and when, on a session's stream or the browser's with sign-in off, as `AgentPending` and `AgentApproved` ([decision 149](149-the-hubs-dashboard.md)). Settings → Machines and the dashboard read the hub again on hearing it. A lapse sends no event: those pages read the hub each time their events stream opens, and the lapsed row leaves on that read.

## Consequences

- A removal is permanent until replacement tokens are built (decision 120, "Pairing"): the machine can't come back under its ID.
- Leases the hub counted for a removed machine are not ended at the removal. They expire as decision 147 says, so the hub grants a repository the removed machine held to another only after that lease runs out, and the removed machine maintains nothing once its own copy of the lease ends ([decision 151](151-a-paired-machine-maintains-only-under-its-hubs-lease.md)). A `ReposSnapshot` decided across the removal, its approval read just before and its lease written just after, can still renew such a lease, which only delays that handover.
- A removed or rejected machine stays paired on its own side, with no way to unpair yet, so it never maintains its repositories again; its messages say it runs no maintenance.
- The hub keeps a removed machine's operation reports (`sync_reports`, up to 10,000), their tallies and its watermarks; its ownership reports expire on their own.
- The waiting table shows how long a machine has waited, not how long is left, and a lapsed machine leaves it only when the page next reads the hub.
