## 14. Milestones

Rough effort assumes one experienced developer working part-time (~10–15 h/week). Everything that is expensive to retrofit after the first real backup (repo identity, snapshot tags, the worker deadline, the config schema version, request checks) is in M1. Each milestone is finished when its feature files pass (§13.2).

| # | Milestone | Deliverables | Effort |
|---|---|---|---|
| **M0** | Skeleton | Workspace in the §8.3 layout; CI with fmt, clippy, tests, the `domain` wasm build, banned-crate and banned-call checks; cucumber-rs runners, red-first check and `cargo mutants` job; fake clock and port fakes with contract-suite scaffold; worker protocol with a fake operation; daemon serving an embedded "hello" Leptos app; Node toolchain with the `@ui` runner, Playwright, axe-core and a keyboard-only journey on that app | 2–3 wks |
| **M1a** | Engine | Pinned `rustic_core` revision; worker with `EventProgressBars`, warnings, SIGTERM handling, deadline watchdog and per-worker cache dir; identify, backup, prune and check jobs (prune and check so the kill test covers prune); cancel and deadline in `RunOperation`; a minimal `Redactor` over the job's password and repository URL credentials (full secret sources in M1b); agent ID and repository ID; `ladderback-*` tags and explicit parents with fallback; kill-at-random-points test (local + one OpenDAL backend); `*-tmp-*` cleanup | 2–3 wks |
| **M1b** | Single machine MVP | Repo connect/init/test, including optional hot/cold repos; secret sources, `${…}` substitution and redaction (§10.2); plans in `config.toml` with schema version and migrations; hook levels; profile **import** and export with the golden test; manual backup with live progress in UI; snapshot list; unmanaged and orphaned groups with local adoption; op log and history; forget/prune/check run manually on the local owner; request checks (§10.1); `l10n` crate with the `en-US` catalog, error codes in the API, catalog tests and dev pseudolocales (§8.4); accessible component set (dialog, table, form field, status, progress, toast) with its gallery, the announcer, route focus handling, token contrast test, CLI `NO_COLOR`/`--plain`/`--json`, and the first Orca and VoiceOver pass (§4.5) | 4–5 wks |
| **M2** | Schedules and errors | Scheduler with catch-up, jitter, time zones, wake detection, network wait, persistent queue and retries; warnings vs failures; notifications (webhook, ntfy, healthchecks) with templates and plain-text forms; service install (systemd, launchd); optional auth with an accessible sign-in form (A5, A6); "Pause live updates"; macOS signing and Full Disk Access test | 2–3 wks |
| **M3** | Browse, restore and automation | Snapshot tree browser (accessible tree pattern, §4.5); restore to a new folder by default, in-place from the local UI, step-up confirmation (§10.4); download via `ls` + `dump` (tar/zip); repo stats page; scoped API tokens (§10.1); the data-dir lock (§9); MCP server in connect, headless and scratch modes with the read, run and preview tools (§4.6) | 2–3 wks |
| **M4a** | Hub: visibility | Pairing with unique hostnames, sync protocol, persistent outbox and replay, aggregated dashboard, agent offline alerts | 3–4 wks |
| **M4b** | Hub: control and maintenance | Repo ownership registry with leases; hub-managed forget/prune/check by plan tag, serialized per repo; adoption from the hub; owner prune guard; hub downloads; remote run/cancel and opt-in new-folder restore (plans view-only from the hub); MCP connect mode against a hub, with an `agent` argument (§4.6) | 3–4 wks |
| **M5** | Polish | Conditions (AC power / metered), charts with text summaries and table views (§4.5), Docker image, release packaging, `db rebuild`, docs | 2–3 wks |

| Milestone | Feature files |
|---|---|
| M1a | `execution/backup.feature` (warnings), `execution/cancel.feature`, `execution/deadline.feature` (`@e2e` with a real worker) |
| M1b | `planning/config.feature`, `planning/hooks.feature`, `repositories/connect.feature` (init/test/connect; `@e2e` against scratch repositories), `interop/import.feature`, `interop/export.feature`, `maintenance/forget.feature`, `maintenance/adopt.feature`, `maintenance/serialization.feature`, `secrets/substitution.feature`, `secrets/redaction.feature`, `security/request_checks.feature` (`@e2e`), `ui/accessibility.feature` (`@ui`: first backup by keyboard alone; announcements; reflow; form errors) |
| M2 | `execution/schedule.feature` (catch-up, wake, time zones), `execution/retry.feature`, `execution/overlap.feature`, `notifications/rules.feature` |
| M3 | `restore/restore.feature`, `restore/download.feature`, `automation/mcp.feature` |
| M4a | `fleet/pairing.feature`, `fleet/sync_replay.feature` |
| M4b | `maintenance/ownership.feature`, `maintenance/prune_guard.feature`, `restore/hub_restore.feature` |
| M5 | `execution/conditions.feature`, `operations/db_rebuild.feature` |

A **usable personal tool** (M0–M2: scheduled, lock-free backups with notifications on each machine) takes about **10–14 weeks part-time**. **Backrest-like multi-host management** (through M4b) takes about **18–24 weeks**, roughly **4½–6 months part-time**. These ranges include writing scenarios and tests first and building the accessible component set in M1b; M0 is longer than in v0.2 because the test tooling, including the browser toolchain, is built there.

---

## 15. Risks

| Risk | Mitigation |
|---|---|
| `rustic_core` is pre-1.0, so its API changes | Pin one revision for both crates. All usage goes through the `engine` crate. Contract tests run against a temp repo in CI |
| The pinned revision is unreleased code | Move to the first release with `Progress::error`. Until then, upgrade the pin only deliberately, with the kill test and contract tests green |
| A `rustic_core` upgrade adds or changes option fields | Ladderback-owned option types with a parity test per mapped struct (§6.1) |
| Divergence from rustic CLI profile semantics (list replacement, hook levels, `use-profiles`) | Golden tests: parse the rustic repo's `config/*.toml` examples and compare against expected plans; export round-trip test (§6.2) |
| rustic CLI behaviour Ladderback's export relies on changes (e.g. `[forget] filter-*` starts working, `[snapshot-filter]` semantics change) | Export golden test runs against the rustic CLI version CI pins |
| A long backup collides with prune's `keep-delete` window | Worker deadline, owner prune guard and duration warnings (§7.3); 6-day default `keep-delete` |
| Clock skew between agent and hub weakens the deadline | Clock offset recorded per agent; warning above 5 min; the 1 h margin absorbs smaller skew |
| SQLite failure on unusual filesystems | Daily copy, `db rebuild`, CI on btrfs and Docker volumes (§9) |
| Config upgrades break existing installs | Schema version and migrations from M1b; refuse newer schemas (§6.1) |
| Leptos 0.8 → 0.9 migration | Keep UI components thin and logic in `api`/view-models. Migrate once 0.9 is stable |
| Hard-coded strings creep into the UI and make localization a rewrite | Generated message IDs, the unused-ID lint and Playwright pseudolocale runs from M1b (§8.4) |
| Accessibility erodes as the UI grows | Invariants A1–A6 (§0); one component set with contract tests; role-and-name-only test locators; axe-core in CI; a screen-reader pass in each milestone's definition of done (§4.5) |
| ICU4X data bloats the daemon binary | Measured in M1b: with every locale's data compiled in, ICU4X adds 1.1 MB (4%) to the stripped release daemon, so v1 doesn't trim it. Generate data for the shipped locales only if a later change makes it much larger (§8.4) |
| macOS TCC permissions | Signed, notarized builds; detect denials and link to the settings pane. Document this in setup |
| Memory use on very large repos | Worker isolation. Restore and `dump` need the full index (`to_indexed()`); measure early with a large test repo. Document `--no-cache` and cache dir options |
| Hub is a single point of control | Agents back up without the hub. Agent owner leases last 7 days without renewal, so an outage longer than that pauses agent-run maintenance; backups continue |
| Test-first discipline erodes under schedule pressure | Red-first check, mutation testing on changed code and the definition of done (§13); no coverage gate to game |
| Fakes drift from real adapters, so green tests prove nothing | Port contract tests run against both the fake and the real adapter (§13.3) |

---

## 16. Decisions

Decisions 1–6 recorded 2026-11-08. Decisions 7–12 recorded 2026-11-08 after the adversarial review. Decisions 13–16 recorded 2026-11-08 with the development method and license. Decisions 17–18 recorded 2026-11-08. Decisions 21–22 recorded 2026-11-08 with the robustness rules and the time library.

| # | Question | Decision |
|---|---|---|
| 1 | Name | **Ladderback** (binary `ladderback`). |
| 2 | Editing plans from the hub | **View-only in v1.** The hub shows agents' plans but can't change them. Editing from the hub is post-v1 and opt-in per agent. `UpdatePlan` is reserved in sync protocol v1 (§7.2). |
| 3 | Secrets on the hub | **The hub has its own secret entry** for each hub-managed repo. Agents never send secrets. Secrets can come from a command, keyring, file (incl. Docker/Podman secrets and systemd credentials), environment variable or inline. `${VAR}` / `${file:…}` substitution is available in repository fields (§10.2). |
| 4 | Windows | **Out of scope.** macOS and Linux only, with code kept portable where it costs nothing (§1). |
| 5 | Hot/cold repos | **Supported from M1.** Accepted in config and as an optional field on the repository form, passed straight to `rustic_backend`. No dedicated screens. |
| 6 | Restore safety | **Step-up confirmation before every restore and download. Default to restoring into a new folder.** In-place restore is an explicit option in the agent's own UI, with a dry-run summary (§10.4). |
| 7 | `rustic_core` dependency | **Pin one git revision of `main`** for `rustic_core` and `rustic_backend`, because `Progress::error` is unreleased. Move to crates.io at the first release that has it (§2.1). |
| 8 | Linking snapshots to plans | **Tags `ladderback-plan:<id>` and `ladderback-agent:<id>`** on every backup. Untagged snapshots match by `(host, label, paths)` only for adoption (§7.3). |
| 9 | Long-backup guard | **Three layers:** worker deadline, owner prune guard, duration warnings; default `keep-delete` of 6 days, just under the weekly prune interval (§7.3). |
| 10 | Hooks model | **rustic's hook levels exactly** (agent, repository, plan), so hooks import and export faithfully. Notifications are a separate system (§6.1, §11). |
| 11 | Authentication | **Optional, as in Backrest**, with request checks always on and a startup rule that refuses non-loopback binds without auth or a trusted proxy (§10.1). |
| 12 | Restore and download from the hub | **Split by risk.** Downloads use the hub's own repo access; restores to agents are new-folder only and opt-in per agent; in-place restore only from the agent's own UI (§10.4). |
| 13 | Code architecture | **Hexagonal (ports and adapters) around a DDD core.** `domain` and `app` are the inner rings; one crate per adapter; ports are `Arc<dyn Port>` with `#[async_trait]`; the dependency rule is enforced by banned-crate and banned-call checks in CI (§3.2). |
| 14 | Development method | **Behaviour first and test first, for all production code.** Gherkin scenarios run by cucumber-rs against use cases with fakes, `@e2e` scenarios against the real binary; red-first check and `cargo mutants --in-diff` in CI instead of a coverage gate (§13). |
| 15 | Option types | **Ladderback owns its option types** in `domain`. The `config` adapter parses TOML with rustic's keys into them; the engine adapter maps them onto `rustic_core` types, with a parity test per struct. Replaces v0.2's plan to deserialize into `rustic_core` types directly (§6.1). |
| 16 | License | **MIT OR Apache-2.0**, the same terms as rustic, so code can move upstream without relicensing. Files: `LICENSE-MIT`, `LICENSE-APACHE`; the workspace `Cargo.toml` will declare `license = "MIT OR Apache-2.0"`. |
| 17 | Internationalization | **Localization-ready from the first UI string; `en-US` only in v1.** Project Fluent via `fluent-bundle` in a shared `l10n` crate, with a per-message fallback chain of bundles; the core and the wire carry codes and typed arguments, never sentences; dates and numbers through browser `Intl` or ICU4X behind Fluent's formatter hook; machine-facing text never localized (§8.4). |
| 18 | Accessibility | **First-class, WCAG 2.2 AA from the first screen.** Invariants A1–A6 in §0; one accessible component set and an app-wide announcer; `@ui` scenarios in Playwright with axe-core and keyboard-only journeys from M0; a manual Orca and VoiceOver pass per milestone (§4.5). |
| 19 | Versioning | **One SemVer version for the whole product**, judged against a declared contract (CLI, config schema, `/api/v1`, `/sync/v1`, snapshot tags, webhook JSON, exported profiles, data migrations). `0.1.0` at the end of M1b, `1.0.0` after M5; `vX.Y.Z` tags and `CHANGELOG.md` produced by release-plz from Conventional Commits through a reviewed release PR (§12.1). |
| 20 | MCP server | **`ladderback mcp`: newline-delimited JSON-RPC 2.0 over stdio (MCP `2025-06-18` or later), in one inbound adapter (`adapters/mcp`).** Connect mode drives a running agent through `/api/v1` (a hub from M4b) with a scoped API token; headless and scratch modes run their own in-process session under the data-dir lock. Tools read state, run and cancel backups, and preview forget and import. Nothing restores, deletes, edits config or adds a command: a model-driven client is not a human confirmation, and token scopes enforce this in the daemon (§4.6, §10.5). |
| 21 | Robustness rules | **Four rules, enforced by tooling where a tool can, recorded 2026-11-08 (§13.5).** Never crash in non-test code (clippy `deny` at every production crate root). Input-derived numbers are hostile (caps and checked access; the worker protocol's messages are capped at 1 MiB, §4.1). A test that can't run says so with `testkit::skip!`, and `just test` lists the skips. Parallel agents each get their own worktree, target directory, ports and scratch folder. Why: the daemon runs unattended and owns the operation queue, so one panic silently stops backups; the inputs that reach it come from other processes, files and networks; a silent skip reads as a pass; and agents sharing a tree or a port break each other's builds. |
| 22 | One time library | **Time goes through `jiff`, never `chrono`** (recorded 2026-11-08). `croner` is used with `default-features = false, features = ["jiff"]`, and `just architecture` bans `chrono` from `domain` and `app`. Why: Ladderback already models time with `jiff` (`Timestamp`, `SignedDuration`, the `Clock` port), and so does `rustic_core`; `jiff`'s time-zone handling and `Span`/DST arithmetic are what scheduling needs; `chrono` would add a second model of time and conversions between the two. `chrono` still reaches the build transitively through `rustic_backend`'s OpenDAL FTP service (`suppaftp`), which can't be turned off; no Ladderback crate depends on it. |
| 23 | Request checks: JSON for every mutating request | **Every mutating request (any method except `GET`, `HEAD`, `OPTIONS`) must be `Content-Type: application/json`, whether or not it carries `Origin`** (recorded 2026-11-08, §10.1). The `Origin` match applies only when `Origin` is present. Why: with auth off, a cross-site `text/plain` form post can set hooks and `password-command`, and a browser or extension that strips `Origin` would otherwise let it through unchecked. Cost: curl and scripts must send the header. |
| 24 | Project name | **The project is named Ladderback** (recorded 2026-11-08), renamed from Steading before any release because of a name clash. It is inspired by Backrest, not derived from it. Why: nothing was released, so no user had data, keyring entries or snapshots under the old name, and stored identifiers changed without migration. |

## 17. Open questions

To be answered by M1a/M2 experiments rather than by decision:

1. Does a SIGKILL during an S3, B2 or SFTP upload leave a partial object under a final pack name? The kill test (§4.1) must cover one OpenDAL backend.
2. Does a Full Disk Access grant on the launchd-started daemon carry over to the worker child process, or does the worker need its own grant?
3. How long does prune take on a large repo? That sets `prune_margin` (§7.3).

To be decided in M4a, when hub and agents first talk:

4. How far may hub and agent versions differ? The likely rule is that a hub speaks the `/sync/v1` protocol of its own release line and the one before, so the hub upgrades first (§12.1).
