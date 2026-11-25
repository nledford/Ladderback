# Decision 169: A machine leaves a lost or replaced hub at its terminal, with its daemon stopped

**`ladderback unpair`, run at the machine while its daemon is stopped, leaves the hub it pinned: once the person confirms (`[y/N]` at a terminal, `--yes` without one), it drops every lease and epoch floor it took from that hub, then forgets the pin. The machine then stands alone, maintains its own repositories as its configuration says, and pairs with any hub's token. `pair` with another hub's token, while a pin is kept, is refused as `pinned-to-another-hub`, naming both hubs' key fingerprints and `ladderback unpair`** (recorded 2026-11-25, §7.1 and §7.3; #1637).

## Why

- **A replaced hub, or a machine taken out of a fleet, is a likely path**, and deleting `hub-pin` by hand was the only way out. The refusal said `An unexpected error occurred` with the error's debug text, which §8.4 forbids.
- **Stopped, not told.** A running daemon holds what a lease is judged by in memory as well as in its store (when it last took its hub's grants, the prunes its hub holds), may be running a maintenance under a lease, and holds a session that takes the hub's next grants moments after a leave. Telling it would need a new local route with its own sign-in and step-up, and a stop of the session that no grant can race. The data folder's lock (§9: one process per data folder) refuses `unpair` while the daemon runs, as it refuses `pairing-token`, and the next start reads no pin. That is the smaller change, and one a test can prove.
- **The leases go first.** A machine that can't drop them keeps its pin, so it never stands alone holding a lease of the hub it left (I1), and running `unpair` again tries again. Its maintenance pauses go with them, so a next hub's 14-day alert counts from its own pause. A machine that dropped them and then couldn't forget the pin is still paired with no lease, so it maintains nothing until it can; its epoch floors are gone too, so until `unpair` runs again it no longer catches a rewound copy of that hub.
- **The hub isn't told.** It may be lost, which is why the person runs `unpair`. Its lease expiry (§7.3) stays the guarantee on its side: it grants the repository to no other machine before the lease this machine held runs out. A hub that still runs lists the machine until a person removes it in its Machines tab, which `unpair` says.
- **Only the pin goes.** The agent key, the machine ID and the connection counter stay, so a later `pair` proves the same machine, and a hub that removed it refuses it as before (decision 154).

## Decided

1. `ladderback unpair [--yes]`, in builds with the hub (decision 122). With no pin it says the machine isn't paired and succeeds. Otherwise it takes the data folder's lock, names the pinned hub by its address and key fingerprint, and asks.
2. The `Identity` port forgets the pin (`forget_pin`); `AgentPairing::leave` saves an empty lease cache, then forgets the pin.
3. `AgentPairingError::PinnedToAnotherHub` is the error code `pinned-to-another-hub`, with the arguments `pinned` and `token`, the two hubs' key fingerprints.

## What it costs

- A machine whose daemon runs has to be stopped first.
- A machine the hub removed keeps its pin until the person runs `unpair`. The hub's signed `machine-removed` could make it leave on its own; that waits for its own change (#1637's known limit), since the daemon would have to drop leases it holds in memory while it runs.
- A machine that stands alone maintains each repository its configuration gives it (`maintenance = "local"`), as any machine with no hub does. If another machine of the fleet maintains one of them too, the person changes one of the two configurations, as two machines with no hub would.

## Rejected

- **`pair --replace`**: it would mix leaving one hub with pairing another in one confirmation, and a removed machine with no new hub to pair with would have no way to stand alone.
- **Leaving through the running daemon's local API**: see "Stopped, not told".
- **Asking the old hub to release the leases first**: the hub is usually the one lost.
