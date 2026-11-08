# Steading

<!-- impeccable:product-schema 1 -->

Self-hosted backup manager for rustic: plans, schedules and one dashboard for every machine.

## Register

product

## Platform

web

## Users

People who back up their own machines with [rustic](https://rustic.cli.rs). They might be on a desktop, a laptop or a headless server, and they keep one or more shared repositories. They know backups matter, but they don't want to babysit cron jobs, lock files and per-machine logs. Their job is to know every machine is backed up, notice quickly when one isn't, and get files back without fear.

Steading is built for its author's own fleet first: Bazzite desktops, Linux servers and a MacBook. It is published so other rustic self-hosters can use it, without support promises.

## Problem

Without Steading, each machine runs rustic from its own cron job or timer:

- Failures surface only if someone reads the logs.
- A laptop that was asleep at 02:00 silently skips its backup.
- Retention and pruning on a shared repository need hand-coordination between hosts.

Backrest solves the UI half, but it wraps restic. That brings two problems:

- Forgetting snapshots takes restic's exclusive lock and blocks other hosts' backups.
- Repository passwords are pushed to every peer.

## Product Purpose

Steading turns rustic into a managed service on each machine, with one place to see them all:

1. **Plans and schedules.** Define what to back up, from which machine, into which repository, how often, and how long to keep it.
2. **One place to see every machine.** Status, live progress, history, warnings and errors for every connected computer.
3. **Safe maintenance on shared repositories.** Backups never wait for a lock. Exactly one place runs forget, prune and check for each repository.
4. **No lock-in.** Import existing rustic profiles, and export any plan back to one, so the `rustic` CLI always remains an option.
5. **One self-contained program.** It runs headless on servers and as a user service on desktops and laptops, with the web UI built in.

Success means: every machine's last good backup is visible at a glance, and a missed or failed backup is reported, not discovered during a restore.

## Positioning

"Backrest, but built on rustic_core." Steading links rustic's library instead of shelling out to a CLI, so it inherits rustic's lock-free repository design: many hosts back up while one owner prunes. Missed runs on sleeping laptops catch up by default. The hub never receives agents' secrets.

## Brand Personality

Calm and exact.

### Tone

- Plain sentences, no alarmism.
- Every warning says what happened and what to do next.
- Times, sizes and counts are precise and carry units.
- A failed backup is stated plainly once, not repeated in red everywhere.
- Backup terms (snapshot, repository, retention) are used consistently and never dumbed down.

## Boundaries

- Linux and macOS only. Windows is out of scope.
- One owner and their machines: no multiple users, no multi-tenant SaaS.
- Not a general rustic GUI. `copy`, `merge`, `rewrite`, `repair`, `webdav` and `mount` stay CLI-only in v1.
- Not a manager for restic repositories that restic also maintains. Restic must never prune a repository Steading manages.
- No mobile app. The web UI is responsive.

## Accessibility & Inclusion

WCAG 2.2 AA:

- every action is reachable by keyboard;
- text and controls meet contrast minimums;
- every control has an accessible name;
- status (success, warning, failure) is never shown by color alone.

## Offer

Free and open source under MIT OR Apache-2.0.

## Evidence on Hand

None yet: Steading is in the design stage. There are no users, screenshots, testimonials or benchmarks, and none may be invented.

## Stack

- Rust, built on `rustic_core`. The web UI is Leptos (client-side rendered), embedded in the binary.
- [docs/TECHNICAL-DESIGN.md](docs/TECHNICAL-DESIGN.md): architecture, safety invariants, data model, milestones and decisions.
- `DESIGN.md` (visual design system): not written yet. It will be recorded once the first UI screen exists.
