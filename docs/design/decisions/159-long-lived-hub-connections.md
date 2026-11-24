# Decision 159: An approved machine keeps one hub connection while it runs

An approved machine opens one sync connection when its daemon starts and keeps it
while the daemon runs. Reports, plans, progress, leases and heartbeats share
that connection. After a drop, the machine reconnects with the existing
backoff, beginning at one second and doubling to five minutes. The hub keeps
only one live connection for a machine ID; when a newer connection is proved,
it closes the older one.

The machine sends a `Heartbeat` every 30 seconds. It carries the running
operations and their repository, kind, index-read time and deadline, plus the
next scheduled runs. The hub records the time of the latest accepted heartbeat
on its clock. A machine is offline after three heartbeat intervals (90 seconds)
without an accepted heartbeat. The hub emits one offline event and notification
for a transition to offline, and one online event when a later heartbeat is
accepted. It does not distinguish sleep from another lost connection in
protocol 1; the reconnect and the transition back online are the behavior after
sleep or a network change.

A pending or refused machine does not keep a connection. Removing a machine,
shutting down either daemon, a protocol mismatch or a failed authentication
ends the connection. A connection does not keep a machine's backups from
running when the hub is unavailable; cached grants expire on the hub's signed
time as already decided in decision 151.

## Why

A connection per burst cannot carry hub-to-machine work, a reliable liveness
signal or the running-operation facts needed by the owner prune guard. Keeping
one approved connection avoids polling and lets the hub renew leases and mark a
machine offline from a bounded heartbeat. Three intervals gives ordinary
network jitter room without leaving a dead machine looking online for ten
minutes.

## Consequences

- The agent's persistent outbox and acknowledgements remain ordered and
  bounded; a reconnect resumes them from the last signed acknowledgement.
- The hub's heartbeat and running-operation state are bounded by the one live
  connection and the existing report limits. A heartbeat replaces the old idle
  close as the liveness clock.
- Idle network and battery use must be measured for one hour on a laptop before
  the implementation merges; the result belongs in the pull request and the
  user documentation if it changes setup guidance.
- A later protocol may add an explicit sleep announcement or a negotiated
  heartbeat interval; protocol 1 does not require either.

Amends decision 145's per-burst connection lifetime and §7.2's current session
text. It enables issue #1463 and is a prerequisite for offline alerts (#570) and
the owner prune guard (#573).
