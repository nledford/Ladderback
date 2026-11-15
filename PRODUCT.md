# Ladderback

<!-- impeccable:product-schema 1 -->

Self-hosted backup manager for rustic: plans, schedules and one dashboard for every machine.

## Platform

web

## Users

People who back up their own machines. They might be on a desktop, a laptop or a headless server, and they keep one or more shared repositories. They know backups matter, but they don't want to babysit cron jobs, lock files and per-machine logs. Their job is to know every machine is backed up, notice quickly when one isn't, and get files back without fear.

They come in two kinds, and Ladderback serves both:

- **Casual users,** who want backups that run by themselves and rarely, if ever, open a terminal. They install Ladderback once and do everything else in the web UI.
- **Advanced users,** often already backing up with the [rustic](https://rustic.cli.rs) CLI, who are at home in a terminal. They expect a command for everything, scriptable, with their existing rustic profiles imported.

Ladderback is built for its author's own fleet first: Bazzite desktops, Linux servers and a MacBook. It is published so other self-hosters can use it, without support promises.

## Problem

Without Ladderback, each machine runs rustic from its own cron job or timer:

- Failures surface only if someone reads the logs.
- A laptop that was asleep at 02:00 silently skips its backup.
- Retention and pruning on a shared repository need hand-coordination between hosts.

Backrest solves the UI half, but it wraps restic. That brings two problems:

- Forgetting snapshots takes restic's exclusive lock and blocks other hosts' backups.
- Repository passwords are pushed to every peer.

## Product Purpose

Ladderback turns rustic into a managed service on each machine, with one place to see them all:

1. **Plans and schedules.** Define what to back up, from which machine, into which repository, how often, and how long to keep it.
2. **One place to see every machine.** Status, live progress, history, warnings and errors for every connected computer.
3. **Safe maintenance on shared repositories.** Backups never wait for a lock. Exactly one place runs forget, prune and check for each repository.
4. **No lock-in.** Import existing rustic profiles, and export any plan back to one, so the `rustic` CLI always remains an option.
5. **One self-contained program.** It runs headless on servers and as a user service on desktops and laptops, with the web UI built in.
6. **Help from an AI agent, confirmed by a person.** People can ask an AI agent to help them work out what to back up, where and for how long. The AI agent proposes the setup; Ladderback shows in its own words what would change, what it could delete and every command it would add, and with sign-in on, nothing changes until a person has reviewed every change and confirmed it in the web UI. With sign-in off, anyone who can reach Ladderback, an AI agent included, can confirm, and Ladderback says so.
7. **The web UI and the terminal, both.** Every routine task works from either. Installing takes one command; after that, nothing a casual user needs (setting the admin password, restarting, updating, reading why a backup failed) requires the terminal. The commands stay, for advanced users and for scripts.

Success means: every machine's last good backup is visible at a glance, and a missed or failed backup is reported, not discovered during a restore.

## Positioning

"Backrest, but built on rustic_core." Ladderback links rustic's library instead of shelling out to a CLI, so it inherits rustic's lock-free repository design: many hosts back up while one owner prunes. Missed runs on sleeping laptops catch up by default. The hub never receives agents' secrets.

## Brand Personality

Calm and exact.

### Tone

- Plain sentences, no alarmism.
- Every warning says what happened and what to do next.
- Times, sizes and counts are precise and carry units.
- A failed backup is stated plainly once, not repeated in red everywhere.
- Backup terms (snapshot, repository, retention) are used consistently and never dumbed down.

## Boundaries

- Linux and macOS only. Windows is out of scope; WSL2 runs it as a Linux install for files inside WSL, never as a way to back up Windows ([decision 4](docs/design/decisions/04-windows.md)).
- One owner and their machines: no multiple users, no multi-tenant SaaS.
- Not a general rustic GUI. `copy`, `merge`, `rewrite`, `repair`, `webdav` and `mount` stay CLI-only in v1.
- Not a manager for restic repositories that restic also maintains. Restic must never prune a repository Ladderback manages.
- No mobile app. The web UI is responsive.
- English (`en-US`) only in v1. Every user-facing string is localizable, so a language can be added without touching application code.

## Accessibility & Inclusion

WCAG 2.2 AA:

- every action is reachable by keyboard;
- text and controls meet contrast minimums;
- every control has an accessible name;
- status (success, warning, failure) is never shown by color alone;
- live changes (progress, finished backups) reach screen readers without flooding them.

Supported assistive technologies: Orca on Linux, VoiceOver on macOS, and keyboard-only use. Accessibility is a release requirement, not a later polish pass.

## Offer

Free and open source under MIT OR Apache-2.0.

## Evidence on Hand

Released (0.1.0–0.3.1) and running on the author's own machines. There are no outside users, testimonials, benchmarks or published screenshots, and none may be invented.

## Stack

- Rust, built on `rustic_core`. The web UI is Leptos (client-side rendered), embedded in the binary.
- [docs/design/](docs/design/README.md): architecture, safety invariants, data model, milestones and decisions.
- [DESIGN.md](DESIGN.md): the web UI's visual design system, its tokens taken from `crates/web/style.css`.
