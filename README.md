# Ladderback

[![CI](https://github.com/nledford/Ladderback/actions/workflows/ci.yml/badge.svg?branch=main&event=push)](https://github.com/nledford/Ladderback/actions/workflows/ci.yml?query=branch%3Amain+event%3Apush)
[![MSRV](https://img.shields.io/badge/dynamic/toml?url=https%3A%2F%2Fraw.githubusercontent.com%2Fnledford%2FLadderback%2Fmain%2FCargo.toml&query=%24.workspace.package%5B%27rust-version%27%5D&label=MSRV&color=orange)](Cargo.toml)
[![Latest release](https://img.shields.io/github/v/release/nledford/Ladderback?sort=semver&label=release)](https://github.com/nledford/Ladderback/releases/latest)

A self-hosted backup manager for [rustic](https://rustic.cli.rs): plans, schedules and one dashboard for every machine.

> [!WARNING]
> **Ladderback is early in development: its current release runs backups only by hand, with no scheduler and no restore.** Don't rely on it for backups until the [roadmap](#roadmap) shows a usable release. Until then, use the `rustic` CLI directly or [Backrest](https://github.com/garethgeorge/backrest); the `rustic` CLI also restores from a repository Ladderback backs up to.

## What it will do

Without a manager, each machine runs rustic from its own cron job or timer. Failures surface only if someone reads the logs, a laptop that was asleep at 02:00 silently skips its backup, and retention on a shared repository needs hand-coordination between hosts. Ladderback turns rustic into a managed service on each machine, with one place to see them all:

- **Plans and schedules.** What to back up, from which machine, into which repository, how often, and how long to keep it. Missed runs on sleeping laptops catch up at the next wake.
- **One dashboard for every machine.** Status, live progress, history, warnings and errors for every connected computer, with notifications (webhook, ntfy, Gotify, healthchecks.io, email).
- **Safe maintenance on shared repositories.** Backups never wait for a lock, and exactly one place runs forget, prune and check for each repository.
- **No lock-in.** Import existing rustic profiles and export any plan back to one, so the `rustic` CLI always stays an option.
- **Careful restores.** Restore into a new folder by default; in-place restore only from the machine's own UI, after a dry-run summary.
- **Scriptable by AI agents.** `ladderback mcp` is an MCP server over stdio. It drives a running instance, or a headless session of its own, through read, run and preview tools. It can't restore, delete or change configuration.
- **One self-contained program.** It runs headless on servers and as a user service on desktops and laptops, with the web UI built in. It is accessible (WCAG 2.2 AA) and localization-ready.

Ladderback supports **Linux and macOS**. Windows is out of scope.

## Ladderback or Backrest?

Ladderback borrows Backrest's user-facing model: repositories, plans, schedules, hooks, an operation log and restore into a new folder. The difference is the engine underneath. Backrest wraps the `restic` CLI. Ladderback links rustic's library, `rustic_core`, and inherits rustic's lock-free repository design.

**Ladderback may suit you better if you:**

- back up **several machines into one shared repository**. In Backrest, each plan runs `restic forget` after its backup, which takes an exclusive lock and blocks other hosts' backups ([backrest#244](https://github.com/garethgeorge/backrest/issues/244)). In Ladderback, backups never wait for a lock, and one owner per repository runs maintenance;
- don't want repository **passwords and backend keys copied to other machines**. Backrest's multi-host sync pushes shared repositories to clients, credentials included. Ladderback's hub keeps its own secrets, and agents never send theirs;
- back up **laptops**. Missed runs catch up by default, and the scheduler notices sleep and wake;
- want **secrets out of the config file**: a password command, the system keyring, a file or an environment variable. A missing secret fails the job instead of silently becoming an empty string;
- want **time-based retention** (`keep-within-*`), not just counts;
- already use **rustic profiles** and want to keep using the CLI on the same repositories.

**Backrest is the better choice if you:**

- need something that **works today**. Backrest is mature; Ladderback hasn't reached its first usable release (M2) and has no scheduler or restore yet;
- use **Windows**;
- use **restic** and want to keep restic maintaining your repositories. Restic must never prune a repository Ladderback manages;
- need **multiple user accounts** or a way to run **arbitrary repository commands** from the UI. Ladderback has neither;
- prefer restic's longer track record over rustic, which is the younger project.

The [technical design](docs/design/README.md#relationship-to-backrest) has the full comparison, with sources.

## Installation

Install it to try it out, not to rely on it: the warning at the top still holds. Releases don't come with binaries yet, so you build one from source, on Linux or macOS.

You need:

- **Rust**, through [rustup](https://rustup.rs). In the checkout, rustup installs the exact Rust version `rust-toolchain.toml` pins, with the `wasm32-unknown-unknown` target, on the first build, so you build with the compiler CI tested ([decision 78](docs/design/decisions/78-the-rust-toolchain-is-pinned.md)). Without rustup, use Rust 1.100.0 or newer, the [MSRV](Cargo.toml), with that target, and take trunk and just from their prebuilt releases.
- **git and a C compiler**: Xcode's Command Line Tools on macOS, `git` and `build-essential` or your distribution's equivalents on Linux.
- **Node.js** 20 or newer, with npm: the web UI's build runs Tailwind's CLI.
- **[trunk](https://trunkrs.dev)**, which builds the web UI, and **[just](https://just.systems)** 1.40 or newer: `cargo +stable install --locked trunk just`, or your package manager's. trunk's newest release can need a newer Rust than the one the checkout pins, hence `+stable`. Building trunk from source needs more than about 2 GB of memory; on a smaller machine, take trunk's [prebuilt release](https://github.com/trunk-rs/trunk/releases) instead. trunk downloads the `wasm-bindgen` and `wasm-opt` the build needs.

On 64-bit ARM Linux with a glibc older than 2.38 (Debian 12, Ubuntu 22.04), the `wasm-bindgen` trunk downloads doesn't run. There, between the `git checkout` and `just install` below, and again after each update, build the version the checkout pins, which trunk then finds on the `PATH`: `cargo install --locked wasm-bindgen-cli --version "$(just _wasm-bindgen-version)"`.

Clone the repository, check out the newest release, and install it:

```sh
git clone https://github.com/nledford/Ladderback.git
cd Ladderback
# The newest release tag, leaving out release candidates (vX.Y.Z-rc.N).
git checkout "$(git tag --list 'v*' --sort=-v:refname | grep -v -- - | head -n 1)" && just install
```

`just install` builds the web UI, then `ladderback` with the UI built in, as the release archives are built (the `dist` profile, with thin LTO), and installs it with `cargo install` into `~/.cargo/bin`. Arguments go to `cargo install`: `just install --root ~/.local` installs into `~/.local/bin`. If `just` answers that there is no recipe `install`, that release predates it; run this instead, which builds with the `release` profile (older releases have no `dist` profile):

```sh
(cd crates/web && trunk build --release --no-default-features) && cargo install --locked --path . --no-default-features
```

Start it with `ladderback serve` and open the address it prints, `listening on http://127.0.0.1:9898`; `--listen` picks another loopback address or port. Its configuration and data live in your platform's config and data folders ([technical design §4.2](docs/design/components.md#42-daemon-agent), "Config"). To update, run `git fetch --tags`, the same `git checkout` line, and `just install` again, which replaces the installed binary.

Without a password, anyone with an account on the machine can use Ladderback, and it listens on loopback only. To have it ask for a password, run `ladderback set-password` and start it again; it can then also listen beyond loopback, for a host name given with `--allowed-host` (put TLS in a reverse proxy in front of it). The rules are in [technical design §10.1](docs/design/security.md#101-web-ui).

## Roadmap

The skeleton (M0), the backup engine (M1a) and the single-machine MVP (M1b, released as [0.1.0](https://github.com/nledford/Ladderback/releases/tag/v0.1.0)) are in place: backups run by hand, with no scheduler yet. Each milestone is done when its acceptance scenarios pass, including keyboard-only and screen-reader checks for the UI. Milestone **M2** is the first release worth using on your own machines.

- [x] **M0: Skeleton.** Workspace, CI, test tooling (acceptance scenarios, mutation testing, browser tests with accessibility checks), and an embedded web UI shell.
- [x] **M1a: Engine.** Backups through `rustic_core` in an isolated worker process, with live progress, warnings, cancellation and deadlines.
- [x] **M1b: Single-machine MVP.** Connect or create repositories; secrets; plans and hooks in `config.toml`; rustic profile import and export; manual backups with live progress; snapshot list; operation history; manual forget, prune and check.
- [ ] **M2: Schedules and errors.** Scheduler with catch-up after sleep, time zones and retries; notifications; systemd and launchd service install; optional login. **First usable release.**
- [ ] **M3: Browse and restore.** Snapshot browser, restore into a new folder or in place, downloads as tar or zip, repository stats; on macOS, builds from source that keep their Full Disk Access across upgrades.
- [ ] **M4a: Hub, visibility.** Pair machines with a hub; one dashboard for all of them; offline alerts.
- [ ] **M4b: Hub, control and maintenance.** Repository ownership; forget, prune and check managed from the hub; remote run and cancel; opt-in restores from the hub.
- [ ] **M5: Polish.** Run conditions (AC power, metered networks), charts, Docker image, release packages, database rebuild, user docs.

Not planned for v1: Windows, multiple users, and rustic's `copy`, `merge`, `rewrite`, `repair`, `webdav` and `mount` commands (use the CLI).

## Documentation

- [PRODUCT.md](PRODUCT.md): who Ladderback is for, what it does and doesn't do.
- [docs/design/](docs/design/README.md): architecture, safety invariants, data model, milestones and decisions.

## Contributing

Ladderback is built for its author's own machines first and published for other rustic users, without support promises. Issues and discussion about the design are welcome. Development is behaviour-first and test-first; see [docs/design/development.md §13](docs/design/development.md#13-development-method) for the method and [AGENTS.md](AGENTS.md) for the working rules.

### Developing on Fedora

`just setup` installs the tools and Playwright's Chromium, but not the system libraries Chromium needs to run the `@ui` journeys (`just test-ui`). On Fedora, work in a [toolbox](https://containertoolbx.org/) or [distrobox](https://distrobox.it/) container: it is the only place `dnf` works on an atomic host (Silverblue, Bazzite), and it keeps the host clean on Workstation and Server too.

```sh
toolbox create ladderback && toolbox enter ladderback
sudo dnf install nss atk at-spi2-core libdrm libxkbcommon libXcomposite libXdamage libXfixes libXrandr mesa-libgbm alsa-lib
```

Outside a container, Workstation and Server take the same list with `sudo dnf install`; an atomic host can layer it with `rpm-ostree install`, which needs a reboot. This list is what headless Chromium needs to launch on Fedora 45.

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or [MIT license](LICENSE-MIT), at your option.
