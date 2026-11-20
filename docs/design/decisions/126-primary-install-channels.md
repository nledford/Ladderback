# Decision 126: Which ways to install Ladderback the project recommends

**Each kind of install has one recommended channel: Linux packages for an agent on Debian, Ubuntu and Fedora (the install script on other Linux systems), the install script for an agent on macOS, and the Docker image for a hub. Homebrew becomes the macOS channel only once a macOS archive can be signed and notarized (#344). The other channels stay for developers or for the community, and crates.io is not a channel** (recorded 2026-11-19, §12; the maintainer's decision).

**Amended 2026-11-20 (the maintainer, #358):** the Docker image serves both roles: a machine's agent, which backs up the folders mounted into it, and from M8 the hub. Releases publish it, once the release workflow does, as a preview that the README calls not yet a supported way to install and run Ladderback, and until M8's release it runs without the hub (decision 122). With M8's release it becomes the hub's recommended channel. For an agent on Linux the packages, or the install script, stay the recommended channel and the image is an alternative; an agent on macOS uses the install script. What an agent in a container can't do is documented as its limits in the [install guide](../../guide/install.md#docker-image-not-supported-yet), not a reason to refuse it.

## Why

An agent and a hub are installed for different reasons. An agent backs up the machine it runs on, so it belongs to that machine's own package manager and to its user's service manager (decision 64). A hub is a long-running server on a server or a NAS, the place where containers are the usual way to run one.

One recommended channel for each case keeps the README and the install guide short, and tells a new user what to pick without a comparison table.

## Decided

1. **An agent on Debian, Ubuntu, Raspberry Pi OS or Fedora:** the release's `.deb` or `.rpm` package (decision 123). The package manager knows the program, `ladderback install-service` points the user service at `/usr/bin/ladderback`, and removing it is the package manager's usual command.
2. **An agent on any other 64-bit glibc Linux:** the install script (decision 79), which downloads the release's archive and checks it against `SHA256SUMS`.
3. **An agent on macOS:** the install script, which builds the release from source.
   - The recommended path includes signing that build with a self-signed certificate named in `LADDERBACK_SIGN_IDENTITY` (decision 71; the install guide's "Signing your build"), so that Full Disk Access carries over to the next update.
   - An ad-hoc-signed build loses its Full Disk Access grant at every update, and a backup tool that quietly stops reading protected folders is worse than one that is harder to install.
4. **A hub:** the Docker image (#358), published as `ghcr.io/nledford/ladderback` and `docker.io/nledford/ladderback` (the maintainer's choice: Docker Hub is where a NAS's container app searches), on a server or a NAS, with the Linux packages as the alternative on a machine without containers. Amended 2026-11-20: the image is no longer for the hub only, and an agent may run from it too, as an alternative. In a container, an agent sees only the folders mounted into it, runs as the container's user, takes the container's hostname, and on macOS, where Docker runs Linux in a virtual machine, gets no Full Disk Access, Photos library or keychain. These are the limits the install guide documents.
5. **The Homebrew tap (#359) waits for #344.**
   - A formula that builds from source is signed ad hoc on every upgrade: Homebrew filters the build's environment, so `LADDERBACK_SIGN_IDENTITY` doesn't reach it, and every upgrade loses Full Disk Access.
   - A prebuilt binary that isn't notarized is blocked by Gatekeeper.
   - So the tap ships the notarized macOS archive once #344 makes one, and then becomes the macOS channel. Until then there is no tap.
6. **Not recommended, still available:**
   - `just install` and the by-hand steps in the install guide are for contributors and for building a branch.
   - `cargo binstall --git` (#357) is for people who already use binstall.
   - The AUR (#360) and Nix (#364) are left to the community; the project reviews a contributed package rather than maintaining one.
7. **Not a channel:** crates.io (#207, closed as not planned). Publishing would mean about 20 more crates on every release and a web UI built outside Cargo, for one more way to install that the channels above already cover.

## Consequences

- The README and the install guide lead with the recommended channel for each platform and give the others after it. Since the packages arrive with 0.5.0, that reordering lands in the 0.5.0 release's docs.
- A new channel is proposed as a sub-issue of #361, the tracking issue for release channels, and becomes a recommended one only through a decision that amends this one.

## Rejected

- **The install script everywhere.** It works on every supported system, but on Debian, Ubuntu and Fedora it installs into `~/.cargo/bin`, where the package manager can't see the program.
- **Docker for agents too.** Reversed in part on 2026-11-20 (the maintainer, #358): the image now serves an agent, as an alternative rather than the recommended channel. The reason given here stands as a limit: a container can't see what an agent has to back up unless it is mounted into it, so backing up a whole machine means mounting it and running as root.
- **A Homebrew formula built from source, now.** See point 5: it trades an easier install for the loss of Full Disk Access at every upgrade.
