# Decision 122: How the hub stays out of releases until M8

**The hub's code merges to `main` behind a Cargo feature, `hub`, which every default and test build has and a release build leaves out, as `just architecture` and the release smoke test check. One pull request deletes the feature just before M8's first release candidate is tagged. Until then no release can pair, sync or serve as a hub, and the hub's place in the public contract (§12.1) and decision 88 start at M8's full release, the first to ship protocol 1** (recorded 2026-11-18, §7, §9, §12.1, §14; #1043, for the maintainer's approval).

**Amended 2026-11-22 (the maintainer, #1366):** the gate comes off for the MVP's release, not M8's. The pull request that deletes the feature merges just before the MVP's release (or its first candidate) is tagged, so testers pair machines from a release build. From that release the hub is in the public contract (§12.1), and it is "the first hub release" of decision 88, item 5, the first to ship protocol 1. Where the items below say M8's candidate or M8's release, read the MVP's; the file keeps its name.

**Amended 2026-11-25 (#1366):** the gate is gone as of the MVP's release. The `hub` feature, its `cfg`s, its optional dependencies, `tests/hub_gate.rs`, `just architecture`'s hub rules and the release smoke tests' no-hub checks are deleted, and every build, a release's included, has the hub. The MVP's first candidate (`0.6.0-rc.1`, #1044) is the first release that can pair, sync and serve as a hub; the hub's commands, flags, routes and protocol 1 join the public contract (§12.1) with it. The items below describe the gate as it was.

## Why

Decision 119 starts the hub now, beside M3b and M4, and every release is cut from `main` (§12.1). Without a gate, 0.5.0 (M3b) and 0.6.0 (M4) would ship half-built pairing and sync code: security code still under review, reachable from the network on `/sync/`. That first release would also start decision 88's six-month promise for protocol 1 and put the hub's commands, flags and routes in the public contract, so a flaw found later in M8 would cost a new protocol or a breaking release.

A long-lived branch would avoid both, but it would merge months of security code at once and leave CI testing the hub only on that branch. A feature keeps the code on `main`, tested on every pull request, and out of what ships.

## Decided

1. **The feature is `hub`**, the glossary's word for the machine that runs `serve --hub` (§3.1). No crate, feature or command in the workspace uses the name. It is a feature of `ladderback` and `web`, and of each workspace crate with hub code a release must not link, which `ladderback` and `web` turn on in turn. Like `pseudo` (technology.md §8.4), a library crate's `hub` is on by default, so that its own tests and mutants cover it, and the workspace dependency turns the default off.
2. **It gates every way in.** Behind it:
   - `serve --hub`, the hub's listener and TLS, every path under `/sync/` with its router and the check it has in place of the request checks (security §10.1), and the hub's own `/api/v1` routes: tokens, pending agents and their approval, the Recovery Kit's page and its Download;
   - the commands `pairing-token`, `pair`, `recovery-kit` and `hub-recover`, and the approval prompt;
   - the `[hub]` table of `config.toml` and the agent's hub client: its connection and the outbox's writer;
   - the hub's files (`hub-key`, `hub-tls-key`, `hub-recovery-key`, `hub-recovery-seed`, `hub-handovers`) and the agent's (its agent key, the key's counter file, `hub-pin`), which a release never makes or reads: a release has the agent ID alone, as it has today;
   - the hub's pages and the agent's pairing pages in `web`, with their places in the navigation;
   - the crates only the hub uses (`qrcode`, `rqrr` and any other), as optional dependencies the feature turns on, so a release doesn't link them and `cargo audit bin` lists only what ships.

   Code that only gated code calls is gated with it, down to the module, so that the build without the feature has no dead code.

   Shared surfaces stay as a release has them. A hub-only field on a shared document (`--json`, `/api/v1`, an MCP result, a webhook) is optional, left out when absent and set only by gated code. A hub-only value in a shared list (a notification event such as `agent.offline` or `agent.pending`, an error code) is accepted from outside only by a build with the hub: a release refuses it in `config.toml` as a value it doesn't know, and never emits one.
3. **It leaves data and definitions in every build.**
   - **The store's migrations, the hub's included.** A database has one schema history. The store refuses a database that holds a migration the build doesn't ship (`NewerSchema`, §9), and sqlx applies migrations by number, so a release without the hub's would refuse every database a development build had opened, and M8's release would apply the skipped ones after later ones. A merged migration is never edited (sqlx checks each one's checksum): a hub table that changes before M8 changes by a new migration, which may drop and recreate it.
   - **What a hub migration may do before M8.** It adds tables, or columns and indexes that a release neither reads nor writes, and changes nothing a release reads; a release never writes to them, so they stay empty in its database. Opening, with a release, a database that a development build with the hub has used is a downgrade, which §12.1 doesn't support: that build may have written rows a release would misread, such as other agents' operations.
   - **The default backup exclusions of decision 120** ("The hub key"): a plan whose sources hold the data folder leaves out `hub-key`, `hub-tls-key`, `hub-recovery-seed` and the agent key file in every build, so a release that backs up a data folder a development build used never puts a hub secret in a repository.
   - **The wire types and codes in `api`, and the Fluent messages.** `l10n`'s code tables match every code with no wildcard, so one table serves every build; a release never sends a hub code, since nothing that sends one is built. Every catalog ships whole: the unused-ID lint reads the source, so it counts a message that gated code uses, and translators get the hub's text as it lands rather than all at once before M8.
   - **The types in `domain` and `app`** (the token codec, fingerprints, the signing port) and the signing adapter, whose `aws-lc-rs` every release already links for TLS. No entry point of a release reaches them.
   - **The keys releases already read:** `[agent] allow-hub-restore` and `maintenance = "hub"`. Gating them would stop files that load today (§12.1). Without a hub, `maintenance = "hub"` means what it means now: this agent doesn't maintain the repository (§7.3, I1).
4. **Default and test builds have it.** `hub` is a default feature, so `just test`, `just mutants`, `just test-ui`, `just try` and `just build` all build the hub, and CI tests it on every pull request. A release's `--no-default-features` (`release_flags`, §12.1) leaves it out with no change to the flags.

   `just test-release`, which CI runs on every pull request, builds `ladderback` without it: a test of the hub's behavior is `#[cfg(feature = "hub")]`, and item 5's refusals are tested there under `cfg(not(feature = "hub"))`. The first pull request with the feature also adds clippy with `release_flags`, on the host and wasm32, to `just lint`: otherwise nothing lints the build without the hub, and its dead code would be warnings no check fails on.
5. **A release refuses what it lacks, as a build refuses a key it predates.**
   - `[hub]` in `config.toml` is refused at load as an unknown key (`ConfigError::Invalid`, `UnknownKey`, naming `hub` and its line), so `serve` doesn't start and says why. Every build meets a table it predates that way (scheduling-and-config.md, the keys of decisions 72 and 80), so the refusal needs no hub code in a release.
   - `serve --hub`, `pairing-token`, `pair`, `recovery-kit` and `hub-recover` are refused by the argument parser as unknown, before anything starts.
   - Every path under `/sync/` answers 404 in a build without the hub, where today a GET of `/sync/v1` gets the UI's page, as any path the router doesn't know does. The hub's `/api/v1` routes are absent, so a request to one gets what an unknown path gets, which holds nothing of the hub.

   `[hub]` is refused rather than ignored. Ignored, it would leave a machine the person set up to report to a hub reporting to nothing, with nothing on that machine to say so. Ignoring also needs code in the release that knows the table, which is what the gate keeps out, and a save from Settings would have to keep a table the build doesn't model.

   Refusal does stop that machine's backups until the table goes, which decision 88, item 6, forbids for a running agent and hub that disagree. It doesn't apply here: no release has spoken to a hub, so a release meets `[hub]` only when a person or a development build wrote it, and the refusal shows when the daemon starts, not weeks later.
6. **`just architecture` proves a release build lacks it**, as it does for the gallery. It reads the feature graph (`cargo tree --edges features`) and fails if a default build of `ladderback`, or of `web` for wasm32, lacks `hub`, or if a build with `release_flags` has any crate's `hub` feature. As for `fluent-pseudo`, it also fails if a release build has a crate only the hub uses (`qrcode`, `rqrr`), which a plain dependency added by mistake would bring in. Both sides are checked, so a change that blinds the check can't pass it with nothing to find. The graph includes dev-dependencies on purpose: no test support may turn `hub` on in the build `just test-release` runs either.

   `scripts/smoke-release.ts` checks the artifact as well: the release binary refuses `serve --hub` and `pairing-token`, its `serve` answers 404 on `/sync/v1`, and the wasm it serves lacks an element ID only a hub page renders. It is never a message's text, which ships in every build (item 3). The check, the smoke test's lines, the clippy run of item 4, the 404 of item 5, the gated sync routes of item 2 and the `just architecture` line in `AGENTS.md` land in the first pull request of #567's slices (#1040, slice 1), which adds the feature even with nothing behind it yet; no hub code merges before them.
7. **M8's release candidate removes the gate.** One pull request deletes the feature: its `cfg`s, its optional dependencies (made plain), and its lines in the check and the smoke test. It merges just before the first M8 candidate is tagged (§12.1, #1044), so the candidate is the build that will be released, and the maintainer's pairing test on real machines runs that build.

   Deleting the feature beats adding `--features hub` to `release_flags`: a feature every build turns on is configuration that does nothing, and `just test-release` would keep building a binary without the hub that nobody ships. From the removal, every release from `main` is an M8 candidate or M8's release: a fix another release needs then ships in one of them, and a milestone that finishes meanwhile is released with M8 or after it.
8. **Decision 88 applies from M8's full release.** Protocol 1, decision 120's handshake and the hub's commands, flags, `[hub]` table and routes first ship there (`0.7.0` as #1044 plans it). That release is "the first hub release" of decision 88, item 5, and "the first release" of decision 120. Until then they may change on `main` in place, with no new protocol number or handshake path: no release speaks them, and builds of two commits of `main` aren't promised to work together.

   From that release on, a change in meaning is a new protocol (88, item 9), a handshake's label and path never change in place, and each six-month window of item 4 starts when a full release first ships the protocol N it counts from. A release candidate is a prerelease, which the update check never offers (§10.7), and starts no window: a protocol change between two candidates goes in the second's notes, and a person running one updates the hub and its agents together.

   `[hub]`, a new optional table, needs no new `schema` by the rule the keys of decisions 72 and 80 followed; the candidate is required by the sync protocol whatever the schema does.

## Not decided here

- **M9.** Its slices that merge before M8's candidate are hub code and sit behind `hub`. Once the gate is gone, whether a part of M9 that mustn't ship half-built (leases and hub-managed maintenance, I1) needs a gate of its own is decided before its first code merges; this decision's pattern is the starting point.

## Rejected

- **A long-lived hub branch,** merged at M8: months of security code reviewed as one merge, conflicts with M3b and M4 every week, and CI testing it only there.
- **Off by default, as the gallery is:** every `just` lane and CI job would need `--features hub`, and one that missed it would pass with no hub in it. On by default, the one build without the hub is the release, which `release_flags` already defines.
- **A run-time switch** (an environment variable or a hidden flag): the code would ship, reachable by whoever finds the switch, and the protocol's promise would start with it.
- **Gating the migrations** (item 3), **ignoring `[hub]`** (item 5) and **adding `--features hub` to `release_flags` at M8** (item 7).
- **Holding releases until M8:** 0.5.0 and 0.6.0 would wait weeks for work they don't contain.

## Cost

- **Two builds until M8.** `cfg` lines where the hub meets shared code (the CLI's commands, the router, `config.toml`'s top table, the navigation), and tests that run in one lane only.
- **A shared path gated wrongly shows only without the feature,** where `just test-release`, the release clippy and the smoke test reach fewer paths than the default lanes do.
- **Every release's database holds the hub's tables** from the first hub migration, empty unless a development build with the hub wrote to it.
