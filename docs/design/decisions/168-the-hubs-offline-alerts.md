# Decision 168: The hub alerts a machine offline for 15 minutes once, and its return only after an alert, from memory

**Every 30 seconds the hub watches its approved machines. One offline for 15 minutes is told once through the hub's notification rules as `machine.offline`, with the machine's hostname and since when it is offline; one that comes back after that alert is told once as `machine.online`, with when it came back, and one back sooner is told nothing. Removing a machine cancels its alert. The hub keeps this in memory, so after it restarts a machine still offline is offline since the hub's first watch and is alerted 15 minutes after the restart.** (Recorded 2026-11-25, #570; the maintainer decided the times and the single alert on 2026-11-24.)

## Why

The dashboard shows a machine offline once its connection ends, 90 seconds after it last sent anything at most ([decision 159](159-long-lived-hub-connections.md)), but nobody watches a dashboard. A machine whose daemon stopped, or whose network broke, backs up nothing its hub can see, and the person should hear of it. A laptop closed for lunch or a router restarting shouldn't page anyone.

- **15 minutes, once.** Offline at 90 seconds is right for the page and too early for an alert: a reboot, an update or a short network change ends within minutes. Fifteen minutes passes those; one alert per time a machine goes offline keeps a machine off for a week from repeating it.
- **Back online only after an alert.** A person told a machine is offline wants to know when it's back; one told nothing has nothing to close. A return the hub didn't alert is told to no one, so a machine that drops for a minute sends nothing at all.
- **No sleep handling.** Protocol 1 has no sleep announcement (decision 159), so a laptop asleep for 15 minutes is alerted like a machine that stopped. A person who doesn't want that leaves `machine.offline` out of their rules; a later protocol may add the announcement and let the hub hold the alert.
- **A watch, not a timer per machine.** The hub's daemon watches every 30 seconds, as often as a machine sends a heartbeat. The connection's end says when a machine went offline and the connection's first message when it came back, so the watch only reads what is due: an alert goes out at most 30 seconds after the 15 minutes, and a return at most 30 seconds after it. Time comes from the `Clock` port, so the scenarios move it by hand.
- **In memory.** Presence is in memory already (decision 159), so the alerts live beside it and need no migration. A machine offline when the hub starts is offline since the hub's first watch, which is the best the hub knows: its connection might have ended at the stop.
- **Sent means a rule took it.** The alert counts as sent once a rule sends `machine.offline`, whether or not its channel answered, as every notification is tried once (decision 74). While no rule sends it, or the configuration can't be read, it is due again at the next watch, so a rule added later still hears of a machine that is still offline, and no return is told for an alert nobody sent.
- **The machine's hostname.** An event names the machine by the hostname it paired under, which the dashboard shows: a template's `{machine}` and a webhook's `machine`. Other events have none, and a rule for some plans never matches a machine's event, as it never matches a prune's.

## Consequences

- A hub restart forgets which machines it alerted. A machine still offline is alerted again 15 minutes after the restart, with the restart as since when it is offline; one that came back during the restart is told nothing.
- A laptop asleep over 15 minutes sends `machine.offline`, and `machine.online` when it wakes.
- A machine that never connected after its approval is alerted 15 minutes after the hub's first watch that found it offline.
- `machine.offline` pings a healthchecks channel's `fail` URL, and `machine.online` its success URL.
- A build without the hub accepts `machine.offline` and `machine.online` in `config.toml` and never sends them, as it accepts `repo.maintenance_paused`; the hub feature is deleted before the MVP's release (#1366).
