<h1>
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="crates/web/assets/brand/logo-dark.svg">
    <img src="crates/web/assets/brand/logo.svg" alt="Ladderback" width="380" height="80">
  </picture>
</h1>

[![CI](https://github.com/nledford/Ladderback/actions/workflows/ci.yml/badge.svg?branch=main&event=push)](https://github.com/nledford/Ladderback/actions/workflows/ci.yml?query=branch%3Amain+event%3Apush)
[![MSRV](https://img.shields.io/badge/dynamic/toml?url=https%3A%2F%2Fraw.githubusercontent.com%2Fnledford%2FLadderback%2Fmain%2FCargo.toml&query=%24.workspace.package%5B%27rust-version%27%5D&label=MSRV&color=orange)](Cargo.toml)
[![Latest release](https://img.shields.io/github/v/release/nledford/Ladderback?sort=semver&label=release)](https://github.com/nledford/Ladderback/releases/latest)

A self-hosted backup manager for [rustic](https://rustic.cli.rs): plans, schedules and one dashboard for every machine.

> [!WARNING]
> **Ladderback 0.3.0 runs scheduled backups, but it can't restore yet.** To get files back, use the `rustic` CLI, which restores from any repository Ladderback backs up to; try a restore with it before you rely on those backups. The snapshot browser and restores in Ladderback come in M3. Until the hub (M4), each machine runs its own Ladderback with its own web UI. The [roadmap](#roadmap) has the order.

## What it does

Without a manager, each machine runs rustic from its own cron job or timer. Failures surface only if someone reads the logs, a laptop that was asleep at 02:00 silently skips its backup, and retention on a shared repository needs hand-coordination between hosts. Ladderback turns rustic into a managed service on each machine, with one place to see them all.

**Today**, each machine runs its own Ladderback, with the web UI built in:

- **Plans and schedules.** What to back up, from which machine, into which repository, how often, and how long to keep it. Missed runs on sleeping laptops catch up at the next wake, and notifications go to webhooks, ntfy and healthchecks.io.
- **Safe maintenance on shared repositories.** Backups never wait for a lock. One machine you choose maintains each repository, and Ladderback warns when it sees snapshots from other hosts in it.
- **No lock-in.** Import existing rustic profiles and export any plan back to one, so the `rustic` CLI always stays an option.
- **One self-contained program.** It runs headless on servers and as a user service on desktops and laptops. It is accessible (WCAG 2.2 AA) and localization-ready.
- **Scriptable by AI agents.** `ladderback mcp` is an MCP server over stdio. An agent can read status, plans, repositories, snapshots and history, run a plan and wait for it, cancel a backup, test a repository, preview a forget or a profile import, and export a plan as a profile. It drives a running instance, with a scoped API token when sign-in is on; a headless session of its own when none is running; or a throwaway scratch setup to try it on. It can't restore, delete or change the configuration.

**Later** (see the [roadmap](#roadmap)):

- **Careful restores (M3).** Restore into a new folder by default; in-place restore only from the machine's own UI, after a dry-run summary.
- **Backup setups proposed by AI agents (M3, or soon after).** An agent will be able to propose a backup setup, which changes nothing until a person confirms it in the web UI.
- **One dashboard for every machine (M4).** Status, live progress, history, warnings and errors for every connected computer.
- **One owner for each repository (M5).** Exactly one place runs forget, prune and check for each repository, whichever machines back up to it.
- **Later, unscheduled.** Notifications to Gotify and email.

Ladderback supports **Linux and macOS**. Linux means 64-bit (x86_64 or aarch64) with glibc 2.35 or newer: 32-bit ARM and musl distributions such as Alpine aren't supported. Windows is out of scope. It runs inside WSL2 as a Linux install, for files inside WSL, but it isn't a way to back up Windows: WSL stops when idle, so scheduled backups don't run unless something keeps it running, and files under `/mnt/c` are read without shadow copies or Windows metadata (ACLs, attributes, alternate data streams), so open files may fail and a restore to Windows loses that metadata.

## Ladderback or Backrest?

Ladderback borrows Backrest's user-facing model: repositories, plans, schedules, hooks, an operation log and restore into a new folder. The difference is the engine underneath. Backrest wraps the `restic` CLI. Ladderback links rustic's library, `rustic_core`, and inherits rustic's lock-free repository design.

**Ladderback may suit you better if you:**

- back up **several machines into one shared repository**. In Backrest, each plan runs `restic forget` after its backup, which takes an exclusive lock and blocks other hosts' backups ([backrest#244](https://github.com/garethgeorge/backrest/issues/244)). In Ladderback, backups never wait for a lock, and with the hub (M5) one owner per repository runs maintenance;
- don't want repository **passwords and backend keys copied to other machines**. Backrest's multi-host sync pushes shared repositories to clients, credentials included. with Ladderback's hub (M4), the hub keeps its own secrets, and agents never send theirs;
- back up **laptops**. Missed runs catch up by default, and the scheduler notices sleep and wake;
- want an **AI agent** to check on and run backups. Ladderback has an MCP server built in, `ladderback mcp`, which takes a token scoped to reading, running or previewing, and can't restore or delete;
- want **secrets out of the config file**: a password command, the system keyring, a file or an environment variable. A missing secret fails the job instead of silently becoming an empty string;
- want **time-based retention** (`keep-within-*`), not just counts;
- already use **rustic profiles** and want to keep using the CLI on the same repositories.

**Backrest is the better choice if you:**

- need **restores from the web UI** today. Backrest is mature; Ladderback's first usable release (M2, 0.2.0) runs scheduled backups, but its snapshot browser and restores come in M3, and until then you restore with the `rustic` CLI;
- use **Windows**;
- use **restic** and want to keep restic maintaining your repositories. Restic must never prune a repository Ladderback manages;
- need **multiple user accounts** or a way to run **arbitrary repository commands** from the UI. Ladderback has neither;
- prefer restic's longer track record over rustic, which is the younger project.

The [technical design](docs/design/README.md#relationship-to-backrest) has the full comparison, with sources.

## Install

One command installs the newest release. On 64-bit Linux (x86_64 or aarch64, glibc 2.35 or newer) it downloads the release's archive and checks it against the release's checksums; on macOS, which has no download yet, and elsewhere, it builds the release from source:

```sh
curl -fsSL https://raw.githubusercontent.com/nledford/Ladderback/main/scripts/install.sh | bash
```

To read the script before it runs, download it, read it, then run it:

```sh
curl -fsSL https://raw.githubusercontent.com/nledford/Ladderback/main/scripts/install.sh -o install.sh
less install.sh
bash install.sh
```

To build from source on Linux too, pass `--from-source`: `curl -fsSL … | bash -s -- --from-source`, or `bash install.sh --from-source`. A build asks before it installs any tool. The [install guide](docs/guide/install.md) says what the script checks and installs, how to download a release or build one by hand, how to update, how to run Ladderback as a service and what macOS asks for.

## Quick start

```sh
ladderback serve
```

Open the address it prints, `http://127.0.0.1:9898`, then:

1. **Connect a repository** on the Repositories page.
2. **Add a plan** to `config.toml` in Ladderback's config folder.
3. **Watch it run.** The start page lists the plan with its next run.

[A first plan](docs/guide/first-plan.md) walks through it with a whole `config.toml`. [Secrets](docs/guide/secrets.md) moves the repository's password out of that file, and [sign-in and remote access](docs/guide/remote-access.md) says how to set a password and reach Ladderback from another machine.

## Roadmap

The skeleton (M0), the backup engine (M1a), the single-machine MVP (M1b, released as [0.1.0](https://github.com/nledford/Ladderback/releases/tag/v0.1.0)) and schedules and errors (M2, released as [0.2.0](https://github.com/nledford/Ladderback/releases/tag/v0.2.0), with fixes from running it on real machines in [0.3.0](https://github.com/nledford/Ladderback/releases/tag/v0.3.0)) are in place: backups run on their schedules, with no restore in Ladderback yet. Each milestone is done when its acceptance scenarios pass, including keyboard-only and screen-reader checks for the UI. M2 is the first release worth using on your own machines; M3, to be released as 0.4.0, adds browsing and restoring snapshots.

- [x] **M0: Skeleton.** Workspace, CI, test tooling (acceptance scenarios, mutation testing, browser tests with accessibility checks), and an embedded web UI shell.
- [x] **M1a: Engine.** Backups through `rustic_core` in an isolated worker process, with live progress, warnings, cancellation and deadlines.
- [x] **M1b: Single-machine MVP.** Connect or create repositories; secrets; plans and hooks in `config.toml`; rustic profile import and export; manual backups with live progress; snapshot list; operation history; manual forget, prune and check.
- [x] **M2: Schedules and errors.** Scheduler with catch-up after sleep, time zones and retries; notifications to webhooks, ntfy and healthchecks; systemd and launchd service install; optional sign-in. **First usable release.**
- [ ] **M3: Browse, restore and automation.** Snapshot browser, restore into a new folder or in place, downloads as tar or zip, repository stats; scoped API tokens and an MCP server for AI agents; on macOS, builds from source that keep their Full Disk Access across upgrades.
- [ ] **M4: Hub, visibility.** Pair machines with a hub; one dashboard for all of them; offline alerts.
- [ ] **M5: Hub, control and maintenance.** Repository ownership; forget, prune and check managed from the hub; remote run and cancel; opt-in restores from the hub.
- [ ] **M6: Polish.** A run condition for metered networks (running only on AC power, and waiting for the network after a wake, came in 0.2.0), charts, Docker image, release packages, database rebuild, user docs.

Not planned for v1: Windows, multiple users, and rustic's `copy`, `merge`, `rewrite`, `repair`, `webdav` and `mount` commands (use the CLI).

## Documentation

- [docs/guide/](docs/guide/README.md): install, a first plan, secrets, and sign-in and remote access.
- [PRODUCT.md](PRODUCT.md): who Ladderback is for, what it does and doesn't do.
- [docs/design/](docs/design/README.md): architecture, safety invariants, data model, milestones and decisions.

## Contributing

Ladderback is built for its author's own machines first and published for other rustic users, without support promises. Issues and discussion about the design are welcome. Development is behaviour-first and test-first; see [docs/design/development.md §13](docs/design/development.md#13-development-method) for the method and [AGENTS.md](AGENTS.md) for the working rules. Setting up the browser journeys on Fedora is in [§13.2](docs/design/development.md#132-behaviour-first-bdd).

**Help test it.** Some setups haven't been tried on a real machine yet: other Linux distributions and desktops, keyrings, ARM, headless servers, Wi-Fi rules, WSL2. Each [issue labeled `needs real-world test`](https://github.com/nledford/Ladderback/issues?q=is%3Aissue%20is%3Aopen%20label%3A%22needs%20real-world%20test%22) lists the steps and what to report; use a scratch repository and a made-up password.

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or [MIT license](LICENSE-MIT), at your option.
