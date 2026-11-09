# Steading

[![CI](https://github.com/nledford/Ladderback/actions/workflows/ci.yml/badge.svg?branch=main&event=push)](https://github.com/nledford/Ladderback/actions/workflows/ci.yml?query=branch%3Amain+event%3Apush)
[![MSRV](https://img.shields.io/badge/dynamic/toml?url=https%3A%2F%2Fraw.githubusercontent.com%2Fnledford%2FLadderback%2Fmain%2FCargo.toml&query=%24.workspace.package%5B%27rust-version%27%5D&label=MSRV&color=orange)](Cargo.toml)

A self-hosted backup manager for [rustic](https://rustic.cli.rs): plans, schedules and one dashboard for every machine.

> [!WARNING]
> **Steading is early in development and can't make backups yet.** Don't use it for backups until the [roadmap](#roadmap) shows a usable release. Until then, use the `rustic` CLI directly or [Backrest](https://github.com/garethgeorge/backrest).

## What it will do

Without a manager, each machine runs rustic from its own cron job or timer. Failures surface only if someone reads the logs, a laptop that was asleep at 02:00 silently skips its backup, and retention on a shared repository needs hand-coordination between hosts. Steading turns rustic into a managed service on each machine, with one place to see them all:

- **Plans and schedules.** What to back up, from which machine, into which repository, how often, and how long to keep it. Missed runs on sleeping laptops catch up at the next wake.
- **One dashboard for every machine.** Status, live progress, history, warnings and errors for every connected computer, with notifications (webhook, ntfy, Gotify, healthchecks.io, email).
- **Safe maintenance on shared repositories.** Backups never wait for a lock, and exactly one place runs forget, prune and check for each repository.
- **No lock-in.** Import existing rustic profiles and export any plan back to one, so the `rustic` CLI always stays an option.
- **Careful restores.** Restore into a new folder by default; in-place restore only from the machine's own UI, after a dry-run summary.
- **Scriptable by AI agents.** `steading mcp` is an MCP server over stdio. It drives a running instance, or a headless session of its own, through read, run and preview tools. It can't restore, delete or change configuration.
- **One self-contained program.** It runs headless on servers and as a user service on desktops and laptops, with the web UI built in. It is accessible (WCAG 2.2 AA) and localization-ready.

Steading supports **Linux and macOS**. Windows is out of scope.

## Steading or Backrest?

Steading borrows Backrest's user-facing model: repositories, plans, schedules, hooks, an operation log and restore into a new folder. The difference is the engine underneath. Backrest wraps the `restic` CLI. Steading links rustic's library, `rustic_core`, and inherits rustic's lock-free repository design.

**Steading may suit you better if you:**

- back up **several machines into one shared repository**. In Backrest, each plan runs `restic forget` after its backup, which takes an exclusive lock and blocks other hosts' backups ([backrest#244](https://github.com/garethgeorge/backrest/issues/244)). In Steading, backups never wait for a lock, and one owner per repository runs maintenance;
- don't want repository **passwords and backend keys copied to other machines**. Backrest's multi-host sync pushes shared repositories to clients, credentials included. Steading's hub keeps its own secrets, and agents never send theirs;
- back up **laptops**. Missed runs catch up by default, and the scheduler notices sleep and wake;
- want **secrets out of the config file**: a password command, the system keyring, a file or an environment variable. A missing secret fails the job instead of silently becoming an empty string;
- want **time-based retention** (`keep-within-*`), not just counts;
- already use **rustic profiles** and want to keep using the CLI on the same repositories.

**Backrest is the better choice if you:**

- need something that **works today**. Backrest is released; Steading is a design;
- use **Windows**;
- use **restic** and want to keep restic maintaining your repositories. Restic must never prune a repository Steading manages;
- need **multiple user accounts** or a way to run **arbitrary repository commands** from the UI. Steading has neither;
- prefer restic's longer track record over rustic, which is the younger project.

The [technical design](docs/design/README.md#relationship-to-backrest) has the full comparison, with sources.

## Roadmap

The skeleton (M0) and the backup engine (M1a) are in place; there is no way to configure or run backups yet. Each milestone is done when its acceptance scenarios pass, including keyboard-only and screen-reader checks for the UI. Milestone **M2** is the first release worth using on your own machines.

- [x] **M0: Skeleton.** Workspace, CI, test tooling (acceptance scenarios, mutation testing, browser tests with accessibility checks), and an embedded web UI shell.
- [x] **M1a: Engine.** Backups through `rustic_core` in an isolated worker process, with live progress, warnings, cancellation and deadlines.
- [ ] **M1b: Single-machine MVP.** Connect or create repositories; secrets; plans and hooks in `config.toml`; rustic profile import and export; manual backups with live progress; snapshot list; operation history; manual forget, prune and check.
- [ ] **M2: Schedules and errors.** Scheduler with catch-up after sleep, time zones and retries; notifications; systemd and launchd service install; optional login; signed macOS builds. **First usable release.**
- [ ] **M3: Browse and restore.** Snapshot browser, restore into a new folder or in place, downloads as tar or zip, repository stats.
- [ ] **M4a: Hub, visibility.** Pair machines with a hub; one dashboard for all of them; offline alerts.
- [ ] **M4b: Hub, control and maintenance.** Repository ownership; forget, prune and check managed from the hub; remote run and cancel; opt-in restores from the hub.
- [ ] **M5: Polish.** Run conditions (AC power, metered networks), charts, Docker image, release packages, database rebuild, user docs.

Not planned for v1: Windows, multiple users, and rustic's `copy`, `merge`, `rewrite`, `repair`, `webdav` and `mount` commands (use the CLI).

## Documentation

- [PRODUCT.md](PRODUCT.md): who Steading is for, what it does and doesn't do.
- [docs/design/](docs/design/README.md): architecture, safety invariants, data model, milestones and decisions.

## Contributing

Steading is built for its author's own machines first and published for other rustic users, without support promises. Issues and discussion about the design are welcome. Development is behaviour-first and test-first; see [docs/design/development.md §13](docs/design/development.md#13-development-method) for the method and [AGENTS.md](AGENTS.md) for the working rules.

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or [MIT license](LICENSE-MIT), at your option.
