## 14. Milestones

Rough effort assumes one experienced developer working part-time (~10–15 h/week). Everything that is expensive to retrofit after the first real backup (repo identity, snapshot tags, the worker deadline, the config schema version, request checks) is in M1. Each milestone is finished when its feature files pass (§13.2). A [GitHub milestone](https://github.com/nledford/Ladderback/milestones) of the same name collects the issues that must close before it is finished; this section, not the milestone, says what each one delivers.

| # | Milestone | Deliverables | Effort |
|---|---|---|---|
| **M0** | Skeleton | Workspace in the §8.3 layout; CI with fmt, clippy, tests, the `domain` wasm build, banned-crate and banned-call checks; cucumber-rs runners, red-first check and `cargo mutants` job; fake clock and port fakes with contract-suite scaffold; worker protocol with a fake operation; daemon serving an embedded "hello" Leptos app; Node toolchain with the `@ui` runner, Playwright, axe-core and a keyboard-only journey on that app | 2–3 wks |
| **M1a** | Engine | Pinned `rustic_core` revision; worker with `EventProgressBars`, warnings, SIGTERM handling, deadline watchdog and per-worker cache dir; identify, backup, prune and check jobs (prune and check so the kill test covers prune); cancel and deadline in `RunOperation`; a minimal `Redactor` over the job's password and repository URL credentials (full secret sources in M1b); agent ID and repository ID; `ladderback-*` tags and explicit parents with fallback; kill-at-random-points test (local + one OpenDAL backend); `*-tmp-*` cleanup | 2–3 wks |
| **M1b** | Single machine MVP | Repo connect/init/test, including optional hot/cold repos; secret sources, `${…}` substitution and redaction (§10.2); plans in `config.toml` with schema version and migrations; hook levels; profile **import** and export with the golden test; manual backup with live progress in UI; snapshot list; unmanaged and orphaned groups with local adoption; op log and history; forget/prune/check run manually on the local owner; request checks (§10.1); `l10n` crate with the `en-US` catalog, error codes in the API, catalog tests and dev pseudolocales (§8.4); accessible component set (dialog, table, form field, status, progress, toast) with its gallery, the announcer, route focus handling, token contrast test, CLI `NO_COLOR`/`--plain`/`--json`, and the first Orca and VoiceOver pass (§4.5) | 4–5 wks |
| **M2** | Schedules and errors | Scheduler with catch-up, jitter, time zones, wake detection, network wait, persistent queue and retries; the AC-power run condition; warnings vs failures; notifications (webhook, ntfy, healthchecks) with templates and plain-text forms; service install (systemd, launchd); optional auth with an accessible sign-in form (A5, A6); "Pause live updates" | 2–3 wks |
| **M3** | Browse, restore and automation | Snapshot tree browser (accessible tree pattern, §4.5); restore to a new folder by default, in-place from the local UI, step-up confirmation (§10.4); download via `ls` + `dump` (tar/zip); repo stats page; scoped API tokens (§10.1); the data-dir lock for `mcp --headless` (§9; `serve` took it in M1b); MCP server in connect, headless and scratch modes with the read, run and preview tools, and in connect mode the proposal tools (§4.6; decision 89); on macOS, `just install` signing with a self-signed certificate, and the Full Disk Access tests (decision 71) | 2–3 wks |
| **M3b** | Shared UI components | The shared components the pages already use (#776), to the bar of §4.5 and §13.7.5, and the pages moved onto them; new styles Tailwind first (decision 117); a narrow or full-width page (#933); Settings as tabs (#935). Added 2026-11-17 by the maintainer, between M3 and the hub. A component no page uses yet is built just before the first page that needs it, in that page's milestone (decision 119). Moving the rest of the existing custom CSS onto utilities (#931) went to M10 on 2026-11-20, since it changes nothing users see | 1–2 wks |
| **M4** | AI agents | What follows M3 for AI agents: approving a proposal that could delete snapshots from its forget preview, the dry run of a draft plan and the MCP proposal tools (#518, decision 89); the MCP tools people expect to ask for (#961); the AI agents tab in Settings (#962, decision 127), done on 2026-11-21 | 2–3 wks |
| **M5** | Scheduled maintenance | Forget, prune and check on their schedules on the machine that owns maintenance, through a maintenance queue with its own limit (decision 75, point 8; #970), each check's result recorded and shown (#974); a check an agent asks for (#960); a missed run announced (#415). One machine; the hub's managed maintenance is M9 | 2–3 wks |
| **M6** | Notification channels | Gotify, Discord, Slack, email, Telegram, Pushover, Pushbullet, web push and a webhook the user templates (#380), with the notification fixes beside them (#401, #402) | 2–3 wks |
| **M7** | Japanese | Japanese, with the translation workflow and a language choice in the UI (§16, decision 101); after `1.0.0`, released on its own once the review of its text (#707) is done | 1–2 wks, and the review |
| **M8** | Hub: visibility | Pairing with unique hostnames, sync protocol, persistent outbox and replay, aggregated dashboard, agent offline alerts. Built behind the `hub` feature, which release builds leave out until a pull request deletes it just before the MVP's release is tagged (decision 122 as amended, #1366). Any installed machine can become the hub in place, with hub mode in `config.toml`; the Docker image, published as a preview by each release after 0.5.0 (#358), becomes the recommended channel for a machine whose only job is the hub (decisions 126 and 136) | 4–6 wks |
| **M9** | Hub: control and maintenance | Repo ownership registry with leases; hub-managed forget/prune/check by plan tag, serialized per repo; adoption from the hub; owner prune guard; hub downloads; remote run/cancel and opt-in new-folder restore (plans view-only from the hub); MCP connect mode against a hub, with an `agent` argument (§4.6) | 3–4 wks |
| **M10** | Polish | The metered-network run condition (AC power came in M2), charts with text summaries and table views (§4.5), release packaging, the rest of the custom CSS moved onto Tailwind utilities (#931), `db rebuild`, docs | 2–3 wks |
| **M11** | Menu bar and tray icon | A status icon in the macOS menu bar and the Linux tray (#385): a separate process that reads the daemon only through its API, so the daemon gains no desktop code; layered icons for scope, activity and outcome, one for a machine and one for the hub. Added 2026-11-22 by the maintainer, after `1.0.0`; it needs a §16 decision when it starts | 2–3 wks (rough) |

From M1b on, each milestone ends with a release; [operations.md §12.1](operations.md#121-versioning-and-releases) says which version each one is.

**Old names.** Until 2026-11-14 the milestones after M3 had other names: M4a is now M4, M4b is now M5, and M5 is now M6. The hub milestone had been split in two, and the lettered names were hard to follow. On 2026-11-17 the maintainer added four milestones and numbered every milestone after M3b in the order it is planned: the hub's visibility (M4 until then) is now M8, its control and maintenance (M5) is M9, and Polish (M6) is M10; AI agents, scheduled maintenance, notification channels and Japanese (split out of the hub's visibility) are M4 to M7. Decisions recorded before a renaming, and the issues and pull requests of that time, keep the names they had then.

**Order since 2026-11-18.** The hub (M8, then M9), AI agents (M4) and the rest of M3b run in parallel, one track each; M5 to M7 follow when a track frees up, and each milestone is still released when it finishes, so the numbers no longer say the order of the work (decision 119). On 2026-11-22 the maintainer put the milestones into three phases (below), and the MVP milestone's issues now come before every track's.

**Three phases since 2026-11-22** (the maintainer's decision). The goal is first an MVP that works and can start real-world testing before a proper `1.0.0`, not perfection or every edge case. The [GitHub milestones](https://github.com/nledford/Ladderback/milestones) hold each phase's issues. **MVP: Ready for real-world testing** comes first, with what testers need: pairing and the hub's dashboard, offline alerts and sync (from M8); the repository ownership registry and the owner prune guard for shared repositories (from M9); the install, getting-started and hub docs (from M10); a few bugs on common paths; and full CI on pull requests again (#1097, decision 124). **Before `1.0.0`:** the rest of M4, M5 and M8 to M10. **After `1.0.0`:** M6, M7 and M11 whole, and a milestone named "After 1.0" for enhancements, polish, rare edge cases and test-infrastructure work.

M2 ends with its release, `0.2.0`: after the tag, it runs on real machines before M3 starts, and what that run turns up is fixed or filed first. On macOS that run uses an ad-hoc-signed build from source, with Full Disk Access granted by hand (decision 71). That run found that a repository has one location, so a laptop that reaches its server by a LAN address at home and a public one away takes the slow route at home (#424): decision 80's one pull request (ordered locations, a bounded probe, the same-repository check, and rules by network link and by Wi-Fi name) is that fix and lands before M3 starts; M10's conditions reuse its platform readers.

Setting up backups with an AI agent's help, a person confirming every change (#518, decision 89), lands in this order, each step usable alone: first plan writing as a use case, with the plan editor on the web UI's Plans page; then proposals, with their REST API, the "Pending changes" page and the dry run, which need M3's scoped tokens (for the `propose` scope) and step-up confirmation, and writing to the keyring (#476); then the MCP proposal tools, with the MCP server (#517) in M3.

Proposals land in parts: first they are kept and made, listed, read and withdrawn over the REST API, plans only and approved by nobody yet (decision 89's first amendment); then the "Pending changes" page, approval and rejection, with the rate limits and the submission warnings, a proposal that could delete snapshots being only rejected (its second amendment); then the forget preview, shown on request (its third amendment); then approving a proposal that could delete snapshots, bound to that preview by its digest, read again under the lock the apply holds (its fourth amendment). The steps before them are outside the effort estimates above; if they aren't done when the rest of M3 is, the MCP proposal tools follow M3 rather than holding it back, in M4. Proposals made on the hub come later, with editing plans from the hub, through `UpdatePlan` (decision 2).

**Where it stood at M3's release (0.4.0).** Plan writing and the Plans page's editor, writing a repository's password to the keyring (#476), the proposals' REST API and the "Pending changes" page are done. The forget preview, the dry run of a draft plan and the MCP proposal tools (#518) were not, so they follow M3, in M4, as the paragraph above allows. So does a proposal for a repository, a hook or a maintenance claim. The forget preview (#915) and approving a proposal from it (#971) have landed since.

| Milestone | Feature files |
|---|---|
| M1a | `execution/backup.feature` (warnings), `execution/cancel.feature`, `execution/deadline.feature` (`@e2e` with a real worker) |
| M1b | `planning/config.feature`, `planning/hooks.feature`, `repositories/connect.feature` (init/test/connect; `@e2e` against scratch repositories), `repositories/api.feature` (`@e2e`: the routes the page uses, over HTTP), `ui/repositories.feature` (`@ui`: the Repositories page by keyboard alone), `snapshots/list.feature` (`@e2e`; a read, not an operation), `interop/import.feature`, `interop/export.feature`, `cli/output.feature` (`@e2e`: `--plain`, `NO_COLOR`, `import-profile --json`, the statuses, a closed pipe), `maintenance/forget.feature`, `maintenance/adopt.feature`, `maintenance/serialization.feature`, `secrets/substitution.feature`, `secrets/redaction.feature`, `security/request_checks.feature`, `security/body_limits.feature` and `security/response_headers.feature` (`@e2e`), `ui/accessibility.feature` (`@ui`: first backup by keyboard alone; announcements; reflow; form errors) |
| M2 | `execution/schedule.feature` (catch-up, wake, time zones), `execution/run_conditions.feature` (AC power, wake, network wait), `execution/retry.feature`, `execution/overlap.feature`, `notifications/rules.feature` |
| M3 | `restore/restore.feature`, `restore/download.feature`, `automation/mcp.feature` |
| M3b | `ui/components/<component>.feature` (each component's contract), and the `@ui` scenarios of the pages each move touches |
| MVP | the scenarios of the parts it takes from M8 and M9: `fleet/pairing.feature`, `fleet/sync_replay.feature`, `maintenance/ownership.feature`, `maintenance/prune_guard.feature` |
| M4 | `automation/mcp.feature`, `planning/proposal*.feature` |
| M5 | scheduled prune and check in `maintenance/` and `execution/schedule.feature`, and the maintenance queue in `execution/overlap.feature` |
| M6 | `notifications/` scenarios for each channel |
| M7 | the catalog tests and the `@ui` scenarios under `ja`, and the language choice in `ui/` |
| M8 | `fleet/pairing.feature`, `fleet/sync_replay.feature` |
| M9 | `maintenance/ownership.feature`, `maintenance/prune_guard.feature`, `restore/hub_restore.feature` |
| 0.2.x follow-up | `repositories/repository_locations.feature` and the scheduler's location skip in `execution/` (decision 80, with the `@e2e` and `@ui` scenarios) |
| M10 | metered-network scenarios in `execution/run_conditions.feature`, `operations/db_rebuild.feature` |
| M11 | named with its §16 decision (#385) |

A **usable personal tool** (M0–M2: scheduled, lock-free backups with notifications on each machine) takes about **10–14 weeks part-time**. **Backrest-like multi-host management** (through M9 as first planned, without M3b to M7) takes about **18–24 weeks**, roughly **4½–6 months part-time**. These ranges include writing scenarios and tests first and building the accessible component set in M1b; M0 is longer than in v0.2 because the test tooling, including the browser toolchain, is built there.

---

## 15. Risks

| Risk | Mitigation |
|---|---|
| `rustic_core` is pre-1.0, so its API changes | Depend on one minor release of each crate. All usage goes through the `engine` crate. Contract tests run against a temp repo in CI |
| A `rustic_core` release changes behaviour (a chunker cut, a prune rule) | Move to a new release only deliberately, in a commit of its own: its breaking changes read against I2 and I3, the kill test and contract tests green, and the engine's start of an `rclone:` server compared with rustic's `rclone.rs` (decision 58) |
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
| The bundled time-zone data bloats the daemon binary | Measured at M1b: the copy of the IANA database adds 248 KB (0.7%) to the stripped release daemon and nothing to the web UI's wasm, which looks up no zone. Kept, because without it a host with no `tzdata` reads every zone as UTC (§16 decision 30) |
| macOS TCC permissions | A code signature that stays the same across upgrades (decision 71). Detect denials and link to the settings pane. Document this in setup |
| Memory use on very large repos | Worker isolation. Restore and `dump` need the full index (`to_indexed()`); measure early with a large test repo. Document `--no-cache` and cache dir options |
| Hub is a single point of control | Agents back up without the hub. Agent owner leases last 7 days without renewal, so an outage longer than that pauses agent-run maintenance; backups continue |
| Test-first discipline erodes under schedule pressure | Red-first check, mutation testing on changed code and the definition of done (§13); no coverage gate to game |
| Fakes drift from real adapters, so green tests prove nothing | Port contract tests run against both the fake and the real adapter (§13.3) |

---

## 16. Decisions

Decisions 1–6 recorded 2026-11-08. Decisions 7–12 recorded 2026-11-08 after the adversarial review. Decisions 13–16 recorded 2026-11-08 with the development method and license. Decisions 17–18 recorded 2026-11-08. Decisions 21–22 recorded 2026-11-08 with the robustness rules and the time library. Decision 25 recorded 2026-11-09 with manual maintenance. Decision 26 recorded 2026-11-09 with adoption. Decision 27 recorded 2026-11-10 with profile interop. Decision 29 recorded 2026-11-10 with the config history. Decision 30 recorded 2026-11-10 with the time-zone data.

| # | Question |
|---|---|
| [1](decisions/01-name.md) | Name |
| [2](decisions/02-editing-plans-from-the-hub.md) | Editing plans from the hub |
| [3](decisions/03-secrets-on-the-hub.md) | Secrets on the hub |
| [4](decisions/04-windows.md) | Windows |
| [5](decisions/05-hot-cold-repos.md) | Hot/cold repos |
| [6](decisions/06-restore-safety.md) | Restore safety |
| [7](decisions/07-rustic-core-dependency.md) | `rustic_core` dependency |
| [8](decisions/08-linking-snapshots-to-plans.md) | Linking snapshots to plans |
| [9](decisions/09-long-backup-guard.md) | Long-backup guard |
| [10](decisions/10-hooks-model.md) | Hooks model |
| [11](decisions/11-authentication.md) | Authentication |
| [12](decisions/12-restore-and-download-from-the-hub.md) | Restore and download from the hub |
| [13](decisions/13-code-architecture.md) | Code architecture |
| [14](decisions/14-development-method.md) | Development method |
| [15](decisions/15-option-types.md) | Option types |
| [16](decisions/16-license.md) | License |
| [17](decisions/17-internationalization.md) | Internationalization |
| [18](decisions/18-accessibility.md) | Accessibility |
| [19](decisions/19-versioning.md) | Versioning |
| [20](decisions/20-mcp-server.md) | MCP server |
| [21](decisions/21-robustness-rules.md) | Robustness rules |
| [22](decisions/22-one-time-library.md) | One time library |
| [23](decisions/23-request-checks-json-for-every-mutating.md) | Request checks: JSON for every mutating request |
| [24](decisions/24-project-name.md) | Project name |
| [25](decisions/25-who-decides-a-forgets-deletions.md) | Who decides a forget's deletions |
| [26](decisions/26-what-an-adoption-writes.md) | What an adoption writes |
| [27](decisions/27-profile-interop-what-an-import-refuses.md) | Profile interop: what an import refuses, what an export says |
| [28](decisions/28-repositories-page-and-its-routes.md) | Repositories page and its routes |
| [29](decisions/29-config-history-symlinks-order-and-flushing.md) | Config history: symlinks, order and flushing |
| [30](decisions/30-time-zone-data.md) | Time-zone data |
| [31](decisions/31-a-secret-a-url-parser-splits.md) | A secret a URL parser splits |
| [32](decisions/32-worker-messages-that-outgrow-the-cap.md) | Worker messages that outgrow the cap |
| [33](decisions/33-body-limits-and-security-headers.md) | Body limits and security headers |
| [34](decisions/34-a-hooks-output-after-the-hook.md) | A hook's output after the hook has ended |
| [35](decisions/35-the-zone-of-an-instant-with.md) | The zone of an instant with no zone |
| [36](decisions/36-repository-jobs-have-a-deadline.md) | Repository jobs have a deadline |
| [37](decisions/37-a-secret-written-in-the-config.md) | A secret written in the config |
| [38](decisions/38-the-command-lines-output.md) | The command line's output |
| [39](decisions/39-live-rows-are-stable.md) | Live rows are stable |
| [40](decisions/40-crash-recovery-and-the-data-dir.md) | Crash recovery and the data-dir lock |
| [41](decisions/41-the-end-of-an-operation-is.md) | The end of an operation is announced from the run |
| [42](decisions/42-a-maintenance-start-the-client-drops.md) | A maintenance start the client drops, and the writes the log refuses |
| [43](decisions/43-dependency-policy.md) | Dependency policy |
| [44](decisions/44-reviewing-maintenance-before-it-runs.md) | Reviewing maintenance before it runs |
| [45](decisions/45-masking-a-text-cut-at-a.md) | Masking a text cut at a cap |
| [46](decisions/46-the-toolings-language.md) | The tooling's language |
| [47](decisions/47-msrv-1-94.md) | MSRV 1.94 (superseded by decision 78) |
| [48](decisions/48-who-maintains-a-saved-repository.md) | Who maintains a saved repository |
| [49](decisions/49-one-name-per-repository.md) | One name per repository |
| [50](decisions/50-a-cancelled-backup-keeps-its-packs-indexed.md) | A cancelled backup keeps its packs indexed |
| [51](decisions/51-what-a-checks-findings-are.md) | What a check's findings are |
| [52](decisions/52-the-operation-log-keeps-the-repositorys-alias.md) | The operation log keeps the repository's alias |
| [53](decisions/53-creating-an-imported-repository-from-its-row.md) | Creating an imported repository from its row |
| [54](decisions/54-early-release-builds.md) | Early release builds; the Linux archives published from 0.3.1 |
| [55](decisions/55-a-backup-stopped-at-its-deadline-winds-down.md) | A backup stopped at its deadline winds down |
| [56](decisions/56-the-calendar-a-retention-rule-counts-on.md) | The calendar a retention rule counts on |
| [57](decisions/57-recording-the-id-of-a-repository-a-job-opened.md) | Recording the ID of a repository a job opened |
| [58](decisions/58-one-child-starts-at-a-time.md) | One child starts at a time |
| [59](decisions/59-where-the-workers-keep-rustics-cache.md) | Where the workers keep rustic's cache |
| [60](decisions/60-how-a-plan-s-next-run-is-worked-out.md) | How a plan's next run is worked out |
| [61](decisions/61-tidying-the-workers-cache-at-start.md) | Tidying the workers' cache at start |
| [62](decisions/62-another-agents-recent-snapshots-come-from-another-machine.md) | Another agent's recent snapshots come from another machine |
| [63](decisions/63-the-ports-for-the-conditions-before-a-run.md) | The ports for the conditions before a run |
| [64](decisions/64-installing-the-agent-as-a-user-service.md) | Installing the agent as a user service |
| [65](decisions/65-pausing-live-updates.md) | Pausing live updates |
| [66](decisions/66-the-server-accepts-through-the-spawn-gate.md) | The server accepts through the spawn gate |
| [68](decisions/68-the-start-of-an-operation-is-announced-from-the-run.md) | The start of an operation is announced from the run |
| [69](decisions/69-a-save-edits-the-file-a-person-wrote.md) | A save edits the file a person wrote |
| [70](decisions/70-sign-in.md) | Sign-in |
| [71](decisions/71-signing-macos-builds-without-a-developer-id.md) | Signing macOS builds without a Developer ID |
| [72](decisions/72-a-schedule-s-interval-and-time-zone-and-a-plan-s-overlap-policy.md) | A schedule's interval and time zone, and a plan's overlap policy |
| [73](decisions/73-the-persistent-queue-and-the-scheduler-loop.md) | The persistent queue and the scheduler loop |
| [74](decisions/74-how-notifications-are-configured-and-delivered.md) | How notifications are configured and delivered |
| [75](decisions/75-the-overlap-policy-and-the-concurrency-limit.md) | The overlap policy and the concurrency limit |
| [76](decisions/76-retries-of-a-failed-backup.md) | Retries of a failed backup |
| [77](decisions/77-which-characters-go-prints.md) | Which characters Go prints |
| [78](decisions/78-the-rust-toolchain-is-pinned.md) | The Rust toolchain is pinned, and the MSRV is the pin |
| [79](decisions/79-a-from-source-install-script.md) | The install script: the Linux archive, or a build from source |
| [80](decisions/80-repository-locations.md) | A repository reached at several locations |
| [81](decisions/81-identify-a-repository-before-its-first-job.md) | A job of an entry that records no repository ID runs after an identify |
| [82](decisions/82-the-step-a-running-operation-is-in.md) | How a page learns which step a running operation is in |
| [83](decisions/83-a-progress-bar-a-screen-reader-can-reach.md) | A progress bar a screen reader can reach, and that never chatters |
| [84](decisions/84-view-blocks-are-formatted-by-leptosfmt.md) | Formatting the inside of `view!` blocks |
| [85](decisions/85-a-server-s-refusal-of-the-credentials.md) | How a REST server's refusal of the credentials is told |
| [86](decisions/86-trying-a-repositorys-locations-at-the-same-time.md) | Trying a repository's locations at the same time |
| [87](decisions/87-listening-on-the-network-with-sign-in-off.md) | Listening on the network with sign-in off |
| [88](decisions/88-hub-and-agent-version-compatibility.md) | How far hub and agent versions may differ |
| [89](decisions/89-agent-proposed-setup.md) | How an AI agent proposes a backup setup for a person to confirm |
| [90](decisions/90-the-build-a-program-names.md) | The build a program names beside its version |
| [91](decisions/91-the-operations-a-start-ends-are-announced.md) | The operations a start ends are announced |
| [92](decisions/92-the-daemons-stop-has-a-limit.md) | The daemon's stop has a limit |
| [93](decisions/93-api-tokens.md) | API tokens |
| [94](decisions/94-restart-from-settings.md) | Restarting from Settings |
| [95](decisions/95-the-mcp-server-build.md) | The MCP server as built |
| [96](decisions/96-plans-edited-in-the-ui.md) | Creating, changing and deleting plans in the UI |
| [97](decisions/97-red-first-in-every-engine.md) | Red-first in every engine |
| [98](decisions/98-no-core-dumps-or-debugger-attach.md) | No core dumps or debugger attach for the processes that hold secrets |
| [99](decisions/99-who-is-trusted.md) | Who is trusted with Ladderback's local data |
| [101](decisions/101-japanese-and-the-translation-workflow.md) | Japanese, and how a translation is kept |
| [102](decisions/102-snapshot-tree-browsing.md) | Browsing a snapshot's tree |
| [103](decisions/103-status-badge-icons.md) | Status badge icons |
| [104](decisions/104-repository-stats.md) | A repository's stats |
| [105](decisions/105-clippy-lints.md) | Clippy lints for complexity, nesting and indexing |
| [106](decisions/106-the-crate-graph-checked.md) | Which workspace crates and foreign models each crate may depend on |
| [107](decisions/107-the-update-notice.md) | The update notice |
| [108](decisions/108-component-vocabulary.md) | One vocabulary of tones, sizes, placements and sides |
| [109](decisions/109-restore.md) | How a restore is started from the web UI |
| [110](decisions/110-repository-keyring.md) | Keeping a typed repository password in the system keyring |
| [111](decisions/111-the-recent-log.md) | The recent log in the web UI |
| [112](decisions/112-no-repository-config-and-the-half-a-server-refused.md) | How no repository config, the half a server refused and a listing's failure are told |
| [113](decisions/113-shared-button.md) | A button drawn from the tokens, not the browser's own |
| [114](decisions/114-download.md) | How an entry of a snapshot is downloaded from the web UI |
| [115](decisions/115-detecting-full-disk-access.md) | Detecting a missing Full Disk Access grant |
| [116](decisions/116-keyring-wait-at-start.md) | A scheduled run waits for the session's keyring service while the daemon starts |
| [117](decisions/117-tailwind-first.md) | Tailwind first, custom CSS only when necessary |
| [118](decisions/118-checkbox-and-radio-drawn-from-the-tokens.md) | A checkbox and a radio drawn from the tokens, not the browser's own |
| [119](decisions/119-components-when-a-page-needs-them.md) | Build each new shared component just before the first page that needs it |
| [120](decisions/120-agent-hub-connection-security.md) | How an agent and its hub prove who they are |
| [121](decisions/121-pairing-a-machine-with-its-hub.md) | How a person pairs a machine with its hub |
| [122](decisions/122-hub-out-of-releases-until-m8.md) | How the hub stays out of releases until M8 |
| [123](decisions/123-deb-and-rpm-packages.md) | .deb and .rpm packages with each release, built by nfpm |
| [124](decisions/124-mvp-ci-mode.md) | How CI runs on pull requests until the MVP |
| [125](decisions/125-coderabbit-reviews-on-request.md) | CodeRabbit reviews pull requests on request, as an advisory second reviewer |
| [126](decisions/126-primary-install-channels.md) | Which ways to install Ladderback the project recommends |
| [127](decisions/127-per-token-permissions.md) | What each API token, and an AI agent without one, may do |
| [128](decisions/128-the-user-docs-site.md) | What builds the user docs site, and from where |
| [129](decisions/129-a-changed-row-stays-marked-until-seen.md) | A changed live row stays marked until it is seen |
| [130](decisions/130-reduced-motion-keeps-fades.md) | What reduced motion stops, and what it keeps |
| [131](decisions/131-an-import-confirms-the-commands-it-adds.md) | An import lists the commands it makes runnable, and asks first |
| [132](decisions/132-find-in-snapshots.md) | How an AI agent finds a file across a repository's snapshots |
| [133](decisions/133-a-machine-not-an-agent.md) | A machine running Ladderback is a machine, not an agent |
| [134](decisions/134-an-agent-reads-the-log-a-page-at-a-time.md) | How an AI agent reads the daemon's log |
| [135](decisions/135-keyring-unlock-wait.md) | A run whose keyring is locked waits for it to be unlocked |
| [136](decisions/136-an-installed-machine-becomes-the-hub.md) | How an installed machine becomes the hub |
| [137](decisions/137-a-hub-moves-by-handover-never-by-copy.md) | A hub moves by handover, never by copy, and a hub that finds itself rewound recovers before it grants |
| [138](decisions/138-the-storage-protects-an-offsite-copy.md) | The storage, not Ladderback, keeps an offsite copy that a machine that backs up can't delete |
| [139](decisions/139-no-graphql-api.md) | No GraphQL API |
| [140](decisions/140-a-reader-chooses-a-tables-columns.md) | A reader chooses which of a table's columns show, and in what order |
| [141](decisions/141-the-header-stands-in-the-page-frame.md) | The header's content stands in the page's frame, and the pages' links take the text's color |
| [142](decisions/142-the-running-version-in-the-footer.md) | The running version stands in a footer at the foot of every page |
| [143](decisions/143-a-hamburger-menu-holds-the-pages-links-below-41rem.md) | Below 41rem a hamburger menu holds the pages' links and Sign out |
| [144](decisions/144-a-copied-data-folder-stops-maintaining.md) | What a machine does when its data folder was copied from another computer |
| [145](decisions/145-the-agents-outbox-and-the-hubs-reports.md) | The agent's outbox and the hub's reports are bounded, and each says what it gives up |
| [146](decisions/146-the-web-ui-wasm-is-built-for-size.md) | The release web UI's wasm is built for size, with a profile of its own |
| [147](decisions/147-the-hub-decides-leases-on-each-report.md) | The hub decides each repository's lease on every report, and counts a lease an hour past its expiry |
| [149](decisions/149-the-hubs-dashboard.md) | The hub's dashboard shows when each machine last connected and its latest operations, follows the hub's events, and is reached from the start page and the Machines tab |
| [150](decisions/150-the-hub-takes-one-computer-per-machine-id.md) | The hub takes one computer under each machine ID |
| [151](decisions/151-a-paired-machine-maintains-only-under-its-hubs-lease.md) | A paired machine maintains only under a live lease from its hub, and asks for its grants every 15 minutes |
| [152](decisions/152-a-one-line-header-with-a-settings-menu.md) | A one-line header with a Settings menu, and links standing alone without an underline |
| [154](decisions/154-removing-a-machine.md) | How a person removes or rejects a machine on the hub, and when a waiting machine lapses |
| [157](decisions/157-the-hubs-start-window-runs-on-its-monotonic-clock.md) | The hub's start window waits on every agent whose last report named a repository, for 7 days and an hour on its monotonic clock |
| [158](decisions/158-maintenance-paused.md) | A paired machine shows when its maintenance paused for want of a lease, and tells it after 14 days |

## 17. Open questions

To be answered by experiments in M1a to M3 rather than by decision:

1. Does a SIGKILL during an S3, B2 or SFTP upload leave a partial object under a final pack name? The kill test (§4.1) must cover one OpenDAL backend.
2. Does a Full Disk Access grant on the launchd-started daemon carry over to the worker child process, or does the worker need its own grant? **Answered** on macOS 27.3 (#281): it carries over, and the worker needs none ([§12](operations.md#12-packaging-and-deployment)).
3. How long does prune take on a large repo? That sets `prune_margin` (§7.3).

To be decided in M8, when hub and agents first talk:

4. How far may hub and agent versions differ? **Decided** in [decision 88](decisions/88-hub-and-agent-version-compatibility.md).
