# Ladderback — Technical design

> **Status:** Draft v0.6 · 2026-11-08
> **Name:** Ladderback (binary `ladderback`). A ladderback is a rustic chair style, a nod to Backrest. Ladderback is inspired by Backrest, never derived from it: Backrest is GPL-3.0, this project is MIT OR Apache-2.0.

A self-hosted backup manager for [rustic](https://rustic.cli.rs). It provides a web UI for connecting repositories, defining backup plans and schedules, and watching backup status and errors across several computers. Think "[Backrest](https://github.com/garethgeorge/backrest), but built on `rustic_core`". It is written from scratch in Rust, and the frontend is written in Leptos.

## Contents

The design is split by topic. A `§N` or `§N.N` mentioned anywhere in the repository points at the heading with that number below.

- [0. Invariants](#0-invariants)
  - [Safety invariants](#safety-invariants)
  - [Accessibility invariants](#accessibility-invariants)
- [1. Scope](#1-scope)
  - [Relationship to Backrest](#relationship-to-backrest)
- [2. Background: what we build on](background.md#2-background-what-we-build-on)
  - [2.1 `rustic_core` (library, Apache-2.0 OR MIT)](background.md#21-rustic_core-library-apache-20-or-mit)
  - [2.2 Why lock-free matters](background.md#22-why-lock-free-matters)
  - [2.3 Leptos](background.md#23-leptos)
- [3. Architecture](architecture.md#3-architecture)
  - [3.0 Runtime topology](architecture.md#30-runtime-topology)
  - [3.1 Domain model (DDD)](architecture.md#31-domain-model-ddd)
  - [3.2 Code architecture: hexagonal, with a DDD core](architecture.md#32-code-architecture-hexagonal-with-a-ddd-core)
- [4. Components](components.md#4-components)
  - [4.1 Worker process (the engine boundary)](components.md#41-worker-process-the-engine-boundary)
  - [4.2 Daemon (agent)](components.md#42-daemon-agent)
  - [4.3 Hub](components.md#43-hub)
  - [4.4 Web UI (Leptos CSR)](components.md#44-web-ui-leptos-csr)
  - [4.5 Accessibility](components.md#45-accessibility)
  - [4.6 MCP server](components.md#46-mcp-server)
- [5. Scheduling](scheduling-and-config.md#5-scheduling)
- [6. Configuration](scheduling-and-config.md#6-configuration)
  - [6.1 Source of truth](scheduling-and-config.md#61-source-of-truth)
  - [6.2 Importing rustic profiles](scheduling-and-config.md#62-importing-rustic-profiles)
- [7. Multiple computers](multi-machine.md#7-multiple-computers)
  - [7.1 Pairing and identity](multi-machine.md#71-pairing-and-identity)
  - [7.2 Sync protocol (`/sync/v1`, WebSocket, JSON, versioned)](multi-machine.md#72-sync-protocol-syncv1-websocket-json-versioned)
  - [7.3 Repository maintenance: exactly one owner](multi-machine.md#73-repository-maintenance-exactly-one-owner)
- [8. Technology choices](technology.md#8-technology-choices)
  - [8.1 Frontend: Leptos CSR with an explicit REST API](technology.md#81-frontend-leptos-csr-with-an-explicit-rest-api)
  - [8.2 Backend crates](technology.md#82-backend-crates)
  - [8.3 Workspace layout](technology.md#83-workspace-layout)
  - [8.4 Internationalization and localization (Project Fluent)](technology.md#84-internationalization-and-localization-project-fluent)
- [9. Data model (SQLite)](data-model.md#9-data-model-sqlite)
- [10. Security](security.md#10-security)
  - [10.1 Web UI](security.md#101-web-ui)
  - [10.2 Repository secrets](security.md#102-repository-secrets)
  - [10.3 Agent ↔ hub](security.md#103-agent--hub)
  - [10.4 Restores](security.md#104-restores)
  - [10.5 MCP server](security.md#105-mcp-server)
  - [10.6 Repository locations](security.md#106-repository-locations)
- [11. Notifications](operations.md#11-notifications)
- [12. Packaging and deployment](operations.md#12-packaging-and-deployment)
  - [12.1 Versioning and releases](operations.md#121-versioning-and-releases)
- [13. Development method](development.md#13-development-method)
  - [13.1 Test first, always](development.md#131-test-first-always)
  - [13.2 Behaviour first (BDD)](development.md#132-behaviour-first-bdd)
  - [13.3 Test levels by ring](development.md#133-test-levels-by-ring)
  - [13.4 Definition of done](development.md#134-definition-of-done)
  - [13.5 Robustness rules](development.md#135-robustness-rules)
  - [13.6 Dependency checks](development.md#136-dependency-checks)
  - [13.7 Changes that touch several places](development.md#137-changes-that-touch-several-places)
  - [13.8 Pull requests and merging](development.md#138-pull-requests-and-merging)
  - [13.9 Dependencies, licence and fixtures](development.md#139-dependencies-licence-and-fixtures)
  - [13.10 Keeping AGENTS.md useful](development.md#1310-keeping-agentsmd-useful)
  - [13.11 Build profiles](development.md#1311-build-profiles)
  - [13.12 The Rust toolchain](development.md#1312-the-rust-toolchain)
  - [13.13 Formatting](development.md#1313-formatting)
- [14. Milestones](roadmap.md#14-milestones)
- [15. Risks](roadmap.md#15-risks)
- [16. Decisions](roadmap.md#16-decisions): the index; each decision is its own file in [decisions/](decisions/README.md)
- [17. Open questions](roadmap.md#17-open-questions)

---

## 0. Invariants

Every later section serves these. Each one has a single enforcing component and a test that proves it.

### Safety invariants

| # | Invariant | Enforced by | Verified by |
|---|---|---|---|
| I1 | Among agents registered to a hub, each repository has at most one maintenance owner, keyed by its rustic repository ID. Ownership is a lease, so a lost or revoked owner stops on its own. A standalone agent can't rule out a second standalone owner and warns instead. | Hub ownership registry with leases; standalone warning (§7.3) | Two agents request `local` for one repo: the second is rejected; an expired lease stops scheduled maintenance |
| I2 | Prune never uses `instant-delete` or `early-delete-index`. | `engine` builds `PruneOptions` itself; no config key exposes either flag (§7.3) | Unit test on the mapping; a config containing either key fails to load |
| I3 | No Ladderback backup writes a snapshot after its deadline (index read start + `keep-delete` − prune margin), except a snapshot write already in progress at the deadline, which the margin covers. | `Deadline` rule in `domain`; stop decision in `app`; worker watchdog, which refuses every snapshot write from when it notices the deadline (when it comes, or within one `watchdog_interval` of waking for a deadline that passed during a suspend, a gap the margin covers too) and ends the worker, once a backup that archives has wound down (decision 55); the daemon's stop at the deadline, with its grace period; backed by the owner's prune guard (§7.3) | Fake-clock tests of the rule and the stop decision; unit tests of the refusal, the wind-down and its 30 s limit; one `@e2e` scenario with a real worker, a short deadline and a scratch repo asserting no snapshot is written, and one where the deadline passes mid-backup and a check finds nothing to warn about |
| I4 | Resolved secrets never appear in argv, worker or hook environments, logs, hook templates, sync messages or the UI. One exception: a repository password Ladderback generates on a proposal's confirmation page is shown exactly once, in the response to that page's Generate request; it is held only in the daemon's memory until approval stores it in its keyring entry, and is never stored anywhere else, logged, sent as an event or readable again (§16, decision NN). | `JobSpec` on stdin, scrubbed environments and the `Redactor` (§3.1, §10.2) | A canary secret is absent from every log, event and child environment; a canary generated password appears in its one response and in no log, event, operation or proposal record, configuration or its history, MCP result or later response |
| I5 | Destructive restic commands (`prune`, `forget`, `rewrite`, `repair`) are never run against a Ladderback-maintained repo, and the UI says so. | Repo page warning; Ladderback never invokes restic (§7.3) | Doc and UI review |
| I6 | Forget, run by any owner, only deletes snapshots that a plan owns, by tag, or that the user explicitly adopted or selected. | Forget algorithm (§7.3) | Untagged snapshots survive a forget run |
| I7 | Maintenance operations on one repository run one at a time. | `OperationStore::try_start`, backed by a partial unique index (§9) | Store contract test: a second maintenance start on the same repo is refused; `maintenance/serialization.feature` |

### Accessibility invariants

The web UI meets **WCAG 2.2 AA** ([PRODUCT.md](../../PRODUCT.md)). These rules bind like I1–I7: a change that breaks one doesn't merge. §4.5 is the design that delivers them.

| # | Invariant | Enforced by | Verified by |
|---|---|---|---|
| A1 | Every workflow can be completed with the keyboard alone, in a logical focus order. Focus is always visible, never hidden behind sticky UI, and never trapped (WCAG 2.1.1, 2.1.2, 2.4.3, 2.4.7, 2.4.11). | Native interactive elements and the shared component set (§4.5) | Keyboard-only Playwright journeys through a helper with no pointer actions, which check focus visibility and obscuring after every Tab |
| A2 | Every control, image, chart and status has a role and an accessible name that contains its visible label, and status is never shown by color alone (1.1.1, 1.3.1, 1.4.1, 2.5.3, 4.1.2). | Semantic HTML first; status is icon + text + color (§4.5) | axe-core on every page and state; UI steps find elements by role and name only, so an unnamed control fails its scenario |
| A3 | Changes nobody asked for (progress, an operation finishing, an agent going offline) reach screen readers without flooding them, and live updates can be paused (4.1.3, 2.2.2). | One app-wide announcer fed by `UiEvent`s; a "Pause live updates" control (§4.5) | Announcer unit tests with a fake clock; screen-reader pass per milestone |
| A4 | Text has 4.5:1 contrast and UI components 3:1. Pages reflow at 320 × 256 CSS px without two-dimensional scrolling, resize text to 200 %, and survive user text-spacing overrides (1.4.3, 1.4.4, 1.4.10, 1.4.11, 1.4.12). | Design tokens; fluid layout with logical properties and relative units | A token contrast test; axe-core; Playwright at 320 × 256, with 200 % root font size and with the 1.4.12 spacing stylesheet injected |
| A5 | Destructive actions can be reviewed and cancelled, and step-up confirmation is no cognitive test: paste and password managers work (3.3.4, 3.3.8). | Confirmation dialog component; `autocomplete` attributes; paste never blocked (§4.5, §10.4) | Keyboard and screen-reader journeys for restore and step-up confirmation |
| A6 | Form errors are identified in text, with a suggestion where one is known, and announced on submit (3.3.1, 3.3.3, 1.3.5). | Form field component and error summary (§4.5) | Component contracts; journeys that submit invalid input |

---

## 1. Scope

Users, purpose, positioning and product boundaries live in [PRODUCT.md](../../PRODUCT.md). This document covers how Ladderback delivers them. Engineering consequences of those boundaries:

- **Exactly one place runs `forget`/`prune`/`check` per repository** (I1). Backups on many hosts and a prune overlap safely within the limits in §2.2.
- **Profile compatibility is two-way:** import (§6.2) and export, checked by a golden test against the `rustic` CLI.
- **One binary** runs as daemon, hub, worker and CLI, with the web UI embedded (§12).
- **No Windows, but no needless barriers to it.** Paths go through `std::path`/`camino`, never a hard-coded `/`, and platform code is isolated in `adapters/platform`. Windows could be added later without a rewrite, but it isn't designed, built or tested for.
- **v1 command scope:** `copy`, `merge`, `rewrite`, `repair`, `webdav` and `mount` stay CLI-only. Restic must never maintain a Ladderback-managed repository (§7.3).

### Relationship to Backrest

Backrest wraps the `restic` CLI. Ladderback keeps its user-facing model (repos, plans, schedules, hooks, an op log, restore into a new folder) and changes what restic's locking and Backrest's single-host origins force on it.

| | Backrest | Ladderback |
|---|---|---|
| Engine | Shells out to `restic` | Links `rustic_core`, runs each operation in a child process (§4.1) |
| Forget with shared repos | Per-plan `restic forget` after each backup takes an exclusive lock and blocks other hosts' backups ([#244](https://github.com/garethgeorge/backrest/issues/244)) | One maintenance owner per repo; backups never wait for a lock (§7.3) |
| Missed runs on laptops | Skipped unless the schedule uses the last-run clock ([#372](https://github.com/garethgeorge/backrest/issues/372)) | Catch-up by default (§5) |
| Plan ↔ snapshot link | Tags `plan:<id>` and `created-by:<instance>` | Same idea: tags `ladderback-plan:<id>` and `ladderback-agent:<id>` (§7.3) |
| Multi-host | Peer sync; shared repos are pushed to clients **including passwords and backend keys** | Hub and agents; the hub keeps its own secrets and agents never send theirs (§10.2) |
| Secrets in config | Plaintext in `config.json`; `${VAR}` in repo URI, env and flags; a missing variable becomes empty | Command, keyring, file, env or inline sources; a missing reference fails the job (§10.2) |
| Retention | Counts only | Full `KeepOptions`, including `keep-within-*` |
| In-place restore | Not offered | Offered from the agent's own UI only, behind a dry-run summary (§10.4) |
| Not carried over | | `RunCommand` (arbitrary repo commands), multiple users, Windows |
