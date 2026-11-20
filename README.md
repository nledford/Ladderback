<h1>
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="crates/web/assets/brand/logo-dark.svg">
    <img src="crates/web/assets/brand/logo.svg" alt="Ladderback" width="380" height="80">
  </picture>
</h1>

[![CI](https://github.com/nledford/Ladderback/actions/workflows/ci.yml/badge.svg?branch=main&event=push)](https://github.com/nledford/Ladderback/actions/workflows/ci.yml?query=branch%3Amain+event%3Apush)
[![MSRV](https://img.shields.io/badge/dynamic/toml?url=https%3A%2F%2Fraw.githubusercontent.com%2Fnledford%2FLadderback%2Fmain%2FCargo.toml&query=%24.workspace.package%5B%27rust-version%27%5D&label=MSRV&color=orange)](Cargo.toml)
[![Latest release](https://img.shields.io/github/v/release/nledford/Ladderback?sort=semver&label=release)](https://github.com/nledford/Ladderback/releases/latest)
[![Commits since the latest release](https://img.shields.io/github/commits-since/nledford/Ladderback/latest?sort=semver)](https://github.com/nledford/Ladderback/commits/main)
[![Platforms: Linux and macOS](https://img.shields.io/badge/platforms-Linux%20%7C%20macOS-blue)](#what-it-does)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue)](#license)

A self-hosted backup manager for [rustic](https://rustic.cli.rs): plans, schedules and one dashboard for every machine.

> [!WARNING]
> **Ladderback 0.5.0 runs scheduled backups and restores from them in its web UI, but its restores arrived only in 0.4.0.** Try one into a scratch folder before you rely on these backups. The `rustic` CLI restores from any repository Ladderback backs up to, and stays your fallback. AI agents can propose plans, over the API or through MCP, but not yet repositories, hooks or maintenance. Until the hub (M8), each machine runs its own Ladderback with its own web UI. The [roadmap](#roadmap) has the order.

## What it does

Without a manager, each machine runs rustic from its own cron job or timer. Failures surface only if someone reads the logs, a laptop that was asleep at 02:00 silently skips its backup, and retention on a shared repository needs hand-coordination between hosts. Ladderback turns rustic into a managed service on each machine, with one place to see them all.

**Today**, each machine runs its own Ladderback, with the web UI built in:

- **Plans and schedules.** What to back up, from which machine, into which repository, how often, and how long to keep it, written in `config.toml` or created, edited and deleted on the Plans page. Missed runs on sleeping laptops catch up at the next wake, and notifications go to webhooks, ntfy and healthchecks.io.
- **Browse, restore and download.** Browse a snapshot's folders and files in an accessible tree. Restore a file or folder into a new folder by default. Restore in place, over the original files, only from the machine's own web UI, after a dry-run summary of what would change. Restores and downloads need a confirmation: the admin password, or the snapshot's short ID when sign-in is off. Download a file as itself, or a folder as a tar or zip archive. The Snapshots page also shows each repository's stats.
- **Safe maintenance on shared repositories.** Backups never wait for a lock. One machine you choose maintains each repository, and Ladderback warns when it sees snapshots from other hosts in it.
- **No lock-in.** Import existing rustic profiles and export any plan back to one, so the `rustic` CLI always stays an option.
- **One self-contained program.** It runs headless on servers and as a user service on desktops and laptops. Its web UI is drawn by one shared, keyboard-tested component set, accessible (WCAG 2.2 AA) and localization-ready, with Settings in tabs and a choice of a narrow or a full-width page.
- **Scriptable by AI agents.** `ladderback mcp` is an MCP server over stdio. An agent can read a status summary, with overdue plans and unreadable repositories, and plans, repositories, snapshots and history; run a plan and wait for it, cancel a backup, test a repository, preview a forget or a profile import, and export a plan as a profile. It drives a running instance, with a scoped API token when sign-in is on; a headless session of its own when none is running; or a throwaway scratch setup to try it on. Driving a running instance, it can also count what a draft plan would back up within the folders you allow, and propose plan changes. It can't restore, delete or change the configuration. `ladderback mcp call` runs the same tools from a shell or a script without an MCP client, printing each result as a line of JSON.
- **Plan changes proposed by AI agents.** An agent whose API token has the `propose` scope can propose adding, changing or removing plans, over the REST API or through an MCP server that drives a running instance, and nothing changes until a person approves it on the Pending changes page, with the same confirmation as a restore. A proposal that could delete snapshots shows what it would forget, before and after, and is approved from that preview. Proposals can't yet touch repositories, hooks or maintenance.

**Later** (see the [roadmap](#roadmap)):

- **The rest of AI agents (M4).** The MCP tools people ask agents for most, such as comparing snapshots and reading the log; an AI agents tab in Settings that limits what agents may do; proposals for repositories, hooks and maintenance.
- **Prune and check on a schedule (M5).** Forget, prune and check run on their own, each check's result kept and shown.
- **More notification channels (M6).** Gotify, Discord, Slack, email, Telegram, Pushover, Pushbullet and web push.
- **Japanese (M7).** The first language after English.
- **One dashboard for every machine (M8).** Status, live progress, history, warnings and errors for every connected computer.
- **One owner for each repository (M9).** Exactly one place runs forget, prune and check for each repository, whichever machines back up to it.

Ladderback supports **Linux and macOS**. Linux means 64-bit (x86_64 or aarch64) with glibc 2.35 or newer: 32-bit ARM and musl distributions such as Alpine aren't supported. Windows is out of scope. It runs inside WSL2 as a Linux install, for files inside WSL, but it isn't a way to back up Windows: WSL stops when idle, so scheduled backups don't run unless something keeps it running, and files under `/mnt/c` are read without shadow copies or Windows metadata (ACLs, attributes, alternate data streams), so open files may fail and a restore to Windows loses that metadata.

## Ladderback or Backrest?

Ladderback borrows Backrest's user-facing model: repositories, plans, schedules, hooks, an operation log and restore into a new folder. The difference is the engine underneath. Backrest wraps the `restic` CLI. Ladderback links rustic's library, `rustic_core`, and inherits rustic's lock-free repository design.

**Ladderback may suit you better if you:**

- back up **several machines into one shared repository**. In Backrest, each plan runs `restic forget` after its backup, which takes an exclusive lock and blocks other hosts' backups ([backrest#244](https://github.com/garethgeorge/backrest/issues/244)). In Ladderback, backups never wait for a lock, and with the hub (M9) one owner per repository runs maintenance;
- don't want repository **passwords and backend keys copied to other machines**. Backrest's multi-host sync pushes shared repositories to clients, credentials included. With Ladderback's hub (M8), the hub keeps its own secrets, and agents never send theirs;
- back up **laptops**. Missed runs catch up by default, and the scheduler notices sleep and wake;
- want an **AI agent** to check on and run backups. Ladderback has an MCP server built in, `ladderback mcp`, which takes a token scoped to reading, running or previewing, and can't restore or delete. An agent can also propose plans, over the API or through MCP, which change nothing until a person approves them in the web UI;
- want **secrets out of the config file**: a password command, the system keyring, a file or an environment variable. A missing secret fails the job instead of silently becoming an empty string;
- want **time-based retention** (`keep-within-*`), not just counts;
- already use **rustic profiles** and want to keep using the CLI on the same repositories;
- want **restores that don't overwrite by accident**. A restore goes into a new folder by default, and one over the original files is offered only from the machine's own web UI, behind a dry-run summary and a confirmation.

**Backrest is the better choice if you:**

- want the **most proven restores**. Backrest is mature. Ladderback's web UI restores a file or folder, and downloads one as a file, a tar or a zip, but these arrived only in 0.4.0 (M3), and some cases are still open, such as an in-place restore over a path that has since become a folder or a symlink;
- use **Windows**;
- use **restic** and want to keep restic maintaining your repositories. Restic must never prune a repository Ladderback manages;
- need **multiple user accounts** or a way to run **arbitrary repository commands** from the UI. Ladderback has neither;
- prefer restic's longer track record over rustic, which is the younger project.

The [technical design](docs/design/README.md#relationship-to-backrest) has the full comparison, with sources.

## Install

Ladderback runs on each machine it backs up, and each system has one recommended way to install it ([decision 126](docs/design/decisions/126-primary-install-channels.md)). The [install guide](docs/guide/install.md) has the details: what each way checks and installs, how to update, how to run Ladderback as a service and what macOS asks for.

### Debian, Ubuntu, Raspberry Pi OS and Fedora: the package

From 0.5.0, each release has a `.deb` for Debian 12 or newer, Ubuntu 22.04 or newer and 64-bit Raspberry Pi OS, and an `.rpm` for Fedora, for x86_64 and aarch64. The package puts `ladderback` in `/usr/bin` and starts nothing. Download yours from the [release](https://github.com/nledford/Ladderback/releases/latest) with its `SHA256SUMS`, check it, install it, then install the service as yourself:

```sh
sha256sum -c --ignore-missing SHA256SUMS
sudo apt install ./ladderback_<version>_arm64.deb          # amd64 on x86_64
sudo dnf install ./ladderback-<version>-1.x86_64.rpm       # aarch64 on ARM
ladderback install-service
```

The packages are unsigned and have no apt or dnf repository yet, so to update, install the next release's package and run `ladderback install-service` again. To move an earlier install, remove its binary first (`~/.cargo/bin/ladderback` from the script or a build, `~/.local/bin/ladderback` from the archive by hand), install the package, then run `ladderback install-service` again, so the service runs `/usr/bin/ladderback`. The [install guide](docs/guide/install.md#install-a-package-debian-ubuntu-fedora) has the details.

### Other 64-bit Linux: the install script

On any other 64-bit Linux (x86_64 or aarch64) with glibc 2.35 or newer, one command downloads the newest release's archive, checks it against the release's checksums and installs `ladderback` into `~/.cargo/bin`:

```sh
curl -fsSL https://raw.githubusercontent.com/nledford/Ladderback/main/scripts/install.sh | bash
```

To read the script before it runs, download it, read it, then run it:

```sh
curl -fsSL https://raw.githubusercontent.com/nledford/Ladderback/main/scripts/install.sh -o install.sh
less install.sh
bash install.sh
```

Then `ladderback install-service` runs it in the background, from each login on.

### macOS: the install script, signed with your own certificate

macOS has no download yet ([#344](https://github.com/nledford/Ladderback/issues/344)), so on a Mac the same script builds the newest release from source, and asks before it installs any tool the build needs. Make a self-signed code-signing certificate in your keychain once, as the install guide's [Signing your build](docs/guide/install.md#signing-your-build) says, and name it in `LADDERBACK_SIGN_IDENTITY`, so that the build's Full Disk Access and Photos grants carry over to the next update:

```sh
curl -fsSL https://raw.githubusercontent.com/nledford/Ladderback/main/scripts/install.sh | LADDERBACK_SIGN_IDENTITY="Ladderback Code Signing" bash
```

Without the certificate the build is signed ad hoc, and after each update macOS asks again for Full Disk Access and Photos: until you grant them again, backups can't read the folders they protect. Then run `ladderback install-service` and [grant Full Disk Access](docs/guide/install.md#macos-full-disk-access).

### Other ways

For contributors, for installing the release's archive by hand, and for people who already use cargo-binstall:

- **From a checkout:** `just install`, or the install guide's steps [by hand](docs/guide/install.md#by-hand), to build a release or a branch yourself. On Linux, `bash install.sh --from-source` has the script build from source too.
- **The release's archive by hand,** on 64-bit Linux: [download a release](docs/guide/install.md#download-a-release-linux).
- **cargo-binstall,** on 64-bit Linux: `cargo binstall --strategies crate-meta-data --git https://github.com/nledford/Ladderback ladderback` installs the release's archive, without checking it against `SHA256SUMS` ([details](docs/guide/install.md#cargo-binstall)).

Ladderback isn't published to crates.io, and has no Homebrew tap until a macOS build can be signed and notarized ([#344](https://github.com/nledford/Ladderback/issues/344)).

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

The skeleton (M0), the backup engine (M1a), the single-machine MVP (M1b, released as [0.1.0](https://github.com/nledford/Ladderback/releases/tag/v0.1.0)), schedules and errors (M2, released as [0.2.0](https://github.com/nledford/Ladderback/releases/tag/v0.2.0), with fixes from running it on real machines in [0.3.0](https://github.com/nledford/Ladderback/releases/tag/v0.3.0)), browsing, restoring and automation (M3, released as [0.4.0](https://github.com/nledford/Ladderback/releases/tag/v0.4.0)) and the shared UI components (M3b, released as [0.5.0](https://github.com/nledford/Ladderback/releases/tag/v0.5.0)) are in place: backups run on their schedules, and snapshots can be browsed, restored and downloaded in the web UI. Each milestone is done when its acceptance scenarios pass, including keyboard-only and screen-reader checks for the UI. M2 is the first release worth using on your own machines; M3 adds restores, so it is the first that can bring files back without the `rustic` CLI.

- [x] **M0: Skeleton.** Workspace, CI, test tooling (acceptance scenarios, mutation testing, browser tests with accessibility checks), and an embedded web UI shell.
- [x] **M1a: Engine.** Backups through `rustic_core` in an isolated worker process, with live progress, warnings, cancellation and deadlines.
- [x] **M1b: Single-machine MVP.** Connect or create repositories; secrets; plans and hooks in `config.toml`; rustic profile import and export; manual backups with live progress; snapshot list; operation history; manual forget, prune and check.
- [x] **M2: Schedules and errors.** Scheduler with catch-up after sleep, time zones and retries; notifications to webhooks, ntfy and healthchecks; systemd and launchd service install; optional sign-in. **First usable release.**
- [x] **M3: Browse, restore and automation.** Snapshot browser; restore into a new folder or in place; downloads as tar or zip; repository stats; scoped API tokens; an MCP server for AI agents; on macOS, builds from source that keep their Full Disk Access across upgrades. **First release with restores in the web UI.** Open after M3: small restore fixes ([#814](https://github.com/nledford/Ladderback/issues/814), [#818](https://github.com/nledford/Ladderback/issues/818)), updating in place from the web UI ([#1080](https://github.com/nledford/Ladderback/issues/1080), M8) and reading listings through the repository's cache ([#163](https://github.com/nledford/Ladderback/issues/163), M8). The MCP tools for proposals, a dry run of a draft plan and a forget preview for proposals followed in 0.5.0.
- [x] **M3b: Shared UI components.** One accessible component set for every widget the pages use, the pages moved onto it, a full-width page option, and Settings in tabs. Released as 0.5.0, with parts of later milestones that were ready: the MCP tools for proposals, a dry run of a draft plan and approving a proposal that could delete snapshots from its forget preview (M4), and `.deb` and `.rpm` packages (M10). Open after M3b: moving the rest of the custom CSS onto Tailwind utilities ([#931](https://github.com/nledford/Ladderback/issues/931)) and option cards for the checkbox and the radio group ([#1019](https://github.com/nledford/Ladderback/issues/1019)).
- [ ] **M4: AI agents.** MCP tools for what people ask agents most, proposals for repositories, hooks and maintenance, and an AI agents tab in Settings.
- [ ] **M5: Scheduled maintenance.** Forget, prune and check on their schedules, one at a time by default, each check's result kept and shown.
- [ ] **M6: Notification channels.** Gotify, Discord, Slack, email, Telegram, Pushover, Pushbullet, web push and a webhook you template.
- [ ] **M7: Japanese.** Japanese, with a language choice in the web UI; released once its review is done.
- [ ] **M8: Hub, visibility.** Pair machines with a hub; one dashboard for all of them; offline alerts.
- [ ] **M9: Hub, control and maintenance.** Repository ownership; forget, prune and check managed from the hub; remote run and cancel; opt-in restores from the hub.
- [ ] **M10: Polish.** A run condition for metered networks (running only on AC power, and waiting for the network after a wake, came in 0.2.0), charts, Docker image, database rebuild, user docs (the `.deb` and `.rpm` packages came in 0.5.0).

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
