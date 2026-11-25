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

## As built (#1463)

- **The heartbeat is empty for now.** Protocol 1's `Heartbeat` is `{}` until
  the owner prune guard (#573), the first reader of `running_ops` and
  `next_runs`, adds them as optional fields. The hub answers each heartbeat
  with a signed `HeartbeatAck {}`, so a machine learns within the 15 seconds
  it gives each answer that a connection which still takes its writes is
  gone. A heartbeat from a machine of this release carries no
  `running_ops`, so #573 must not read a missing list as "nothing running".
- **Any message keeps a connection alive.** The hub closes a connection that
  sent it no message for 90 seconds, counted on its monotonic clock, so a
  step of its wall clock marks no machine offline. WebSocket pings don't
  count. A heartbeat is the message an idle machine sends; a report or a
  lease exchange counts too, so a long replay is never taken for silence. A
  machine keeps beating while it lists its repositories before a lease
  exchange.
- **Online from the first message.** A machine sends a heartbeat as it
  connects, and is online from its connection's first message after the
  `Welcome`, when the browsers hear `AgentOnline`; `AgentOffline` when the
  hub no longer holds a connection of it. A check from the machine's
  terminal (`hub-status`, or `pair` again) sends none, so it never replaces
  the daemon's connection. A newer connection replacing the older tells
  nothing. The offline notification is the offline alert's (#570).
- **Last heard from.** When a machine's live connection ends, the hub keeps
  that time as when it last heard from the machine, which the dashboard
  shows when it isn't online, so a machine that held a connection for days
  shows when it went away, not when it connected.
- **Removal closes the connection.** Removing a machine closes the
  connection the hub holds of it at once (decision 154), and a connection
  whose machine a person removed during its handshake is closed as it goes
  live.
- **The machine's own troubles keep the connection.** A configuration the
  machine can't read skips the lease exchange, with a warning, and keeps the
  connection. Leases are asked for as each connection starts and every 15
  minutes, no longer when reports are sent. The backoff starts over at 1 s
  only after the hub answered a heartbeat, so a hub that welcomes and then
  drops each connection is tried less and less often.
- **Presence is in memory.** After the hub restarts, every machine is offline
  until it connects again: within a heartbeat and a second when the restart
  is quick, and within the backoff, at most 5 minutes, after a longer
  outage.
- **Idle cost, measured.** On 2026-11-24, from 23:52 to 00:22 UTC (30
  minutes, cut short of the hour), the release build was measured on an
  Apple-silicon Mac: a hub, one paired, approved and idle machine, and an
  unpaired daemon as the baseline. Per hour, by doubling the 30 minutes,
  the machine took 0.38 s of CPU against the baseline's 0.08 s, and about
  27,000 context switches against 2,800. It sent about 21 KB and received
  about 24 KB. The hub took 0.08 s of CPU and about 3,500 context switches.
  `top` read 0 idle wakeups on that busy, shared machine, so context
  switches stand in for wakeups. The machine's extra context switches,
  about 7 a second, are mostly its 2-second look at what is due; a longer
  look while idle is a later change if they matter.
