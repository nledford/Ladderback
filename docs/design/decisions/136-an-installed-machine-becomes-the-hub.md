# Decision 136: How an installed machine becomes the hub

**Any installed machine can become the hub in place, by the command line or by a form in Settings, and a hub can run beside a machine instead, as a second Ladderback the machine pairs with. Moving a machine's data into a container is not supported until a general way to move a machine exists (#1050). From M8's first release, hub mode is a setting in `config.toml`, with the listening address and the allowed host names, and a command-line flag overrides it. Turning hub mode off is refused while any machine is paired. Decision 126 recommends by role, not by install channel** (recorded 2026-11-20, §7 and §12; the maintainer's decision on #1202).

**Amended 2026-11-21 (#1206):** "Become a hub" asks whether a hub already exists and, if so, takes that hub's pairing token so the machine pairs first and can be promoted later. No network discovery is built. Points 7 and 9 below.

## Why

- **Becoming the hub changes a mode, not an install.** The hub is a machine running in hub mode ([architecture.md](../architecture.md)), and from M8's release every build has the hub's code (decision 122). A machine that becomes the hub in place keeps its plans, its snapshots' host and paths, its secrets and its data folder: nothing is copied.
- **Copying a machine into a container breaks it.** The hub is a machine as well, so the container takes over that machine's backups, and there:
  - its snapshots change identity: they take the container's hostname as their host and record the paths inside the container (operations.md §12, Docker), so the first backup finds no parent, and retention keeps old and new snapshots in separate groups;
  - it sees only what is mounted into it, and has no keyring, nor the `password-command` program or rclone the installed machine may use (§10.2);
  - if the installed service still runs, two daemons maintain the same repositories (I1);
  - a copied data folder carries the machine ID onto a second machine (#296).

  Handling all of that is the general move of a machine to a new computer (#1050), not a step of its own.
- **A role kept only in a service file would need migrating later.** With hub mode in `config.toml` from the first release, no M8 hub has its role only in a systemd unit or launchd plist, and `serve` reads it at start like any other setting.
- **A recommendation by role survives new channels.** Naming a channel for the hub would need amending for each new one, such as an apt or dnf repository.

## Decided

1. **Routes.** Supported:
   - in place, by the command line;
   - in place, by a Settings form;
   - a hub beside the machine, which the guide documents and which needs no code of its own.

   Moving a machine's data into a container is not supported until a general "move this machine" procedure exists (#1050). The guide says so and points to the in-place routes.
2. **Hub mode lives in `config.toml` from M8's first release,** with the listening address and the allowed host names (today `serve --listen` and `--allowed-host`, security §10.1). No M8 hub keeps its role only in a service file, so none needs migrating later. The Settings form is a thin layer over that setting and can follow M8.
3. **Decision 126 recommends by role, not by install channel:** any installed machine can become the hub, and the Docker image is the recommended channel for a machine whose only job is the hub.
4. **The config is the source of truth; a command-line flag overrides it.** When a flag does, Settings shows the value read-only and says the command line sets it, so the page never shows a setting that isn't in effect. `install-service` needs no `--hub` of its own.
5. **Turning hub mode off is refused while any machine is paired,** naming them. Handing the hub role to another machine without pairing again is the durable answer and belongs with moving and restoring a hub (#1039).
6. **A hub that sleeps or goes offline is warned about from what the hub observes,** not from a guess at the hardware (#1205). A battery check at setup may add an early hint through the platform adapter.
7. **"Become a hub" asks whether a hub already exists** (the maintainer's decision on #1206, 2026-11-21). On yes it takes that hub's pairing token, so the machine pairs first, checking the token and the hub's fingerprint as decision 121 has it, and can be promoted to hub later by the handover of #1039, which runs on the hub for a paired machine. The form makes no hub until the person confirms. A hint never gates: the option to become a hub stays available whatever the person answers.
8. **A hub doesn't need sign-in on.** [Decision 120](120-agent-hub-connection-security.md) ("Who may make a token, approve or re-key") asks for it when a browser beyond loopback makes a pairing token or approves a machine, and for the Recovery Kit's page wherever it is opened. With sign-in off, a browser on the hub's own machine makes tokens and approves machines, and `ladderback pairing-token`, `recovery-kit`, `hub-recover` and the approval prompt work in a terminal as the data folder's owner. Sign-in stays the recommendation for a hub reachable on the network, and the guide says why.
9. **No mDNS or other network discovery.** An announcement is an unauthenticated answer from whoever replies, the weakness decision 120 named in rejecting RFC 8628's device flow. It needs a new crate, with its own approval and `cargo deny`, and contends with Avahi and mDNSResponder for UDP port 5353. It also fits poorly with decision 87's loopback default and decision 122's `hub` build gate, since browsing would have to sit in builds without the hub. If discovery is wanted later it comes back as a new issue and only as a hint: it never pairs, never pre-trusts a fingerprint (none in the record, or one marked claimed and unchecked), never blocks "Become a hub", announces only when the hub already listens beyond loopback, and has its own crate approval.

## How a person does it

The guide's hub section, written for M8's release, takes these steps. Until a release has the hub (decision 122), the guide says only that it arrives with M8.

**In place.** Copy the future hub's config and data folders aside, then update every machine to M8's release, in any order (decision 88).

1. Turn hub mode on in that machine's `config.toml`, with an address the other machines can reach and the names they reach it by, and restart its service, or Settings' restart (decision 94). Once it follows M8, the Settings form writes the same setting.
2. The hub serves its pages on the listener the other machines sync with (§7.1). With built-in TLS its address becomes `https://` with a self-signed certificate: the browser warns once, and bookmarks and MCP connect addresses change.
3. Save the Recovery Kit offline within its 7 days ([decision 121](121-pairing-a-machine-with-its-hub.md)).
4. Pair each other machine: a pairing token on the hub, `ladderback pair` on the machine, then approve it (§7.1).
5. Back up the hub's own state: a plan for its data folder, with what restoring it needs written down (#1039 decides what a restored hub may do).

**Beside.** Run the hub as a second Ladderback on the same computer, such as the Docker image with volumes of its own and another port, and pair the computer's installed machine with it like any other. Nothing moves, and the hub's own machine backs up nothing.

**Not by moving into a container,** for the reasons under "Why", until #1050.

## What it costs

- Until the form follows M8, the command line is the only in-place route.
- A machine paired with a hub that is to stop being one has to be unpaired first, until #1039 decides a handover.
- The form's own checks, and the names of the new `config.toml` keys, are settled when they are built.

## Rejected

- **Moving the machine into the container now** (#1202's second option): the mounts, the snapshots' host, the secrets and the machine ID each need handling, which the general move (#1050) covers once.
- **Hub mode only as a flag of the service** (`install-service --hub`): the role would live outside the config, and every hub would need migrating once the config held it.
- **The image as the hub's one recommended channel** (decision 126 before this amendment): it ties the role to an install channel, and a machine installed from a package would have to move to become the hub.
- **Warning from a guess at the hardware,** such as "a laptop": what the hub observes (#1205) says whether it was asleep or offline.
- **Finding a hub by mDNS** (#1206's first option), by the OS responders over D-Bus and `dns-sd` (two code paths, and Avahi isn't everywhere), or by probing the subnet for the port (it reads as a port scan): see point 9. Asking the person, then pairing first, needs none of them.
